//! A prediction-market factory whose listing rules are judged by the policy gate.
//!
//! A creator posts a bond and a question. The factory sends the question, its
//! closing date and its resolution source to the gate under its rulebook
//! (`markets/listing@N` in the registry) and acts on the verdict with this
//! code alone:
//!
//! | verdict | what happens |
//! |---|---|
//! | pass | the market is created; the bond stays with it |
//! | escalate | the moderator decides; the bond is held |
//! | block | no market; the bond goes back, with the rules that fired |
//! | no verdict | nothing decided; the bond goes back |
//!
//! The rules keep out what makes prediction markets dangerous: bets on a
//! person's death or harm, markets about private people, and markets whose
//! creator decides the outcome.

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, near_bindgen, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise, PromiseError};
use policy_gate_interface::{ext_policy_gate, Answer, Decision, Text};
use schemars::JsonSchema;

const CALLBACK_GAS: Gas = Gas::from_tgas(25);

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    Markets,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde", rename_all = "snake_case", tag = "status")]
pub enum Status {
    Judging,
    /// Created; the bond stays with the market.
    Open,
    /// The rulebook refused it; the bond went back.
    Refused { rules: Vec<String> },
    /// The moderator decides; the bond is held.
    Review { rules: Vec<String> },
    /// The moderator said no; the bond went back.
    Rejected,
    /// No verdict; the bond went back.
    Failed { why: String },
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde")]
pub struct Market {
    pub id: u64,
    #[schemars(with = "String")]
    pub creator: AccountId,
    pub question: String,
    /// `YYYY-MM-DD`.
    pub closes: String,
    pub resolution_source: String,
    pub status: Status,
    pub verdict_message: Option<String>,
    pub verdict_signature: Option<String>,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
pub struct PredictionMarkets {
    gate: AccountId,
    pack: String,
    moderator: AccountId,
    bond: u128,
    next_id: u64,
    markets: LookupMap<u64, Market>,
}

fn check_date(date: &str) -> Result<(), &'static str> {
    let b = date.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if b.len() == 10 && b[4] == b'-' && b[7] == b'-' && digits(0..4) && digits(5..7) && digits(8..10) {
        Ok(())
    } else {
        Err("closes is a date, YYYY-MM-DD")
    }
}

/// The text the gate judges: the whole listing, as a reader of the market sees it.
pub fn listing(question: &str, closes: &str, resolution_source: &str) -> String {
    format!("Market question: {question}\nCloses: {closes}\nResolution source: {resolution_source}")
}

#[near_bindgen]
impl PredictionMarkets {
    #[init]
    pub fn new(gate: AccountId, pack: String, moderator: AccountId, bond: U128) -> Self {
        Self { gate, pack, moderator, bond: bond.0, next_id: 0, markets: LookupMap::new(StorageKey::Markets) }
    }

    /// Create a market. Attach the bond plus the OutLayer deposit, and
    /// 300 TGas: the bond is held until the verdict, the rest goes to OutLayer
    /// and its unused part comes back to you.
    #[payable]
    pub fn create_market(&mut self, question: String, closes: String, resolution_source: String) -> Promise {
        require!((10..=300).contains(&question.trim().len()), "a question is 10–300 bytes");
        if let Err(why) = check_date(&closes) {
            env::panic_str(why);
        }
        require!((1..=200).contains(&resolution_source.trim().len()), "name a resolution source, up to 200 bytes");
        let attached = env::attached_deposit().as_yoctonear();
        require!(attached > self.bond, "attach the bond plus the OutLayer deposit");

        let creator = env::predecessor_account_id();
        let id = self.next_id;
        self.next_id += 1;
        let text = listing(&question, &closes, &resolution_source);
        self.markets.insert(
            &id,
            &Market { id, creator: creator.clone(), question, closes, resolution_source, status: Status::Judging, verdict_message: None, verdict_signature: None },
        );
        ext_policy_gate::ext(self.gate.clone())
            .with_attached_deposit(NearToken::from_yoctonear(attached - self.bond))
            .with_unused_gas_weight(1)
            .check(self.pack.clone(), Text::Plain(text), Some(creator))
            .then(Self::ext(env::current_account_id()).with_static_gas(CALLBACK_GAS).on_judged(id))
    }

    #[private]
    pub fn on_judged(&mut self, id: u64, #[callback_result] answer: Result<Answer, PromiseError>) -> Market {
        let mut market = self.markets.get(&id).unwrap_or_else(|| env::panic_str("no such market"));
        let answer = answer.unwrap_or_else(|_| Answer::Failed("the gate did not answer".to_string()));
        if let Answer::Verdict(checked) = &answer {
            market.verdict_message = Some(checked.message.clone());
            market.verdict_signature = Some(checked.signature.clone());
        }
        market.status = match answer {
            Answer::Failed(why) => {
                self.give_back(&market);
                Status::Failed { why }
            }
            Answer::Verdict(checked) => {
                let rules: Vec<String> = checked.verdict.fired.iter().map(|f| f.rule.clone()).collect();
                match checked.verdict.decision {
                    Decision::Pass => Status::Open,
                    Decision::Escalate => Status::Review { rules },
                    Decision::Block => {
                        self.give_back(&market);
                        Status::Refused { rules }
                    }
                }
            }
        };
        self.markets.insert(&id, &market);
        market
    }

    /// The moderator opens or rejects a market the rules handed to them.
    pub fn moderate(&mut self, id: u64, approve: bool) -> Market {
        require!(env::predecessor_account_id() == self.moderator, "only the moderator decides");
        let mut market = self.markets.get(&id).unwrap_or_else(|| env::panic_str("no such market"));
        require!(matches!(market.status, Status::Review { .. }), "the rules did not hand this market to the moderator");
        market.status = if approve {
            Status::Open
        } else {
            self.give_back(&market);
            Status::Rejected
        };
        self.markets.insert(&id, &market);
        market
    }

    pub fn market(&self, id: u64) -> Option<Market> {
        self.markets.get(&id)
    }

    pub fn markets(&self, from: u64, limit: u64) -> Vec<Market> {
        (from..self.next_id.min(from.saturating_add(limit.min(50)))).filter_map(|id| self.markets.get(&id)).collect()
    }

    pub fn config(&self) -> (AccountId, String, AccountId, U128) {
        (self.gate.clone(), self.pack.clone(), self.moderator.clone(), U128(self.bond))
    }
}

impl PredictionMarkets {
    /// The bond back to the creator.
    fn give_back(&self, market: &Market) {
        Promise::new(market.creator.clone()).transfer(NearToken::from_yoctonear(self.bond));
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
            .current_account_id("markets.testnet".parse().unwrap())
            .predecessor_account_id(predecessor.parse().unwrap())
            .account_balance(NearToken::from_near(100))
            .build());
    }

    fn factory_with_market() -> PredictionMarkets {
        context("markets.testnet");
        let mut f = PredictionMarkets::new("gate.testnet".parse().unwrap(), "markets/listing@1".into(), "mod.testnet".parse().unwrap(), U128(10u128.pow(24)));
        f.markets.insert(
            &0,
            &Market {
                id: 0,
                creator: "creator.testnet".parse().unwrap(),
                question: "Will a sitting head of state be assassinated before 2028?".into(),
                closes: "2027-12-31".into(),
                resolution_source: "news".into(),
                status: Status::Judging,
                verdict_message: None,
                verdict_signature: None,
            },
        );
        f.next_id = 1;
        f
    }

    fn verdict(decision: Decision, fired: &[&str]) -> Result<Answer, PromiseError> {
        Ok(Answer::Verdict(Checked {
            verdict: Verdict {
                format: "policy-judge/verdict/1".into(),
                gate: "gate.testnet".parse().unwrap(),
                for_account: "markets.testnet".parse().unwrap(),
                request: 0,
                pack: "markets/listing@1".into(),
                pack_sha256: "a".repeat(64),
                registry: "policies.zavodil.testnet".parse().unwrap(),
                block_height: 1,
                text_sha256: "b".repeat(64),
                decision,
                fired: fired.iter().map(|r| Fired { rule: r.to_string(), bp: 9694 }).collect(),
                outlayer_request: "1".into(),
                at: 1,
            },
            message: "{}".into(),
            signature: "00".into(),
        }))
    }

    #[test]
    fn a_market_on_a_persons_death_is_not_created_and_the_bond_goes_back() {
        let mut f = factory_with_market();
        let market = f.on_judged(0, verdict(Decision::Block, &["harm_to_a_person"]));
        assert_eq!(market.status, Status::Refused { rules: vec!["harm_to_a_person".into()] });
        assert_eq!(market.verdict_message.as_deref(), Some("{}"), "the signed verdict is kept with the market");
        assert_eq!(get_created_receipts().len(), 1, "the bond is sent back");
    }

    #[test]
    fn a_clean_market_opens_and_keeps_its_bond() {
        let mut f = factory_with_market();
        assert_eq!(f.on_judged(0, verdict(Decision::Pass, &[])).status, Status::Open);
        assert!(get_created_receipts().is_empty(), "nothing is sent back");
    }

    #[test]
    fn an_election_market_waits_for_the_moderator_alone() {
        let mut f = factory_with_market();
        assert!(matches!(f.on_judged(0, verdict(Decision::Escalate, &["election"])).status, Status::Review { .. }));
        context("creator.testnet");
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.moderate(0, true))).is_err());
        context("mod.testnet");
        assert_eq!(f.moderate(0, true).status, Status::Open);
    }

    #[test]
    fn no_verdict_decides_nothing_and_returns_the_bond() {
        let mut f = factory_with_market();
        assert!(matches!(f.on_judged(0, Err(PromiseError::Failed)).status, Status::Failed { .. }));
        assert_eq!(get_created_receipts().len(), 1);
    }

    #[test]
    fn a_closing_date_is_a_date() {
        assert!(check_date("2026-12-31").is_ok());
        for bad in ["2026-1-31", "31-12-2026", "2026/12/31", "soon"] {
            assert!(check_date(bad).is_err(), "{bad}");
        }
        assert_eq!(listing("Q?", "2026-12-31", "CoinGecko"), "Market question: Q?\nCloses: 2026-12-31\nResolution source: CoinGecko");
    }
}
