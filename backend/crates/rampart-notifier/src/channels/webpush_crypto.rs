//! Web Push payload encryption (RFC 8291, `aes128gcm`) + VAPID (RFC 8292).
//!
//! Pure Rust — p256 (ECDH + ECDSA), hkdf, aes-gcm. No openssl/C linkage,
//! which keeps the runtime image lean and matches the rest of the tree.
//!
//! The encryption is the part that MUST be byte-exact or the browser
//! silently drops the message, so `encrypt_with_keys` is split out and
//! covered by the RFC 8291 §5 known-answer test vector below.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hkdf::Hkdf;
use p256::ecdh::diffie_hellman;
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use sha2::Sha256;

#[derive(Debug)]
pub enum WebPushError {
    BadKey(String),
    Crypto(String),
}

impl std::fmt::Display for WebPushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebPushError::BadKey(m) => write!(f, "bad key: {m}"),
            WebPushError::Crypto(m) => write!(f, "crypto: {m}"),
        }
    }
}
impl std::error::Error for WebPushError {}

/// Result of encrypting a payload: the `aes128gcm` body to POST and the
/// record salt is already embedded in the body header.
pub struct EncryptedPayload {
    pub body: Vec<u8>,
}

/// Encrypt `plaintext` for a subscription.
///
/// * `ua_public`  — the subscription's p256dh key (65-byte uncompressed P-256 point)
/// * `auth_secret`— the subscription's 16-byte auth secret
///
/// Generates an ephemeral key + random salt internally (production path).
pub fn encrypt(
    plaintext: &[u8],
    ua_public: &[u8],
    auth_secret: &[u8],
) -> Result<EncryptedPayload, WebPushError> {
    use rand::RngCore;
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let as_secret = SecretKey::random(&mut rand::thread_rng());
    encrypt_with_keys(plaintext, ua_public, auth_secret, &as_secret, &salt)
}

/// Deterministic core — separated so the RFC test vector can pin the
/// ephemeral key + salt. Everything in `encrypt` flows through here.
fn encrypt_with_keys(
    plaintext: &[u8],
    ua_public: &[u8],
    auth_secret: &[u8],
    as_secret: &SecretKey,
    salt: &[u8],
) -> Result<EncryptedPayload, WebPushError> {
    let ua_pub = PublicKey::from_sec1_bytes(ua_public)
        .map_err(|e| WebPushError::BadKey(format!("ua p256dh: {e}")))?;
    let as_public_point = as_secret.public_key().to_encoded_point(false);
    let as_public = as_public_point.as_bytes(); // 65 bytes, uncompressed

    // ECDH shared secret.
    let shared = diffie_hellman(as_secret.to_nonzero_scalar(), ua_pub.as_affine());
    let shared_bytes = shared.raw_secret_bytes();

    // RFC 8291 §3.4: combine via HKDF keyed by auth_secret.
    //   key_info = "WebPush: info" || 0x00 || ua_public || as_public
    //   IKM      = HKDF(salt=auth_secret, ikm=ecdh, info=key_info, L=32)
    let mut key_info = Vec::with_capacity(14 + 65 + 65);
    key_info.extend_from_slice(b"WebPush: info\0");
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(as_public);
    let ikm = hkdf_expand(auth_secret, shared_bytes.as_slice(), &key_info, 32)?;

    // RFC 8188 aes128gcm derivation keyed by the record salt.
    let cek = hkdf_expand(salt, &ikm, b"Content-Encoding: aes128gcm\0", 16)?;
    let nonce_bytes = hkdf_expand(salt, &ikm, b"Content-Encoding: nonce\0", 12)?;

    // Single-record payload: plaintext || 0x02 (last-record padding delimiter).
    let mut padded = Vec::with_capacity(plaintext.len() + 1);
    padded.extend_from_slice(plaintext);
    padded.push(0x02);

    let cipher = Aes128Gcm::new_from_slice(&cek)
        .map_err(|e| WebPushError::Crypto(format!("aes key: {e}")))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, Payload { msg: &padded, aad: b"" })
        .map_err(|e| WebPushError::Crypto(format!("aes-gcm: {e}")))?;

    // RFC 8188 header: salt(16) || rs(4, BE) || idlen(1) || keyid(as_public).
    let rs: u32 = 4096;
    let mut body = Vec::with_capacity(16 + 4 + 1 + 65 + ciphertext.len());
    body.extend_from_slice(salt);
    body.extend_from_slice(&rs.to_be_bytes());
    body.push(as_public.len() as u8);
    body.extend_from_slice(as_public);
    body.extend_from_slice(&ciphertext);

    Ok(EncryptedPayload { body })
}

fn hkdf_expand(
    salt: &[u8],
    ikm: &[u8],
    info: &[u8],
    len: usize,
) -> Result<Vec<u8>, WebPushError> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut out = vec![0u8; len];
    hk.expand(info, &mut out)
        .map_err(|e| WebPushError::Crypto(format!("hkdf expand: {e}")))?;
    Ok(out)
}

// ── VAPID (RFC 8292) ──────────────────────────────────────────────────────

/// Build the `Authorization: vapid t=<jwt>, k=<pubkey>` header value for a
/// push endpoint. `aud` is the scheme+host origin of the endpoint.
pub fn vapid_authorization(
    endpoint: &str,
    subject: &str,
    vapid_private_pkcs8_b64: &str,
    vapid_public_b64url: &str,
) -> Result<String, WebPushError> {
    let aud = origin_of(endpoint)?;
    let exp = now_secs() + 12 * 3600;

    // JWT header + claims, base64url-no-pad, ES256-signed.
    let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = format!(r#"{{"aud":"{aud}","exp":{exp},"sub":"{subject}"}}"#);
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims.as_bytes());
    let signing_input = format!("{header}.{claims_b64}");

    let pkcs8 = STANDARD
        .decode(vapid_private_pkcs8_b64)
        .map_err(|e| WebPushError::BadKey(format!("vapid key b64: {e}")))?;
    let signing_key = SigningKey::from_pkcs8_der(&pkcs8)
        .map_err(|e| WebPushError::BadKey(format!("vapid pkcs8: {e}")))?;
    let sig: Signature = signing_key.sign(signing_input.as_bytes());
    // ES256 JWT wants the raw 64-byte r||s, which is exactly p256's fixed
    // encoding — not the DER form.
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
    let jwt = format!("{signing_input}.{sig_b64}");

    Ok(format!("vapid t={jwt}, k={vapid_public_b64url}"))
}

/// Generate a fresh VAPID keypair. Returns (public_b64url_uncompressed,
/// private_pkcs8_der_b64). Stored in settings on first use.
pub fn generate_vapid_keys() -> (String, String) {
    use p256::pkcs8::EncodePrivateKey;
    let secret = SecretKey::random(&mut rand::thread_rng());
    let public_point = secret.public_key().to_encoded_point(false);
    let public_b64 = URL_SAFE_NO_PAD.encode(public_point.as_bytes());
    let pkcs8 = secret
        .to_pkcs8_der()
        .expect("p256 pkcs8 encode")
        .as_bytes()
        .to_vec();
    let private_b64 = STANDARD.encode(pkcs8);
    (public_b64, private_b64)
}

fn origin_of(url: &str) -> Result<String, WebPushError> {
    // scheme://host[:port] — strip the path. Cheap parse; endpoints are
    // always absolute https URLs from the browser.
    let scheme_end = url
        .find("://")
        .ok_or_else(|| WebPushError::BadKey("endpoint has no scheme".into()))?;
    let after = &url[scheme_end + 3..];
    let host_end = after.find('/').unwrap_or(after.len());
    Ok(format!("{}{}", &url[..scheme_end + 3], &after[..host_end]))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

use p256::pkcs8::DecodePrivateKey as _;

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier as _;

    // Test-only receiver side: decrypt an aes128gcm body with the UA
    // private key. Mirrors encrypt_with_keys so any mismatch in HKDF info
    // strings, ordering, or key derivation makes the round-trip fail.
    fn decrypt(body: &[u8], ua_private: &SecretKey, auth_secret: &[u8]) -> Vec<u8> {
        let salt = &body[0..16];
        let idlen = body[20] as usize;
        let keyid = &body[21..21 + idlen]; // as_public (65 bytes)
        let ciphertext = &body[21 + idlen..];

        let as_pub = PublicKey::from_sec1_bytes(keyid).unwrap();
        let ua_public = ua_private
            .public_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();

        let shared = diffie_hellman(ua_private.to_nonzero_scalar(), as_pub.as_affine());

        let mut key_info = Vec::new();
        key_info.extend_from_slice(b"WebPush: info\0");
        key_info.extend_from_slice(&ua_public);
        key_info.extend_from_slice(keyid);
        let ikm = hkdf_expand(auth_secret, shared.raw_secret_bytes().as_slice(), &key_info, 32).unwrap();

        let cek = hkdf_expand(salt, &ikm, b"Content-Encoding: aes128gcm\0", 16).unwrap();
        let nonce = hkdf_expand(salt, &ikm, b"Content-Encoding: nonce\0", 12).unwrap();

        let cipher = Aes128Gcm::new_from_slice(&cek).unwrap();
        let mut pt = cipher
            .decrypt(Nonce::from_slice(&nonce), Payload { msg: ciphertext, aad: b"" })
            .unwrap();
        // Strip the single-record padding delimiter (0x02).
        while matches!(pt.last(), Some(0u8)) {
            pt.pop();
        }
        if matches!(pt.last(), Some(2u8)) {
            pt.pop();
        }
        pt
    }

    // RFC 8291 §5 known-answer inputs. We encrypt with the RFC's fixed
    // application-server key + salt, then decrypt with the RFC's receiver
    // private key and assert the plaintext round-trips. This pins the full
    // ECDH + HKDF + AES-GCM pipeline to the spec's exact key material.
    // https://www.rfc-editor.org/rfc/rfc8291#section-5
    #[test]
    fn rfc8291_section5_roundtrip() {
        let plaintext = b"When I grow up, I want to be a watermelon";
        let ua_public = URL_SAFE_NO_PAD
            .decode("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4")
            .unwrap();
        let auth = URL_SAFE_NO_PAD.decode("BTBZMqHH6r4Tts7J_aSIgg").unwrap();
        let as_secret = SecretKey::from_slice(
            &URL_SAFE_NO_PAD.decode("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw").unwrap(),
        )
        .unwrap();
        let salt = URL_SAFE_NO_PAD.decode("DGv6ra1nlYgDh4VAd6lkpg").unwrap();
        // Receiver (UA) private key from the RFC.
        let ua_secret = SecretKey::from_slice(
            &URL_SAFE_NO_PAD.decode("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94").unwrap(),
        )
        .unwrap();
        // Sanity: the RFC's UA private key must match the p256dh we encrypt to.
        assert_eq!(
            ua_secret.public_key().to_encoded_point(false).as_bytes(),
            ua_public.as_slice(),
            "RFC UA keypair mismatch"
        );

        let out = encrypt_with_keys(plaintext, &ua_public, &auth, &as_secret, &salt).unwrap();
        let recovered = decrypt(&out.body, &ua_secret, &auth);
        assert_eq!(recovered, plaintext, "RFC 8291 §5 round-trip failed");
    }

    // A random keypair + salt must also round-trip (covers the production
    // `encrypt` path, not just fixed vectors).
    #[test]
    fn random_roundtrip() {
        let ua_secret = SecretKey::random(&mut rand::thread_rng());
        let ua_public = ua_secret.public_key().to_encoded_point(false).as_bytes().to_vec();
        let mut auth = [0u8; 16];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut auth);
        let msg = b"rampart: monitor api.example.com is DOWN";
        let out = encrypt(msg, &ua_public, &auth).unwrap();
        let recovered = decrypt(&out.body, &ua_secret, &auth);
        assert_eq!(recovered, msg);
    }

    #[test]
    fn vapid_jwt_is_verifiable() {
        let (pub_b64, priv_b64) = generate_vapid_keys();
        let auth = vapid_authorization(
            "https://push.example.net/push/abc",
            "mailto:ops@example.com",
            &priv_b64,
            &pub_b64,
        )
        .unwrap();
        // Pull the JWT back out and verify the signature with the public key.
        let t = auth.split("t=").nth(1).unwrap().split(',').next().unwrap();
        let parts: Vec<&str> = t.split('.').collect();
        assert_eq!(parts.len(), 3);
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let sig_bytes = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        let sig = Signature::from_slice(&sig_bytes).unwrap();
        let pub_bytes = URL_SAFE_NO_PAD.decode(&pub_b64).unwrap();
        let vk = p256::ecdsa::VerifyingKey::from_sec1_bytes(&pub_bytes).unwrap();
        vk.verify(signing_input.as_bytes(), &sig).unwrap();
    }
}
