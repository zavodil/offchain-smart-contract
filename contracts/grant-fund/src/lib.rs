//! A grant fund whose rulebook is judged by the policy gate.
//!
//! An applicant asks for an amount and describes what it is for. The fund
//! sends the application to the gate under its rulebook (`grants/...@N` in
//! the registry) and acts on the verdict with this code alone:
//!
//! | verdict | amount | what happens |
//! |---|---|---|
//! | pass | up to `auto_cap` | paid at once |
//! | pass | over `auto_cap` | the council decides |
//! | escalate | any | the council decides |
//! | block | any | refused, with the rules that fired |
//! | no verdict | any | nothing decided; the applicant may apply again |
//!
//! Anyone may fund it with `donate`. The council can only approve or reject
//! what the rules handed to it.

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, log, near_bindgen, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise, PromiseError};
use policy_gate_interface::{ext_policy_gate, Answer, Decision, Text};
use schemars::JsonSchema;

const CALLBACK_GAS: Gas = Gas::from_tgas(25);
/// Kept for the fund's own storage; never paid out.
const RESERVE: NearToken = NearToken::from_near(1);

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    Applications,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde", rename_all = "snake_case", tag = "status")]
pub enum Status {
    /// Waiting for the gate.
    Judging,
    Paid,
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
    auto_cap: u128,
    next_id: u64,
    applications: LookupMap<u64, Application>,
}

#[near_bindgen]
impl GrantFund {
    #[init]
    pub fn new(gate: AccountId, pack: String, council: AccountId, auto_cap: U128) -> Self {
        Self { gate, pack, council, auto_cap: auto_cap.0, next_id: 0, applications: LookupMap::new(StorageKey::Applications) }
    }

    /// Add to the fund.
    #[payable]
    pub fn donate(&mut self) {
        log!("{} donated {}", env::predecessor_account_id(), env::attached_deposit());
    }

    /// Ask for `amount` yoctoNEAR. `application` says what it is for, in the
    /// clear or sealed to the gate's enclave key for this fund. Attach the
    /// OutLayer deposit and 300 TGas; the unused deposit comes back to you.
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
    pub fn on_judged(&mut self, id: u64, #[callback_result] answer: Result<Answer, PromiseError>) -> Application {
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
                Decision::Pass => self.pay(&application),
            },
        };
        self.applications.insert(&id, &application);
        application
    }

    /// The council approves or rejects an application the rules handed to it.
    pub fn council_decide(&mut self, id: u64, approve: bool) -> Application {
        require!(env::predecessor_account_id() == self.council, "only the council decides");
        let mut application = self.applications.get(&id).unwrap_or_else(|| env::panic_str("no such application"));
        require!(matches!(application.status, Status::Council { .. }), "the rules did not hand this application to the council");
        application.status = if approve { self.pay(&application) } else { Status::Rejected };
        self.applications.insert(&id, &application);
        application
    }

    pub fn application(&self, id: u64) -> Option<Application> {
        self.applications.get(&id)
    }

    pub fn applications(&self, from: u64, limit: u64) -> Vec<Application> {
        (from..self.next_id.min(from.saturating_add(limit.min(50)))).filter_map(|id| self.applications.get(&id)).collect()
    }

    /// What the fund can pay out now.
    pub fn available(&self) -> U128 {
        U128(env::account_balance().saturating_sub(RESERVE).as_yoctonear())
    }

    pub fn config(&self) -> (AccountId, String, AccountId, U128) {
        (self.gate.clone(), self.pack.clone(), self.council.clone(), U128(self.auto_cap))
    }
}

impl GrantFund {
    /// Pay, when the fund holds the amount; otherwise hand it to the council.
    fn pay(&self, application: &Application) -> Status {
        if self.available().0 < application.amount.0 {
            return Status::Council { reason: "the fund does not hold the amount now".to_string() };
        }
        Promise::new(application.applicant.clone()).transfer(NearToken::from_yoctonear(application.amount.0));
        Status::Paid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use policy_gate_interface::{Checked, Fired, Verdict};

    fn context(predecessor: &str, balance: u128) {
        testing_env!(VMContextBuilder::new()
            .current_account_id("fund.testnet".parse().unwrap())
            .predecessor_account_id(predecessor.parse().unwrap())
            .account_balance(NearToken::from_yoctonear(balance))
            .build());
    }

    fn fund_with(amount: u128) -> GrantFund {
        context("fund.testnet", 100 * 10u128.pow(24));
        let mut fund = GrantFund::new("gate.testnet".parse().unwrap(), "grants/digital-art@1".into(), "council.testnet".parse().unwrap(), U128(5 * 10u128.pow(24)));
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
                pack: "grants/digital-art@1".into(),
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

    const NEAR: u128 = 10u128.pow(24);

    #[test]
    fn an_off_theme_exhibition_is_refused_and_nothing_is_paid() {
        let mut fund = fund_with(2 * NEAR);
        let application = fund.on_judged(0, verdict(Decision::Block, &["off_theme"]));
        assert_eq!(application.status, Status::Refused { rules: vec!["off_theme".into()] });
        assert_eq!(application.verdict_message.as_deref(), Some("{}"), "the signed verdict is kept with the application");
    }

    #[test]
    fn a_passing_application_within_the_cap_is_paid_and_a_larger_one_goes_to_the_council() {
        let mut fund = fund_with(2 * NEAR);
        assert_eq!(fund.on_judged(0, verdict(Decision::Pass, &[])).status, Status::Paid);

        let mut fund = fund_with(20 * NEAR);
        assert!(matches!(fund.on_judged(0, verdict(Decision::Pass, &[])).status, Status::Council { .. }));
        context("council.testnet", 100 * NEAR);
        assert_eq!(fund.council_decide(0, true).status, Status::Paid);
    }

    #[test]
    fn an_escalating_rule_hands_it_to_the_council_who_alone_may_decide() {
        let mut fund = fund_with(2 * NEAR);
        assert!(matches!(fund.on_judged(0, verdict(Decision::Escalate, &["large_venue"])).status, Status::Council { ref reason } if reason.contains("large_venue")));
        context("artist.testnet", 100 * NEAR);
        let stranger = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fund.council_decide(0, true)));
        assert!(stranger.is_err(), "only the council decides");
        context("council.testnet", 100 * NEAR);
        assert_eq!(fund.council_decide(0, false).status, Status::Rejected);
    }

    #[test]
    fn no_verdict_decides_nothing() {
        let mut fund = fund_with(2 * NEAR);
        assert!(matches!(fund.on_judged(0, Ok(Answer::Failed("the judge run failed".into()))).status, Status::Failed { .. }));
        let mut fund = fund_with(2 * NEAR);
        assert!(matches!(fund.on_judged(0, Err(PromiseError::Failed)).status, Status::Failed { .. }));
    }
}
