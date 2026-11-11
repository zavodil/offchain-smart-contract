//! Private text: sealed by the client to this build's enclave key, opened
//! only here.
//!
//! The key pair is derived, never stored: its secret is the `mac` of a fixed
//! label under the `inbox` encryption key, which the keystore derives inside
//! the TEE for this exact build (`bind: "wasm"`) and the contract that called
//! OutLayer (`caller: "predecessor"`) — the gate. Another build, or the same
//! build called by another contract, derives another key and opens nothing
//! sealed to this one.
//!
//! The sealed plaintext names the contract it is for, and the judge refuses
//! it under any other: a ciphertext copied out of one contract's request into
//! another's does not open.

use outlayer::encryption_keys;
use serde::Deserialize;

const KEY_PATH: &str = "inbox";
const KEY_LABEL: &[u8] = b"policy-judge/ecies-secp256k1/v1";

/// What a client seals: the text, and the contract it is for.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sealed {
    #[serde(rename = "for")]
    pub for_account: String,
    pub text: String,
}

fn enclave_secret() -> Result<libsecp256k1::SecretKey, String> {
    let seed = encryption_keys::mac(KEY_PATH, None, KEY_LABEL)
        .map_err(|e| format!("the enclave key is not available to this run: {e}"))?;
    let seed: [u8; 32] = seed.try_into().map_err(|_| "the enclave key seed is not 32 bytes".to_string())?;
    libsecp256k1::SecretKey::parse(&seed).map_err(|_| "the enclave key seed is not a valid secp256k1 scalar".to_string())
}

/// The enclave's public key, compressed, in hex: what clients seal to.
pub fn enclave_public_key() -> Result<String, String> {
    let secret = enclave_secret()?;
    Ok(hex::encode(libsecp256k1::PublicKey::from_secret_key(&secret).serialize_compressed()))
}

/// Open what a client sealed for `for_account`.
pub fn open(sealed: &[u8], for_account: &str) -> Result<String, String> {
    let secret = enclave_secret()?;
    let plaintext = ecies::decrypt(&secret.serialize(), sealed).map_err(|_| {
        "the text does not open with this enclave's key: it was sealed to another build or another gate, or changed on the way"
            .to_string()
    })?;
    read(&plaintext, for_account)
}

fn read(plaintext: &[u8], for_account: &str) -> Result<String, String> {
    let sealed: Sealed =
        serde_json::from_slice(plaintext).map_err(|e| format!("the sealed text is `{{\"for\", \"text\"}}`: {e}"))?;
    if sealed.for_account != for_account {
        return Err(format!(
            "the text was sealed for {}, not for {for_account}, the contract that asked",
            sealed.for_account
        ));
    }
    Ok(sealed.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_text_opens_only_for_the_contract_it_names() {
        let (secret, public) = ecies::utils::generate_keypair();
        let blob = ecies::encrypt(&public.serialize_compressed(), br#"{"for":"treasury.testnet","text":"Pay the venue"}"#).unwrap();
        let plaintext = ecies::decrypt(&secret.serialize(), &blob).unwrap();
        assert_eq!(read(&plaintext, "treasury.testnet").unwrap(), "Pay the venue");
        assert!(read(&plaintext, "other.testnet").unwrap_err().contains("sealed for treasury.testnet"));
        assert!(read(br#"{"text":"no owner"}"#, "treasury.testnet").is_err());
        assert!(read(br#"{"for":"t","text":"x","extra":1}"#, "t").is_err());
    }
}
