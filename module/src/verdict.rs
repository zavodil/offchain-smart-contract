//! The verdict: what the judge decided, about which text, under which
//! rulebook, for which contract — signed with the `verdict` key, which
//! belongs to this exact build and to no caller (`bind: "wasm"`,
//! `caller: "none"`). Every caller of the build drives the same key, so the
//! verdict names its caller from the host, never from the input: `gate` is
//! the contract that called OutLayer (`NEAR_PREDECESSOR_ID`), and `for` is
//! that gate's word for the contract that asked it. A gate accepts only
//! verdicts that name it.
//!
//! The signed bytes are `PREFIX` followed by `message`, the verdict's compact
//! JSON exactly as the output carries it. Nothing is re-serialised to verify:
//! a contract checks the signature over the string it was given, then parses
//! that same string.

use crate::screening::Decision;
use outlayer::signing_keys;
use serde::Serialize;

pub const PREFIX: &[u8] = b"policy-judge/verdict/1\n";
pub const FORMAT: &str = "policy-judge/verdict/1";
const KEY_PATH: &str = "verdict";

#[derive(Debug, Serialize)]
pub struct Verdict {
    pub format: &'static str,
    /// The contract that called OutLayer, from the host.
    pub gate: String,
    /// The contract the gate asked for.
    #[serde(rename = "for")]
    pub for_account: String,
    /// The gate's number for the request.
    pub request: u64,
    /// `id@version`.
    pub pack: String,
    pub pack_sha256: String,
    pub registry: String,
    pub block_height: u64,
    /// SHA-256 of the text judged (the plaintext, when it came sealed).
    pub text_sha256: String,
    pub decision: Decision,
    /// The rules that fired, strictest action first.
    pub fired: Vec<Fired>,
    /// OutLayer's request id: its attestation names the build that ran.
    pub outlayer_request: String,
    pub at: u64,
}

#[derive(Debug, Serialize)]
pub struct Fired {
    pub rule: String,
    /// P(yes) in basis points.
    pub bp: u16,
}

/// What the run prints. Members in alphabetical order.
#[derive(Debug, Serialize)]
pub struct Signed {
    pub message: String,
    /// Hex, 64 bytes of ed25519.
    pub signature: String,
}

pub fn sign(verdict: &Verdict) -> Result<Signed, String> {
    let message = serde_json::to_string(verdict).map_err(|e| format!("the verdict does not encode: {e}"))?;
    let mut bytes = PREFIX.to_vec();
    bytes.extend_from_slice(message.as_bytes());
    let signature = signing_keys::sign(KEY_PATH, None, &bytes).map_err(|e| format!("the verdict could not be signed: {e}"))?;
    Ok(Signed { message, signature: hex::encode(signature) })
}

/// The verdict key, hex: what a gate pins.
pub fn public_key() -> Result<String, String> {
    signing_keys::public_key(KEY_PATH, None)
        .map(hex::encode)
        .map_err(|e| format!("the verdict key is not available to this run: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_is_compact_json_with_a_fixed_member_order() {
        let verdict = Verdict {
            format: FORMAT,
            gate: "gate.testnet".into(),
            for_account: "fund.testnet".into(),
            request: 7,
            pack: "grants/digital-art@1".into(),
            pack_sha256: "ab".into(),
            registry: "policies.zavodil.testnet".into(),
            block_height: 9,
            text_sha256: "cd".into(),
            decision: Decision::Block,
            fired: vec![Fired { rule: "off_theme".into(), bp: 9812 }],
            outlayer_request: "42".into(),
            at: 1,
        };
        let message = serde_json::to_string(&verdict).unwrap();
        assert!(message.starts_with(r#"{"format":"policy-judge/verdict/1","gate":"gate.testnet","for":"fund.testnet","request":7,"#));
        assert!(message.contains(r#""decision":"block","fired":[{"rule":"off_theme","bp":9812}]"#));
        assert!(!message.contains(": ") && !message.contains(", "), "compact");
        assert!(message.len() < 700, "the verdict fits the 1024 bytes OutLayer resumes a contract with: {}", message.len());
    }
}
