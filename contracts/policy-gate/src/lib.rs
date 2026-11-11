//! The policy gate: the on-chain half of an off-chain smart contract.
//!
//! Any contract asks `check(pack, text)`. The gate calls OutLayer with ONE
//! exact build of the policy judge (`WasmUrl` + its sha256, fixed at `new`),
//! which reads the rulebook from the registry, has Sage answer each rule about
//! the text inside an Intel TDX enclave, and signs a verdict with a key that
//! belongs to that build alone. The gate checks the signature on chain, checks
//! that the verdict is about this request, for this caller, under this
//! rulebook, and hands it to the caller's callback. The caller acts on the
//! decision with its own code.
//!
//! The gate has no owner and nothing to change: the build, the registry, the
//! OutLayer contract and the NEAR AI secret row are fixed at `new`, and the
//! build's keys are fetched once, from the build itself. A new build is a new
//! gate.

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::serde_json::{self, json, Value};
use near_sdk::{env, near_bindgen, require, AccountId, Gas, PanicOnDefault, Promise, PromiseError};
use policy_gate_interface::{Answer, Checked, Text, Verdict, VERDICT_FORMAT, VERDICT_PREFIX};
use schemars::JsonSchema;

/// The gate's own callback, out of what the caller attached.
const CALLBACK_GAS: Gas = Gas::from_tgas(20);
/// What a judge run may take: a registry read and one decision call. The
/// instruction bound is generous because a gate never changes it; OutLayer
/// prices it at a fraction of the run's base fee.
const RUN_SECONDS: u64 = 60;
const RUN_INSTRUCTIONS: u64 = 50_000_000_000;
/// The most bytes of text a request carries (a sealed text's base64 included).
const MAX_TEXT_BYTES: usize = 16 * 1024;

/// OutLayer's contract, as the gate calls it. Its one method takes the
/// arguments OutLayer defines, hence the lint.
mod outlayer {
    #![allow(dead_code, clippy::too_many_arguments)]
    use near_sdk::serde_json::Value;
    use near_sdk::{ext_contract, AccountId};

    #[ext_contract(ext_outlayer)]
    pub trait OutLayer {
        fn request_execution(
            &mut self,
            source: Value,
            resource_limits: Option<Value>,
            input_data: Option<String>,
            secrets_ref: Option<Value>,
            response_format: Option<String>,
            payer_account_id: Option<AccountId>,
            params: Option<Value>,
        );
    }
}
use outlayer::ext_outlayer;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
pub struct PolicyGate {
    outlayer: AccountId,
    wasm_url: String,
    wasm_sha256: String,
    /// The registry every rulebook must come from.
    registry: AccountId,
    /// The row holding NEARAI_API_KEY. Its access condition admits only this
    /// build, called only through this gate.
    secrets_owner: AccountId,
    secrets_profile: String,
    /// The judge build's `verdict` key, ed25519.
    verdict_key: Option<[u8; 32]>,
    /// The judge build's enclave key for this gate, secp256k1 compressed, hex.
    enclave_key: Option<String>,
    next_request: u64,
}

/// Everything the gate is fixed to.
#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Eq)]
#[serde(crate = "near_sdk::serde")]
pub struct Build {
    #[schemars(with = "String")]
    pub outlayer: AccountId,
    pub wasm_url: String,
    pub wasm_sha256: String,
    #[schemars(with = "String")]
    pub registry: AccountId,
    #[schemars(with = "String")]
    pub secrets_owner: AccountId,
    pub secrets_profile: String,
    /// Hex; none until `load_keys` has run.
    pub verdict_key: Option<String>,
    /// Hex, what clients seal private text to; none until `load_keys` has run.
    pub enclave_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
struct Keys {
    enclave_key: String,
    verdict_key: String,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
struct Signed {
    message: String,
    signature: String,
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[near_bindgen]
impl PolicyGate {
    #[init]
    pub fn new(
        outlayer: AccountId,
        wasm_url: String,
        wasm_sha256: String,
        registry: AccountId,
        secrets_owner: AccountId,
        secrets_profile: String,
    ) -> Self {
        require!(wasm_url.starts_with("https://"), "the judge build is fetched over https");
        require!(is_sha256_hex(&wasm_sha256), "wasm_sha256 is 64 lowercase hex characters");
        require!(!secrets_profile.is_empty(), "name the secret row's profile");
        Self {
            outlayer,
            wasm_url,
            wasm_sha256,
            registry,
            secrets_owner,
            secrets_profile,
            verdict_key: None,
            enclave_key: None,
            next_request: 0,
        }
    }

    /// Fetch the build's keys from the build itself, once. Anyone may call it;
    /// attach the OutLayer deposit and 200 TGas.
    #[payable]
    pub fn load_keys(&mut self) -> Promise {
        require!(self.verdict_key.is_none(), "the keys are loaded; a gate never changes them");
        self.run(json!({ "op": "keys" }), env::predecessor_account_id())
            .then(Self::ext(env::current_account_id()).with_static_gas(CALLBACK_GAS).on_keys())
    }

    #[private]
    pub fn on_keys(&mut self, #[callback_result] out: Result<Option<Value>, PromiseError>) -> Build {
        require!(self.verdict_key.is_none(), "the keys are loaded; a gate never changes them");
        let out = match out {
            Ok(Some(out)) => out,
            Ok(None) => env::panic_str("the judge run failed; no keys"),
            Err(_) => env::panic_str("OutLayer refused the request; no keys"),
        };
        if let Some(error) = out.get("error") {
            env::panic_str(&format!("the judge refused: {error}"));
        }
        let keys: Keys = serde_json::from_value(out).unwrap_or_else(|e| env::panic_str(&format!("the keys do not read: {e}")));
        let verdict_key: [u8; 32] = hex::decode(&keys.verdict_key)
            .ok()
            .and_then(|k| k.try_into().ok())
            .unwrap_or_else(|| env::panic_str("the verdict key is not 32 bytes of hex"));
        let enclave = hex::decode(&keys.enclave_key).unwrap_or_default();
        require!(enclave.len() == 33 && (enclave[0] == 2 || enclave[0] == 3), "the enclave key is not a compressed secp256k1 point");
        self.verdict_key = Some(verdict_key);
        self.enclave_key = Some(keys.enclave_key);
        self.build()
    }

    /// Judge `text` under the rulebook `pack` (`id@version`). Attach the
    /// OutLayer deposit (`estimate_execution_cost` on the OutLayer contract)
    /// and at least 200 TGas; the unused deposit goes to `refund_to`, or to the
    /// caller. The caller's callback receives an [`Answer`].
    #[payable]
    pub fn check(&mut self, pack: String, text: Text, refund_to: Option<AccountId>) -> Promise {
        require!(self.verdict_key.is_some(), "the gate's keys are not loaded yet: call load_keys");
        require!(pack.contains('@') && pack.len() <= 96, "pack is `id@version`");
        let size = match &text {
            Text::Plain(t) | Text::Sealed(t) => t.len(),
        };
        require!(size > 0 && size <= MAX_TEXT_BYTES, "the text is 1 to 16384 bytes");

        let caller = env::predecessor_account_id();
        let request = self.next_request;
        self.next_request += 1;
        let input = json!({ "op": "screen", "request": request, "for": caller, "pack": pack, "text": text });
        self.run(input, refund_to.unwrap_or_else(|| caller.clone()))
            .then(Self::ext(env::current_account_id()).with_static_gas(CALLBACK_GAS).on_verdict(request, caller, pack))
    }

    #[private]
    pub fn on_verdict(
        &mut self,
        request: u64,
        caller: AccountId,
        pack: String,
        #[callback_result] out: Result<Option<Value>, PromiseError>,
    ) -> Answer {
        let answer = self.answer(request, &caller, &pack, out);
        let (decision, detail) = match &answer {
            Answer::Verdict(checked) => (json!(checked.verdict.decision), json!(checked.verdict.text_sha256)),
            Answer::Failed(why) => (json!(null), json!(why)),
        };
        env::log_str(&format!(
            "EVENT_JSON:{}",
            json!({
                "standard": "policy-gate",
                "version": "1.0.0",
                "event": "verdict",
                "data": [{ "request": request, "for": caller, "pack": pack, "decision": decision, "detail": detail }],
            })
        ));
        answer
    }

    /// The verdict, when `signature` is the pinned judge's over `message` and
    /// the verdict names this gate: a verdict any contract can check again,
    /// long after it was given.
    pub fn verify(&self, message: String, signature: String) -> Option<Verdict> {
        self.signed_verdict(&message, &signature).ok()
    }

    pub fn build(&self) -> Build {
        Build {
            outlayer: self.outlayer.clone(),
            wasm_url: self.wasm_url.clone(),
            wasm_sha256: self.wasm_sha256.clone(),
            registry: self.registry.clone(),
            secrets_owner: self.secrets_owner.clone(),
            secrets_profile: self.secrets_profile.clone(),
            verdict_key: self.verdict_key.map(hex::encode),
            enclave_key: self.enclave_key.clone(),
        }
    }

    /// The key clients seal private text to, for this gate.
    pub fn enclave_key(&self) -> Option<String> {
        self.enclave_key.clone()
    }
}

impl PolicyGate {
    /// One run of the pinned build. The deposit goes on to OutLayer, and so
    /// does every unit of gas this call does not reserve.
    fn run(&self, input: Value, refund_to: AccountId) -> Promise {
        ext_outlayer::ext(self.outlayer.clone())
            .with_attached_deposit(env::attached_deposit())
            .with_unused_gas_weight(1)
            .request_execution(
                json!({ "WasmUrl": { "url": self.wasm_url, "hash": self.wasm_sha256, "build_target": "wasm32-wasip2" } }),
                Some(json!({ "max_instructions": RUN_INSTRUCTIONS, "max_memory_mb": 128, "max_execution_seconds": RUN_SECONDS })),
                Some(input.to_string()),
                Some(json!({ "account_id": self.secrets_owner, "profile": self.secrets_profile })),
                Some("Json".to_string()),
                Some(refund_to),
                None,
            )
    }

    fn answer(&self, request: u64, caller: &AccountId, pack: &str, out: Result<Option<Value>, PromiseError>) -> Answer {
        let out = match out {
            Ok(Some(out)) => out,
            Ok(None) => return Answer::Failed("the judge run failed; nothing was decided".to_string()),
            Err(_) => return Answer::Failed("OutLayer refused the request; nothing was decided".to_string()),
        };
        if let Some(error) = out.get("error") {
            return Answer::Failed(format!("the judge refused: {}", error.as_str().unwrap_or(&error.to_string())));
        }
        let signed: Signed = match serde_json::from_value(out) {
            Ok(signed) => signed,
            Err(e) => return Answer::Failed(format!("the judge's answer does not read: {e}")),
        };
        let verdict = match self.signed_verdict(&signed.message, &signed.signature) {
            Ok(verdict) => verdict,
            Err(why) => return Answer::Failed(why),
        };
        if verdict.request != request || &verdict.for_account != caller || verdict.pack != pack {
            return Answer::Failed("the verdict is about another request".to_string());
        }
        if verdict.registry != self.registry {
            return Answer::Failed(format!("the rulebook came from {}, not the gate's registry {}", verdict.registry, self.registry));
        }
        Answer::Verdict(Checked { verdict, message: signed.message, signature: signed.signature })
    }

    fn signed_verdict(&self, message: &str, signature: &str) -> Result<Verdict, String> {
        let key = self.verdict_key.ok_or("the gate's keys are not loaded")?;
        let signature: [u8; 64] = hex::decode(signature)
            .ok()
            .and_then(|s| s.try_into().ok())
            .ok_or("the signature is not 64 bytes of hex")?;
        let mut signed = VERDICT_PREFIX.to_vec();
        signed.extend_from_slice(message.as_bytes());
        if !env::ed25519_verify(&signature, &signed, &key) {
            return Err("the verdict is not signed by the pinned judge build".to_string());
        }
        let verdict: Verdict = serde_json::from_str(message).map_err(|e| format!("the verdict does not read: {e}"))?;
        if verdict.format != VERDICT_FORMAT {
            return Err(format!("the verdict's format is {}, not {VERDICT_FORMAT}", verdict.format));
        }
        if verdict.gate != env::current_account_id() {
            return Err(format!("the verdict was given to the gate {}, not this one", verdict.gate));
        }
        Ok(verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use policy_gate_interface::Decision;

    const SHA: &str = "e2b5fb1daa521dcca23633757aa69844f0c5908a3ae99c361ca75eb23e5a4e01";

    fn judge_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn gate() -> PolicyGate {
        testing_env!(VMContextBuilder::new().current_account_id("gate.testnet".parse().unwrap()).build());
        let mut gate = PolicyGate::new(
            "outlayer.testnet".parse().unwrap(),
            "https://example.com/judge.wasm".into(),
            SHA.into(),
            "policies.zavodil.testnet".parse().unwrap(),
            "keys.testnet".parse().unwrap(),
            "nearai".into(),
        );
        gate.verdict_key = Some(judge_key().verifying_key().to_bytes());
        gate.enclave_key = Some(format!("02{}", "11".repeat(32)));
        gate
    }

    fn message(gate: &str, for_account: &str, request: u64, decision: &str) -> String {
        format!(
            r#"{{"format":"policy-judge/verdict/1","gate":"{gate}","for":"{for_account}","request":{request},"pack":"grants/digital-art@1","pack_sha256":"{SHA}","registry":"policies.zavodil.testnet","block_height":9,"text_sha256":"{SHA}","decision":"{decision}","fired":[{{"rule":"off_theme","bp":9812}}],"outlayer_request":"42","at":1}}"#
        )
    }

    fn sign(message: &str) -> String {
        let mut bytes = VERDICT_PREFIX.to_vec();
        bytes.extend_from_slice(message.as_bytes());
        hex::encode(judge_key().sign(&bytes).to_bytes())
    }

    fn out(message: &str, signature: &str) -> Result<Option<Value>, PromiseError> {
        Ok(Some(json!({ "message": message, "signature": signature })))
    }

    #[test]
    fn a_verdict_signed_by_the_pinned_build_for_this_request_is_handed_on() {
        let gate = gate();
        let caller: AccountId = "fund.testnet".parse().unwrap();
        let m = message("gate.testnet", "fund.testnet", 3, "block");
        match gate.answer(3, &caller, "grants/digital-art@1", out(&m, &sign(&m))) {
            Answer::Verdict(checked) => {
                assert_eq!(checked.verdict.decision, Decision::Block);
                assert_eq!(checked.verdict.fired[0].bp, 9812);
                assert_eq!(checked.message, m, "the exact signed bytes travel with it");
            }
            other => panic!("{other:?}"),
        }
        assert!(gate.verify(m.clone(), sign(&m)).is_some(), "and it verifies again later");
    }

    #[test]
    fn a_verdict_not_from_the_pinned_build_is_refused() {
        let gate = gate();
        let caller: AccountId = "fund.testnet".parse().unwrap();
        let m = message("gate.testnet", "fund.testnet", 3, "block");
        let forged = m.replace(r#""decision":"block""#, r#""decision":"pass""#);
        let refused = gate.answer(3, &caller, "grants/digital-art@1", out(&forged, &sign(&m)));
        assert!(matches!(refused, Answer::Failed(ref why) if why.contains("not signed by the pinned judge")), "{refused:?}");

        let other_key = SigningKey::from_bytes(&[9u8; 32]);
        let mut bytes = VERDICT_PREFIX.to_vec();
        bytes.extend_from_slice(m.as_bytes());
        let by_another = hex::encode(other_key.sign(&bytes).to_bytes());
        assert!(gate.verify(m, by_another).is_none());
    }

    #[test]
    fn a_verdict_for_another_gate_caller_request_or_rulebook_is_refused() {
        let gate = gate();
        let caller: AccountId = "fund.testnet".parse().unwrap();
        let elsewhere = message("other-gate.testnet", "fund.testnet", 3, "pass");
        assert!(matches!(gate.answer(3, &caller, "grants/digital-art@1", out(&elsewhere, &sign(&elsewhere))), Answer::Failed(ref w) if w.contains("other-gate.testnet")));

        let m = message("gate.testnet", "launchpad.testnet", 3, "pass");
        assert!(matches!(gate.answer(3, &caller, "grants/digital-art@1", out(&m, &sign(&m))), Answer::Failed(ref w) if w.contains("another request")));
        let m = message("gate.testnet", "fund.testnet", 4, "pass");
        assert!(matches!(gate.answer(3, &caller, "grants/digital-art@1", out(&m, &sign(&m))), Answer::Failed(ref w) if w.contains("another request")));
        let m = message("gate.testnet", "fund.testnet", 3, "pass");
        assert!(matches!(gate.answer(3, &caller, "grants/other@1", out(&m, &sign(&m))), Answer::Failed(ref w) if w.contains("another request")));
        let m = message("gate.testnet", "fund.testnet", 3, "pass").replace("policies.zavodil.testnet", "my-rules.testnet");
        assert!(matches!(gate.answer(3, &caller, "grants/digital-art@1", out(&m, &sign(&m))), Answer::Failed(ref w) if w.contains("my-rules.testnet")));
    }

    #[test]
    fn a_failed_run_decides_nothing() {
        let gate = gate();
        let caller: AccountId = "fund.testnet".parse().unwrap();
        assert!(matches!(gate.answer(0, &caller, "p@1", Ok(None)), Answer::Failed(ref w) if w.contains("run failed")));
        assert!(matches!(gate.answer(0, &caller, "p@1", Err(PromiseError::Failed)), Answer::Failed(ref w) if w.contains("refused the request")));
        let refusal = Ok(Some(json!({ "error": "the registry holds no pack p@1" })));
        assert!(matches!(gate.answer(0, &caller, "p@1", refusal), Answer::Failed(ref w) if w.contains("no pack p@1")));
    }

    #[test]
    fn the_judge_reads_the_text_as_it_expects() {
        assert_eq!(json!(Text::Plain("x".into())), json!({ "plain": "x" }));
        assert_eq!(json!(Text::Sealed("AA==".into())), json!({ "sealed": "AA==" }));
    }
}
