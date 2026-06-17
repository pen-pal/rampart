//! Envelope encryption for secret-bearing JSON columns.
//!
//! Notification channel `config` holds live credentials — webhook bearer
//! tokens, SMTP passwords, the API keys of 128 channels — as JSONB. At rest
//! that is plaintext, so a database read (backup leak, replica, SQL injection
//! elsewhere) exposes every outbound credential. This module encrypts those
//! blobs with AES-256-GCM.
//!
//! **Opt-in + backward compatible.** Encryption activates only when
//! `RAMPART_SECRET_KEY` is set (a 32-byte key as 64 hex chars or base64):
//!
//!   * [`seal`] wraps a value as `{"__enc1": "<base64 nonce‖ciphertext>"}`.
//!     With no key it returns the value unchanged (plaintext, as before).
//!   * [`open`] reverses a sealed value; anything not sealed passes through.
//!
//! So a key-less install keeps storing plaintext; setting a key encrypts on
//! the next write of each row (lazy migration) while still reading old
//! plaintext rows. Losing the key after encrypting means the blob can't be
//! decrypted — back it up like any other secret.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use once_cell::sync::OnceCell;
use serde_json::{json, Value};

const ENVELOPE_KEY: &str = "__enc1";

/// Process-wide cipher derived once from `RAMPART_SECRET_KEY`. `None` when the
/// env var is unset or malformed (→ encryption disabled, plaintext mode).
fn cipher() -> Option<&'static Aes256Gcm> {
    static CIPHER: OnceCell<Option<Aes256Gcm>> = OnceCell::new();
    CIPHER
        .get_or_init(|| {
            let raw = std::env::var("RAMPART_SECRET_KEY").ok()?;
            let bytes = decode_key(raw.trim())?;
            Some(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&bytes)))
        })
        .as_ref()
}

/// Accept a 32-byte key as 64 hex chars or as base64.
fn decode_key(s: &str) -> Option<[u8; 32]> {
    if s.len() == 64 {
        if let Ok(v) = hex::decode(s) {
            return v.try_into().ok();
        }
    }
    B64.decode(s).ok().and_then(|v| v.try_into().ok())
}

/// Whether secrets-at-rest encryption is active (a valid `RAMPART_SECRET_KEY`
/// is configured). When `false`, secret-bearing JSONB columns are stored as
/// plaintext — the startup path warns loudly and `/healthz` surfaces it.
pub fn is_enabled() -> bool {
    cipher().is_some()
}

/// Encrypt `v` if a key is configured; otherwise return it unchanged.
pub fn seal(v: &Value) -> Value {
    seal_with(cipher(), v)
}

/// Decrypt `v` if it is a sealed envelope and a key is configured; otherwise
/// return it unchanged.
pub fn open(v: Value) -> Value {
    open_with(cipher(), v)
}

fn seal_with(cipher: Option<&Aes256Gcm>, v: &Value) -> Value {
    let Some(cipher) = cipher else {
        return v.clone();
    };
    let plaintext = match serde_json::to_vec(v) {
        Ok(b) => b,
        Err(_) => return v.clone(),
    };
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    match cipher.encrypt(&nonce, plaintext.as_ref()) {
        Ok(ct) => {
            let mut blob = nonce.to_vec();
            blob.extend_from_slice(&ct);
            json!({ ENVELOPE_KEY: B64.encode(blob) })
        }
        Err(_) => v.clone(),
    }
}

fn open_with(cipher: Option<&Aes256Gcm>, v: Value) -> Value {
    // Sealed = an object whose ONLY key is the envelope marker.
    let sealed = v
        .as_object()
        .filter(|o| o.len() == 1)
        .and_then(|o| o.get(ENVELOPE_KEY))
        .and_then(|x| x.as_str());
    let Some(b64) = sealed else {
        return v; // not sealed → plaintext passthrough
    };
    let Some(cipher) = cipher else {
        return v; // sealed but no key → can't open; return as-is
    };
    let blob = match B64.decode(b64) {
        Ok(b) => b,
        Err(_) => return v,
    };
    if blob.len() < 12 {
        return v;
    }
    let (nonce, ct) = blob.split_at(12);
    match cipher.decrypt(Nonce::from_slice(nonce), ct) {
        Ok(pt) => serde_json::from_slice(&pt).unwrap_or(v),
        Err(_) => v,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cipher() -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&[7u8; 32]))
    }

    #[test]
    fn roundtrip_with_key() {
        let c = test_cipher();
        let secret = json!({"url": "https://hooks.example.com/x", "token": "s3cr3t"});
        let sealed = seal_with(Some(&c), &secret);
        // Sealed form hides the plaintext + is the envelope shape.
        assert!(sealed.get(ENVELOPE_KEY).is_some());
        assert!(!sealed.to_string().contains("s3cr3t"));
        // Opens back to the original.
        assert_eq!(open_with(Some(&c), sealed), secret);
    }

    #[test]
    fn passthrough_without_key() {
        let secret = json!({"token": "abc"});
        assert_eq!(seal_with(None, &secret), secret);
        assert_eq!(open_with(None, secret.clone()), secret);
    }

    #[test]
    fn plaintext_config_opens_unchanged() {
        // A normal (unsealed) config read with a key set must pass through.
        let c = test_cipher();
        let plain = json!({"url": "https://x", "token": "t"});
        assert_eq!(open_with(Some(&c), plain.clone()), plain);
    }

    #[test]
    fn wrong_key_returns_blob_not_garbage() {
        let sealed = seal_with(Some(&test_cipher()), &json!({"a": 1}));
        let other = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&[9u8; 32]));
        // Can't decrypt with the wrong key → returns the sealed value as-is
        // (never panics, never emits partial plaintext).
        assert_eq!(open_with(Some(&other), sealed.clone()), sealed);
    }

    #[test]
    fn decode_key_accepts_hex_and_base64() {
        assert!(decode_key(&"a".repeat(64)).is_some()); // 64 hex
        assert!(decode_key(&B64.encode([1u8; 32])).is_some()); // base64
        assert!(decode_key("too-short").is_none());
    }
}
