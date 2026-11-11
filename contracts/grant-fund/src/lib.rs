//! A grant fund in USDC whose rulebook is judged by the policy gate.
//!
//! Patrons fund it with `ft_transfer_call` of the token. An applicant asks for
//! an amount and describes what it is for. The fund sends the application to
//! the gate under its rulebook (`grants/...@N` in the registry) and acts on the
//! verdict with this code alone:
//!
//! | verdict | amount | what happens |
//! |---|---|---|
//! | pass | up to `auto_cap` | paid at once, in the same transaction |
//! | pass | over `auto_cap` | the council decides |
//! | escalate | any | the council decides |
//! | block | any | refused, with the rules that fired |
//! | no verdict | any | nothing decided; the applicant may apply again |
//!
//! A payout is one rule: the amount leaves the fund's balance, the token
//! transfers it, and if the transfer fails the amount comes back to the
//! balance and the applicant can claim again. The council can only approve or
//! reject what the rules handed to it.

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, ext_contract, log, near_bindgen, require, AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseError, PromiseOrValue};
use policy_gate_interface::{ext_policy_gate, Answer, Decision, Text};
use schemars::JsonSchema;

/// The callback's share of the caller's 300 TGas: OutLayer's run takes the
/// rest. A payout from the callback reserves the token transfer and the
/// bookkeeping after it out of this share.
const CALLBACK_GAS: Gas = Gas::from_tgas(30);
const TRANSFER_GAS: Gas = Gas::from_tgas(7);
const PAID_GAS: Gas = Gas::from_tgas(5);

#[ext_contract(ext_ft)]
#[allow(dead_code)]
trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

/// The book's storage prefix: its own bytes, so it never reads records a
/// fund on this account kept under another layout.
const APPLICATIONS: &[u8] = b"applications-usdc";

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde", rename_all = "snake_case", tag = "status")]
pub enum Status {
    /// Waiting for the gate.
    Judging,
    /// The transfer is on its way.
    Paying,
    Paid,
    /// The transfer failed (the applicant is not registered with the token);
    /// the amount is back in the fund and the applicant may `claim`.
    Unpaid { why: String },
    /// The rulebook refused it; the rules that fired.
    Refused { rules: Vec<String> },
    /// Handed to the council: by an escalating rule, or by the amount.
    Council { reason: String },
    /// The council said no.
    Rejected,
    /// No verdict came back; nothing was decided.
    Failed { why: String },
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde")]
pub struct Application {
    pub id: u64,
    #[schemars(with = "String")]
    pub applicant: AccountId,
    /// In the token's units (USDC: 6 decimals, 1_000_000 = $1).
    #[schemars(with = "String")]
    pub amount: U128,
    pub status: Status,
    /// The signed verdict, exactly as the gate checked it: anyone can check
    /// it again with the gate's `verify`.
    pub verdict_message: Option<String>,
    pub verdict_signature: Option<String>,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
pub struct GrantFund {
    gate: AccountId,
    pack: String,
    council: AccountId,
    token: AccountId,
    auto_cap: u128,
    /// What the fund holds and has not promised to a payout in flight.
    balance: u128,
    next_id: u64,
    applications: LookupMap<u64, Application>,
}

#[near_bindgen]
impl GrantFund {
    /// The fund's account initialises it; a fund that ran before starts over
    /// with an empty book.
    #[init(ignore_state)]
    #[private]
    pub fn new(gate: AccountId, pack: String, council: AccountId, token: AccountId, auto_cap: U128) -> Self {
        Self { gate, pack, council, token, auto_cap: auto_cap.0, balance: 0, next_id: 0, applications: LookupMap::new(APPLICATIONS) }
    }

    /// The fund's account opens a new book under the book's own prefix and
    /// keeps everything else, the balance included.
    #[init(ignore_state)]
    #[private]
    pub fn start_new_book() -> Self {
        let old: Self = env::state_read().unwrap_or_else(|| env::panic_str("no fund to carry over"));
        Self { next_id: 0, applications: LookupMap::new(APPLICATIONS), ..old }
    }

    /// NEP-141: a patron funds the fund by `ft_transfer_call` of its token.
    pub fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        require!(env::predecessor_account_id() == self.token, "the fund holds only its own token");
        self.balance += amount.0;
        log!("{sender_id} funded {} ({msg})", amount.0);
        PromiseOrValue::Value(U128(0))
    }

    /// Ask for `amount` of the token. `application` says what it is for, in the
    /// clear or sealed to the gate's enclave key for this fund. Attach the
    /// OutLayer deposit in NEAR and 300 TGas; the unused deposit comes back.
    #[payable]
    pub fn apply(&mut self, amount: U128, application: Text) -> Promise {
        require!(amount.0 > 0, "ask for an amount");
        let applicant = env::predecessor_account_id();
        let id = self.next_id;
        self.next_id += 1;
        self.applications.insert(
            &id,
            &Application { id, applicant: applicant.clone(), amount, status: Status::Judging, verdict_message: None, verdict_signature: None },
        );
        ext_policy_gate::ext(self.gate.clone())
            .with_attached_deposit(env::attached_deposit())
            .with_unused_gas_weight(1)
            .check(self.pack.clone(), application, Some(applicant))
            .then(Self::ext(env::current_account_id()).with_static_gas(CALLBACK_GAS).on_judged(id))
    }

    #[private]
    pub fn on_judged(&mut self, id: u64, #[callback_result] answer: Result<Answer, PromiseError>) -> PromiseOrValue<Application> {
        let mut application = self.applications.get(&id).unwrap_or_else(|| env::panic_str("no such application"));
        let answer = answer.unwrap_or_else(|_| Answer::Failed("the gate did not answer".to_string()));
        if let Answer::Verdict(checked) = &answer {
            application.verdict_message = Some(checked.message.clone());
            application.verdict_signature = Some(checked.signature.clone());
        }
        application.status = match answer {
            Answer::Failed(why) => Status::Failed { why },
            Answer::Verdict(checked) => match checked.verdict.decision {
                Decision::Block => Status::Refused { rules: checked.verdict.fired.iter().map(|f| f.rule.clone()).collect() },
                Decision::Escalate => Status::Council {
                    reason: format!("the rulebook asks for people: {}", checked.verdict.fired.iter().map(|f| f.rule.as_str()).collect::<Vec<_>>().join(", ")),
                },
                Decision::Pass if application.amount.0 > self.auto_cap => Status::Council { reason: "over the amount paid without the council".to_string() },
                Decision::Pass => return self.pay(application),
            },
        };
        self.applications.insert(&id, &application);
        PromiseOrValue::Value(application)
    }

    /// The council approves or rejects an application the rules handed to it.
    pub fn council_decide(&mut self, id: u64, approve: bool) -> PromiseOrValue<Application> {
        require!(env::predecessor_account_id() == self.council, "only the council decides");
        let mut application = self.applications.get(&id).unwrap_or_else(|| env::panic_str("no such application"));
        require!(matches!(application.status, Status::Council { .. }), "the rules did not hand this application to the council");
        if approve {
            return self.pay(application);
        }
        application.status = Status::Rejected;
        self.applications.insert(&id, &application);
        PromiseOrValue::Value(application)
    }

    /// An applicant whose payout failed asks again, once registered with the token.
    pub fn claim(&mut self, id: u64) -> PromiseOrValue<Application> {
        let application = self.applications.get(&id).unwrap_or_else(|| env::panic_str("no such application"));
        require!(env::predecessor_account_id() == application.applicant, "only the applicant claims");
        require!(matches!(application.status, Status::Unpaid { .. }), "nothing to claim");
        self.pay(application)
    }

    #[private]
    pub fn on_paid(&mut self, id: u64, #[callback_result] sent: Result<(), PromiseError>) -> Application {
        let mut application = self.applications.get(&id).unwrap_or_else(|| env::panic_str("no such application"));
        application.status = match sent {
            Ok(()) => Status::Paid,
            Err(_) => {
                self.balance += application.amount.0;
                Status::Unpaid { why: "the token refused the transfer: register with the token (storage_deposit), then claim".to_string() }
            }
        };
        self.applications.insert(&id, &application);
        application
    }

    pub fn application(&self, id: u64) -> Option<Application> {
        self.applications.get(&id)
    }

    pub fn applications(&self, from: u64, limit: u64) -> Vec<Application> {
        (from..self.next_id.min(from.saturating_add(limit.min(50)))).filter_map(|id| self.applications.get(&id)).collect()
    }

    /// What the fund can pay out now, in the token's units.
    pub fn available(&self) -> U128 {
        U128(self.balance)
    }

    pub fn config(&self) -> (AccountId, String, AccountId, AccountId, U128) {
        (self.gate.clone(), self.pack.clone(), self.council.clone(), self.token.clone(), U128(self.auto_cap))
    }
}

impl GrantFund {
    /// Pay, when the fund holds the amount; otherwise hand it to the council.
    fn pay(&mut self, mut application: Application) -> PromiseOrValue<Application> {
        if self.balance < application.amount.0 {
            application.status = Status::Council { reason: "the fund does not hold the amount now".to_string() };
            self.applications.insert(&application.id, &application);
            return PromiseOrValue::Value(application);
        }
        self.balance -= application.amount.0;
        application.status = Status::Paying;
        self.applications.insert(&application.id, &application);
        PromiseOrValue::Promise(
            ext_ft::ext(self.token.clone())
                .with_attached_deposit(NearToken::from_yoctonear(1))
                .with_static_gas(TRANSFER_GAS)
                .ft_transfer(application.applicant.clone(), application.amount, Some(format!("grant {}", application.id)))
                .then(Self::ext(env::current_account_id()).with_static_gas(PAID_GAS).on_paid(application.id)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{get_created_receipts, VMContextBuilder};
    use near_sdk::testing_env;
    use policy_gate_interface::{Checked, Fired, Verdict};

    const USD: u128 = 1_000_000;

    fn context(predecessor: &str) {
        testing_env!(VMContextBuilder::new()
            .current_account_id("fund.testnet".parse().unwrap())
            .predecessor_account_id(predecessor.parse().unwrap())
            .account_balance(NearToken::from_near(10))
            .build());
    }

    fn fund_with(amount: u128, balance: u128) -> GrantFund {
        context("fund.testnet");
        let mut fund = GrantFund::new(
            "gate.testnet".parse().unwrap(),
            "grants/digital-art@2".into(),
            "council.testnet".parse().unwrap(),
            "usdc.testnet".parse().unwrap(),
            U128(250 * USD),
        );
        fund.balance = balance;
        fund.applications.insert(
            &0,
            &Application { id: 0, applicant: "artist.testnet".parse().unwrap(), amount: U128(amount), status: Status::Judging, verdict_message: None, verdict_signature: None },
        );
        fund.next_id = 1;
        fund
    }

    fn verdict(decision: Decision, fired: &[&str]) -> Result<Answer, PromiseError> {
        Ok(Answer::Verdict(Checked {
            verdict: Verdict {
                format: "policy-judge/verdict/1".into(),
                gate: "gate.testnet".parse().unwrap(),
                for_account: "fund.testnet".parse().unwrap(),
                request: 0,
                pack: "grants/digital-art@2".into(),
                pack_sha256: "a".repeat(64),
                registry: "policies.zavodil.testnet".parse().unwrap(),
                block_height: 1,
                text_sha256: "b".repeat(64),
                decision,
                fired: fired.iter().map(|r| Fired { rule: r.to_string(), bp: 9000 }).collect(),
                outlayer_request: "1".into(),
                at: 1,
            },
            message: "{}".into(),
            signature: "00".into(),
        }))
    }

    fn value(answer: PromiseOrValue<Application>) -> Application {
        match answer {
            PromiseOrValue::Value(a) => a,
            PromiseOrValue::Promise(_) => panic!("a promise, not a value"),
        }
    }

    #[test]
    fn an_off_theme_exhibition_is_refused_and_nothing_is_paid() {
        let mut fund = fund_with(100 * USD, 1000 * USD);
        let application = value(fund.on_judged(0, verdict(Decision::Block, &["off_theme"])));
        assert_eq!(application.status, Status::Refused { rules: vec!["off_theme".into()] });
        assert_eq!(application.verdict_message.as_deref(), Some("{}"), "the signed verdict is kept with the application");
        assert_eq!(fund.available().0, 1000 * USD);
    }

    #[test]
    fn a_passing_application_within_the_cap_is_paid_through_the_token() {
        let mut fund = fund_with(100 * USD, 1000 * USD);
        assert!(matches!(fund.on_judged(0, verdict(Decision::Pass, &[])), PromiseOrValue::Promise(_)));
        assert_eq!(fund.application(0).unwrap().status, Status::Paying);
        assert_eq!(fund.available().0, 900 * USD, "the amount leaves the balance before the transfer");
        assert!(!get_created_receipts().is_empty(), "an ft_transfer is sent");
        assert_eq!(fund.on_paid(0, Ok(())).status, Status::Paid);
    }

    #[test]
    fn a_failed_transfer_puts_the_amount_back_and_the_applicant_can_claim() {
        let mut fund = fund_with(100 * USD, 1000 * USD);
        let _ = fund.on_judged(0, verdict(Decision::Pass, &[]));
        assert!(matches!(fund.on_paid(0, Err(PromiseError::Failed)).status, Status::Unpaid { .. }));
        assert_eq!(fund.available().0, 1000 * USD);
        context("someone.testnet");
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fund.claim(0))).is_err(), "only the applicant claims");
        context("artist.testnet");
        assert!(matches!(fund.claim(0), PromiseOrValue::Promise(_)));
    }

    #[test]
    fn over_the_cap_or_escalated_goes_to_the_council_who_alone_may_decide() {
        let mut fund = fund_with(300 * USD, 1000 * USD);
        assert!(matches!(value(fund.on_judged(0, verdict(Decision::Pass, &[]))).status, Status::Council { .. }));
        context("artist.testnet");
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fund.council_decide(0, true))).is_err());
        context("council.testnet");
        assert_eq!(value(fund.council_decide(0, false)).status, Status::Rejected);

        let mut fund = fund_with(100 * USD, 1000 * USD);
        assert!(matches!(value(fund.on_judged(0, verdict(Decision::Escalate, &["third_party_payment"]))).status, Status::Council { ref reason } if reason.contains("third_party_payment")));
    }

    #[test]
    fn a_fund_without_the_amount_asks_the_council_and_only_its_token_funds_it() {
        let mut fund = fund_with(100 * USD, 50 * USD);
        assert!(matches!(value(fund.on_judged(0, verdict(Decision::Pass, &[]))).status, Status::Council { ref reason } if reason.contains("does not hold")));
        context("other-token.testnet");
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fund.ft_on_transfer("patron.testnet".parse().unwrap(), U128(5), String::new()))).is_err());
        context("usdc.testnet");
        let _ = fund.ft_on_transfer("patron.testnet".parse().unwrap(), U128(500 * USD), "donate".into());
        assert_eq!(fund.available().0, 550 * USD);
    }

    #[test]
    fn no_verdict_decides_nothing() {
        let mut fund = fund_with(100 * USD, 1000 * USD);
        assert!(matches!(value(fund.on_judged(0, Err(PromiseError::Failed))).status, Status::Failed { .. }));
    }
}
