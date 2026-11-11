//! A memecoin launchpad whose listing rules are judged by the policy gate.
//!
//! A creator pays the launch fee and names a token. The launchpad sends the
//! name, the ticker and the description to the gate under its rulebook
//! (`memecoins/launch@N` in the registry) and acts on the verdict with this
//! code alone:
//!
//! | verdict | what happens |
//! |---|---|
//! | pass | the token is created and its whole supply minted to the creator; the fee is kept |
//! | escalate | the moderator decides; the fee is held |
//! | block | no token; the fee goes back, with the rules that fired |
//! | no verdict | nothing decided; the fee goes back and the ticker is free |
//!
//! Tokens live in the launchpad's own ledger (`balance_of`, `transfer`).

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, near_bindgen, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise, PromiseError};
use policy_gate_interface::{ext_policy_gate, Answer, Decision, Text};
use schemars::JsonSchema;

const CALLBACK_GAS: Gas = Gas::from_tgas(25);
/// Every token's supply: one billion, 18 decimals.
const SUPPLY: u128 = 1_000_000_000 * 10u128.pow(18);

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    Launches,
    Tickers,
    Balances,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde", rename_all = "snake_case", tag = "status")]
pub enum Status {
    Judging,
    Launched,
    /// The rulebook refused it; the fee went back.
    Refused { rules: Vec<String> },
    /// The moderator decides; the fee is held.
    Review { rules: Vec<String> },
    /// The moderator said no; the fee went back.
    Rejected,
    /// No verdict; the fee went back.
    Failed { why: String },
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde")]
pub struct Launch {
    pub id: u64,
    #[schemars(with = "String")]
    pub creator: AccountId,
    pub name: String,
    pub ticker: String,
    pub description: String,
    pub status: Status,
    pub verdict_message: Option<String>,
    pub verdict_signature: Option<String>,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
pub struct MemeLaunchpad {
    gate: AccountId,
    pack: String,
    moderator: AccountId,
    launch_fee: u128,
    next_id: u64,
    launches: LookupMap<u64, Launch>,
    /// Ticker → the launch that holds it (judging, under review, or launched).
    tickers: LookupMap<String, u64>,
    /// (ticker, account) → balance.
    balances: LookupMap<(String, AccountId), u128>,
}

fn check_ticker(ticker: &str) -> Result<(), &'static str> {
    if (2..=10).contains(&ticker.len()) && ticker.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
        Ok(())
    } else {
        Err("a ticker is 2–10 capital letters and digits")
    }
}

#[near_bindgen]
impl MemeLaunchpad {
    #[init]
    pub fn new(gate: AccountId, pack: String, moderator: AccountId, launch_fee: U128) -> Self {
        Self {
            gate,
            pack,
            moderator,
            launch_fee: launch_fee.0,
            next_id: 0,
            launches: LookupMap::new(StorageKey::Launches),
            tickers: LookupMap::new(StorageKey::Tickers),
            balances: LookupMap::new(StorageKey::Balances),
        }
    }

    /// Launch a token. Attach the launch fee plus the OutLayer deposit, and
    /// 300 TGas: the fee is held until the verdict, the rest goes to OutLayer
    /// and its unused part comes back to you.
    #[payable]
    pub fn launch(&mut self, name: String, ticker: String, description: String) -> Promise {
        if let Err(why) = check_ticker(&ticker) {
            env::panic_str(why);
        }
        require!(!name.trim().is_empty() && name.len() <= 64, "a name is 1–64 bytes");
        require!(description.len() <= 1000, "a description is up to 1000 bytes");
        require!(self.tickers.get(&ticker).is_none(), "this ticker is taken");
        let attached = env::attached_deposit().as_yoctonear();
        require!(attached > self.launch_fee, "attach the launch fee plus the OutLayer deposit");

        let creator = env::predecessor_account_id();
        let id = self.next_id;
        self.next_id += 1;
        let text = format!("Token name: {name}\nTicker: {ticker}\nDescription: {description}");
        self.tickers.insert(&ticker, &id);
        self.launches.insert(
            &id,
            &Launch { id, creator: creator.clone(), name, ticker, description, status: Status::Judging, verdict_message: None, verdict_signature: None },
        );
        ext_policy_gate::ext(self.gate.clone())
            .with_attached_deposit(NearToken::from_yoctonear(attached - self.launch_fee))
            .with_unused_gas_weight(1)
            .check(self.pack.clone(), Text::Plain(text), Some(creator))
            .then(Self::ext(env::current_account_id()).with_static_gas(CALLBACK_GAS).on_judged(id))
    }

    #[private]
    pub fn on_judged(&mut self, id: u64, #[callback_result] answer: Result<Answer, PromiseError>) -> Launch {
        let mut launch = self.launches.get(&id).unwrap_or_else(|| env::panic_str("no such launch"));
        let answer = answer.unwrap_or_else(|_| Answer::Failed("the gate did not answer".to_string()));
        if let Answer::Verdict(checked) = &answer {
            launch.verdict_message = Some(checked.message.clone());
            launch.verdict_signature = Some(checked.signature.clone());
        }
        launch.status = match answer {
            Answer::Failed(why) => {
                self.give_back(&launch);
                Status::Failed { why }
            }
            Answer::Verdict(checked) => {
                let rules: Vec<String> = checked.verdict.fired.iter().map(|f| f.rule.clone()).collect();
                match checked.verdict.decision {
                    Decision::Pass => self.create(&launch),
                    Decision::Escalate => Status::Review { rules },
                    Decision::Block => {
                        self.give_back(&launch);
                        Status::Refused { rules }
                    }
                }
            }
        };
        self.launches.insert(&id, &launch);
        launch
    }

    /// The moderator launches or rejects a token the rules handed to them.
    pub fn moderate(&mut self, id: u64, approve: bool) -> Launch {
        require!(env::predecessor_account_id() == self.moderator, "only the moderator decides");
        let mut launch = self.launches.get(&id).unwrap_or_else(|| env::panic_str("no such launch"));
        require!(matches!(launch.status, Status::Review { .. }), "the rules did not hand this launch to the moderator");
        launch.status = if approve {
            self.create(&launch)
        } else {
            self.give_back(&launch);
            Status::Rejected
        };
        self.launches.insert(&id, &launch);
        launch
    }

    pub fn transfer(&mut self, ticker: String, to: AccountId, amount: U128) {
        let from = env::predecessor_account_id();
        let have = self.balances.get(&(ticker.clone(), from.clone())).unwrap_or(0);
        require!(have >= amount.0, "not enough tokens");
        self.balances.insert(&(ticker.clone(), from), &(have - amount.0));
        let theirs = self.balances.get(&(ticker.clone(), to.clone())).unwrap_or(0);
        self.balances.insert(&(ticker, to), &(theirs + amount.0));
    }

    pub fn balance_of(&self, ticker: String, account: AccountId) -> U128 {
        U128(self.balances.get(&(ticker, account)).unwrap_or(0))
    }

    pub fn launch_of(&self, id: u64) -> Option<Launch> {
        self.launches.get(&id)
    }

    pub fn launches(&self, from: u64, limit: u64) -> Vec<Launch> {
        (from..self.next_id.min(from.saturating_add(limit.min(50)))).filter_map(|id| self.launches.get(&id)).collect()
    }

    pub fn config(&self) -> (AccountId, String, AccountId, U128) {
        (self.gate.clone(), self.pack.clone(), self.moderator.clone(), U128(self.launch_fee))
    }
}

impl MemeLaunchpad {
    fn create(&mut self, launch: &Launch) -> Status {
        self.balances.insert(&(launch.ticker.clone(), launch.creator.clone()), &SUPPLY);
        Status::Launched
    }

    /// The fee back to the creator, and the ticker free again.
    fn give_back(&mut self, launch: &Launch) {
        self.tickers.remove(&launch.ticker);
        Promise::new(launch.creator.clone()).transfer(NearToken::from_yoctonear(self.launch_fee));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{get_created_receipts, VMContextBuilder};
    use near_sdk::testing_env;
    use policy_gate_interface::{Checked, Fired, Verdict};

    fn context(predecessor: &str) {
        testing_env!(VMContextBuilder::new()
            .current_account_id("launchpad.testnet".parse().unwrap())
            .predecessor_account_id(predecessor.parse().unwrap())
            .account_balance(NearToken::from_near(100))
            .build());
    }

    fn pad_with_launch(ticker: &str) -> MemeLaunchpad {
        context("launchpad.testnet");
        let mut pad = MemeLaunchpad::new("gate.testnet".parse().unwrap(), "memecoins/launch@1".into(), "mod.testnet".parse().unwrap(), U128(10u128.pow(24)));
        pad.tickers.insert(&ticker.to_string(), &0);
        pad.launches.insert(
            &0,
            &Launch {
                id: 0,
                creator: "creator.testnet".parse().unwrap(),
                name: "Some Coin".into(),
                ticker: ticker.into(),
                description: "x".into(),
                status: Status::Judging,
                verdict_message: None,
                verdict_signature: None,
            },
        );
        pad.next_id = 1;
        pad
    }

    fn verdict(decision: Decision, fired: &[&str]) -> Result<Answer, PromiseError> {
        Ok(Answer::Verdict(Checked {
            verdict: Verdict {
                format: "policy-judge/verdict/1".into(),
                gate: "gate.testnet".parse().unwrap(),
                for_account: "launchpad.testnet".parse().unwrap(),
                request: 0,
                pack: "memecoins/launch@1".into(),
                pack_sha256: "a".repeat(64),
                registry: "policies.zavodil.testnet".parse().unwrap(),
                block_height: 1,
                text_sha256: "b".repeat(64),
                decision,
                fired: fired.iter().map(|r| Fired { rule: r.to_string(), bp: 9500 }).collect(),
                outlayer_request: "1".into(),
                at: 1,
            },
            message: "{}".into(),
            signature: "00".into(),
        }))
    }

    #[test]
    fn an_offensive_token_is_not_launched_and_the_fee_goes_back() {
        let mut pad = pad_with_launch("SLUR");
        let launch = pad.on_judged(0, verdict(Decision::Block, &["slur_or_hate"]));
        assert_eq!(launch.status, Status::Refused { rules: vec!["slur_or_hate".into()] });
        assert_eq!(pad.balance_of("SLUR".into(), "creator.testnet".parse().unwrap()).0, 0, "no token");
        assert!(pad.tickers.get(&"SLUR".to_string()).is_none(), "the ticker is free again");
        assert_eq!(get_created_receipts().len(), 1, "the fee is sent back");
    }

    #[test]
    fn a_clean_token_is_minted_to_its_creator() {
        let mut pad = pad_with_launch("FROG");
        assert_eq!(pad.on_judged(0, verdict(Decision::Pass, &[])).status, Status::Launched);
        assert_eq!(pad.balance_of("FROG".into(), "creator.testnet".parse().unwrap()).0, SUPPLY);
        context("creator.testnet");
        pad.transfer("FROG".into(), "friend.testnet".parse().unwrap(), U128(5));
        assert_eq!(pad.balance_of("FROG".into(), "friend.testnet".parse().unwrap()).0, 5);
    }

    #[test]
    fn an_escalated_token_waits_for_the_moderator_alone() {
        let mut pad = pad_with_launch("CEO");
        assert!(matches!(pad.on_judged(0, verdict(Decision::Escalate, &["real_person"])).status, Status::Review { .. }));
        context("creator.testnet");
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pad.moderate(0, true))).is_err());
        context("mod.testnet");
        assert_eq!(pad.moderate(0, true).status, Status::Launched);
    }

    #[test]
    fn a_ticker_is_capitals_and_digits() {
        assert!(check_ticker("PEPE2").is_ok());
        for bad in ["P", "pepe", "PE PE", "TOOLONGTICKER"] {
            assert!(check_ticker(bad).is_err(), "{bad}");
        }
    }
}
