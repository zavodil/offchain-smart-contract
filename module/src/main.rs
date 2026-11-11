//! Policy judge: the off-chain half of an off-chain smart contract.
//!
//! A contract on NEAR (the gate) calls OutLayer with this exact build. Inside
//! an Intel TDX enclave the judge reads a rulebook from the registry contract
//! by `id@version`, asks Sage each rule about the text as a calibrated
//! yes/no, takes the strictest fired action, and answers with a verdict signed
//! by a key that belongs to this build alone. The gate checks the signature on
//! chain, and the contract that asked acts on the decision: pay, refuse, or
//! hand the case to people.
//!
//! | `op` | input | output |
//! |---|---|---|
//! | `keys` | — | `{enclave_key, verdict_key}`: the key clients seal private text to (this build and this gate), and the key verdicts are signed with (this build) |
//! | `screen` | `{request, for, pack, text}`: the gate's request number, the contract it asks for, `id@version`, and `{"plain": text}` or `{"sealed": base64}` | `{message, signature}`: the verdict's JSON and its ed25519 signature over `policy-judge/verdict/1\n` + `message` |
//!
//! The build runs only from a contract: the `inbox` key belongs to the
//! calling contract (`caller: "predecessor"`), and a run with none is refused
//! before it starts. The network is NEAR AI alone; the registry is read
//! through the worker's NEAR RPC.

mod envelope;
mod http;
mod nearai;
mod pack;
mod screening;
mod verdict;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use outlayer::env;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The manifest, embedded so the build's hash covers it: NEAR AI as the only
/// host, and the two keys bound to this build.
#[cfg(target_family = "wasm")]
#[used]
#[link_section = "outlayer.manifest"]
static OUTLAYER_MANIFEST: [u8; include_bytes!("../manifest.json").len()] = *include_bytes!("../manifest.json");

/// Every operation, as the manifest lists them.
pub const OPERATIONS: &[&str] = &["keys", "screen"];

/// The longest text judged, in characters.
const MAX_TEXT_CHARS: usize = 8_000;

#[derive(Deserialize)]
struct Named {
    op: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Screen {
    #[allow(dead_code)]
    op: String,
    request: u64,
    #[serde(rename = "for")]
    for_account: String,
    pack: String,
    text: Text,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Text {
    Plain(String),
    /// Base64 of an ECIES blob sealed to `enclave_key`, holding `{"for", "text"}`.
    Sealed(String),
}

fn main() {
    let input = env::input();
    let answer = match serde_json::from_slice::<Named>(&input) {
        Err(e) => Err(format!("the input is not an operation (`{{\"op\": ...}}`): {e}")),
        Ok(named) => run(&named.op, &input),
    };
    let _ = env::output_json(&answer.unwrap_or_else(|error| json!({ "error": error })));
}

fn run(op: &str, input: &[u8]) -> Result<Value, String> {
    match op {
        "keys" => Ok(json!({ "enclave_key": envelope::enclave_public_key()?, "verdict_key": verdict::public_key()? })),
        "screen" => {
            let screen: Screen = serde_json::from_slice(input).map_err(|e| format!("`screen` takes request, for, pack and text: {e}"))?;
            let signed = self::screen(screen)?;
            serde_json::to_value(signed).map_err(|e| format!("the output does not encode: {e}"))
        }
        other => Err(format!("{other:?} is not an operation of this build ({})", OPERATIONS.join(", "))),
    }
}

fn screen(input: Screen) -> Result<verdict::Signed, String> {
    let gate = host_var("NEAR_PREDECESSOR_ID")
        .ok_or("the judge answers a contract: this run carries no calling contract (NEAR_PREDECESSOR_ID)")?;
    let text = match input.text {
        Text::Plain(text) => text,
        Text::Sealed(sealed) => {
            let blob = BASE64.decode(sealed.trim()).map_err(|_| "`sealed` is not base64".to_string())?;
            envelope::open(&blob, &input.for_account)?
        }
    };
    if text.trim().is_empty() || text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("the text is 1–{MAX_TEXT_CHARS} characters"));
    }

    let reference = pack::PackRef::parse(&input.pack)?;
    let loaded = pack::load(&pack::default_registry(), &reference)?;
    let api = nearai::Client::from_env()?;
    let judgment = screening::judge(&api, &text, &loaded)?;

    let mut fired: Vec<(&screening::Outcome, usize)> =
        judgment.outcomes.iter().enumerate().filter(|(_, o)| o.fired).map(|(i, o)| (o, i)).collect();
    fired.sort_by(|(a, i), (b, j)| b.action.cmp(&a.action).then(i.cmp(j)));

    verdict::sign(&verdict::Verdict {
        format: verdict::FORMAT,
        gate,
        for_account: input.for_account,
        request: input.request,
        pack: reference.name(),
        pack_sha256: loaded.sha256,
        registry: loaded.registry,
        block_height: loaded.block_height,
        text_sha256: sha256_hex(text.as_bytes()),
        decision: judgment.decision,
        fired: fired.into_iter().map(|(o, _)| verdict::Fired { rule: o.id.clone(), bp: o.p_yes_bp }).collect(),
        outlayer_request: host_var("NEAR_REQUEST_ID").unwrap_or_default(),
        at: unix_now(),
    })
}

/// A variable the host sets, or none when it is empty.
fn host_var(name: &str) -> Option<String> {
    env::var(name).filter(|v| !v.trim().is_empty())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_takes_its_fields_and_nothing_else() {
        let ok: Screen = serde_json::from_str(
            r#"{"op":"screen","request":3,"for":"fund.testnet","pack":"grants/digital-art@1","text":{"plain":"A gallery show"}}"#,
        )
        .unwrap();
        assert!(matches!(ok.text, Text::Plain(ref t) if t == "A gallery show"));
        let sealed: Screen = serde_json::from_str(r#"{"op":"screen","request":3,"for":"f","pack":"p@1","text":{"sealed":"AA=="}}"#).unwrap();
        assert!(matches!(sealed.text, Text::Sealed(_)));
        assert!(serde_json::from_str::<Screen>(r#"{"op":"screen","request":3,"for":"f","pack":"p@1","text":{"plain":"x"},"gate":"me"}"#).is_err(), "the gate is the host's to say");
        assert!(serde_json::from_str::<Screen>(r#"{"op":"screen","request":3,"for":"f","pack":"p@1","text":{"html":"x"}}"#).is_err());
    }

    #[test]
    fn the_manifest_and_the_code_name_the_same_operations_and_keys() {
        let manifest: Value = serde_json::from_str(include_str!("../manifest.json")).unwrap();
        let listed: Vec<&str> = manifest["operations"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(listed, OPERATIONS);
        assert_eq!(manifest["signing_keys"][0]["path"], "verdict");
        assert_eq!(manifest["signing_keys"][0]["bind"], "wasm", "the verdict key belongs to this build");
        assert_eq!(manifest["signing_keys"][0]["caller"], "none", "one verdict key, whoever calls");
        assert_eq!(manifest["encryption_keys"][0]["bind"], "wasm");
        assert_eq!(manifest["encryption_keys"][0]["caller"], "predecessor", "the enclave key belongs to the calling contract");
        let hosts = manifest["capabilities"]["network"].as_array().unwrap();
        assert_eq!(hosts.len(), 1, "NEAR AI is the only way out");
        assert_eq!(hosts[0]["host"], "cloud-api.near.ai");
    }
}
