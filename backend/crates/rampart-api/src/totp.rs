//! TOTP helpers — generate secrets, build otpauth URIs for QR enrolment,
//! and verify 6-digit codes with a ±1 step (30s) tolerance.
//!
//! Default algorithm is SHA-1 / 6 digits / 30 s — what Google
//! Authenticator, 1Password, Authy, and Bitwarden all assume when an
//! otpauth URI doesn't specify otherwise.

use base32::Alphabet;
use rand::RngCore;
use totp_rs::{Algorithm, TOTP};

const ISSUER: &str = "Rampart";

/// 160-bit random secret encoded as base32 (no padding) — the canonical
/// shape for otpauth URIs.
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut bytes);
    base32::encode(Alphabet::Rfc4648 { padding: false }, &bytes)
}

/// Build the otpauth URI the user pastes into / scans from their
/// authenticator app. Embedding the issuer means apps show "Rampart:
/// alice@example.com" instead of just the email.
pub fn provisioning_uri(secret_b32: &str, account: &str) -> Result<String, String> {
    let totp = build_totp(secret_b32, account)?;
    Ok(totp.get_url())
}

/// Verify a 6-digit code against the secret. Accepts the current step
/// or either neighbour to absorb clock skew.
pub fn verify(secret_b32: &str, code: &str) -> bool {
    let code = code.trim();
    let Ok(totp) = build_totp(secret_b32, "_") else {
        return false;
    };
    matches!(totp.check_current(code), Ok(true))
}

fn build_totp(secret_b32: &str, account: &str) -> Result<TOTP, String> {
    let raw = base32::decode(Alphabet::Rfc4648 { padding: false }, secret_b32)
        .ok_or_else(|| "invalid base32 secret".to_string())?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1, // window: 1 step either side of current
        30,
        raw,
        Some(ISSUER.to_string()),
        account.to_string(),
    )
    .map_err(|e| format!("totp construct: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_is_base32_decodable() {
        let s = generate_secret();
        // Base32 secret length for 20 bytes (no padding) is 32 chars.
        assert_eq!(s.len(), 32);
        assert!(base32::decode(Alphabet::Rfc4648 { padding: false }, &s).is_some());
    }

    #[test]
    fn provisioning_uri_includes_issuer_and_account() {
        let s = generate_secret();
        let u = provisioning_uri(&s, "alice@example.com").unwrap();
        assert!(u.starts_with("otpauth://totp/"), "uri = {u}");
        assert!(u.contains("Rampart"), "uri = {u}");
    }

    #[test]
    fn verify_rejects_garbage() {
        let s = generate_secret();
        assert!(!verify(&s, "000000"));
        assert!(!verify(&s, "not-a-number"));
    }
}
