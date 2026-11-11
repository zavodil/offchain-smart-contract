//! NEAR AI Cloud, as the judge uses it: typed decisions (`/v1/systemone`).
//!
//! `Accept-Encoding: identity` keeps the answer byte for byte as NEAR AI sent
//! it; `x-no-aliasing: true` asks for the exact model the pack names.

use crate::http::{self, Reply};
use serde_json::Value;

const BASE: &str = "https://cloud-api.near.ai";

pub struct Client {
    authorization: String,
}

impl Client {
    /// The gate's NEAR AI key: a secret row the gate names in every call,
    /// whose access condition admits only this build, called only through
    /// the gate.
    pub fn from_env() -> Result<Self, String> {
        let key = outlayer::env::var("NEARAI_API_KEY").filter(|k| !k.trim().is_empty()).ok_or(
            "NEARAI_API_KEY is not set: the call must name the gate's secret row, which admits this build only",
        )?;
        Ok(Self { authorization: format!("Bearer {}", key.trim()) })
    }

    /// `POST /v1/systemone`: typed decisions. The answer, byte for byte.
    pub fn decide(&self, body: &[u8]) -> Result<Vec<u8>, String> {
        let reply = http::post(
            &format!("{BASE}/v1/systemone"),
            &[
                ("Authorization", &self.authorization),
                ("Content-Type", "application/json"),
                ("Accept", "application/json"),
                ("Accept-Encoding", "identity"),
                ("x-no-aliasing", "true"),
            ],
            body,
        )?;
        if !reply.ok() {
            return Err(format!("NEAR AI refused the decision: HTTP {} {}", reply.status, error_message(&reply)));
        }
        Ok(reply.body)
    }
}

/// NEAR AI's error text: `error.message` when the body has one.
fn error_message(reply: &Reply) -> String {
    serde_json::from_slice::<Value>(&reply.body)
        .ok()
        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| reply.excerpt())
}
