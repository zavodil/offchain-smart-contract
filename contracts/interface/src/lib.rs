//! What a contract needs to call the policy gate.
//!
//! A contract asks `check(pack, text)` and, in its own callback, receives an
//! [`Answer`]: a verdict the gate has already checked — signed by the pinned
//! judge build, about this request, for this caller — or the reason there is
//! none. The contract then acts on [`Verdict::decision`] with its own code.
//!
//! ```ignore
//! ext_policy_gate::ext(self.gate.clone())
//!     .with_attached_deposit(env::attached_deposit())
//!     .with_unused_gas_weight(1)
//!     .check("grants/digital-art@2".into(), Text::Plain(application), Some(applicant))
//!     .then(Self::ext(env::current_account_id()).with_static_gas(Gas::from_tgas(20)).on_judged(id))
//! ```

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{ext_contract, AccountId};
use schemars::JsonSchema;

/// The prefix the judge signs before a verdict's message.
pub const VERDICT_PREFIX: &[u8] = b"policy-judge/verdict/1\n";
/// The verdict format this interface reads.
pub const VERDICT_FORMAT: &str = "policy-judge/verdict/1";

/// The text to judge.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde", rename_all = "snake_case")]
pub enum Text {
    /// Public text: an application, a token's name and description, a post.
    Plain(String),
    /// Base64 of an ECIES blob sealed to the gate's `enclave_key`, holding
    /// `{"for": <the calling contract>, "text": ...}`. Only the pinned judge
    /// build, called by this gate, opens it, and only for that contract.
    Sealed(String),
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde", rename_all = "snake_case")]
pub enum Decision {
    /// No rule fired.
    Pass,
    /// A rule that hands the case to people fired, and none that refuses.
    Escalate,
    /// A rule that refuses fired.
    Block,
}

/// A rule that fired, and P(yes) in basis points.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde")]
pub struct Fired {
    pub rule: String,
    pub bp: u16,
}

/// The judge's verdict, as it signed it.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde")]
pub struct Verdict {
    pub format: String,
    /// The gate that called the judge.
    #[schemars(with = "String")]
    pub gate: AccountId,
    /// The contract that asked the gate.
    #[serde(rename = "for")]
    #[schemars(with = "String")]
    pub for_account: AccountId,
    /// The gate's number for the request.
    pub request: u64,
    /// The rulebook, `id@version`.
    pub pack: String,
    /// SHA-256 of the rulebook's bytes, as the registry holds them.
    pub pack_sha256: String,
    #[schemars(with = "String")]
    pub registry: AccountId,
    pub block_height: u64,
    /// SHA-256 of the text judged (the plaintext, when it came sealed).
    pub text_sha256: String,
    pub decision: Decision,
    /// The rules that fired, strictest first.
    pub fired: Vec<Fired>,
    /// OutLayer's request id: its attestation names the build that ran.
    pub outlayer_request: String,
    /// Unix seconds, from the enclave.
    pub at: u64,
}

/// A verdict the gate checked, with the exact bytes it checked: any contract
/// can check them again later with the gate's `verify`.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde")]
pub struct Checked {
    pub verdict: Verdict,
    /// The verdict's JSON, exactly as signed.
    pub message: String,
    /// Hex, ed25519 over `VERDICT_PREFIX` + `message`.
    pub signature: String,
}

/// What `check` gives the caller's callback.
#[allow(clippy::large_enum_variant)]
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde", rename_all = "snake_case")]
pub enum Answer {
    Verdict(Checked),
    /// No verdict: the judge refused the request, or the run failed. Nothing
    /// was decided; the caller may ask again.
    Failed(String),
}

impl Answer {
    pub fn decision(&self) -> Option<Decision> {
        match self {
            Answer::Verdict(checked) => Some(checked.verdict.decision),
            Answer::Failed(_) => None,
        }
    }
}

#[ext_contract(ext_policy_gate)]
#[allow(dead_code)]
pub trait PolicyGate {
    /// Judge `text` under `pack`. Attach the OutLayer deposit
    /// (`outlayer.estimate_execution_cost`) and at least 200 TGas; the unused
    /// deposit goes to `refund_to`, or to the caller.
    fn check(&mut self, pack: String, text: Text, refund_to: Option<AccountId>) -> Answer;
    /// The verdict, when `signature` is the pinned judge's over `message` and
    /// the verdict names this gate.
    fn verify(&self, message: String, signature: String) -> Option<Verdict>;
}
