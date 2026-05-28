# Security Debt — accepted advisories

cargo-deny gates the build on RUSTSEC advisories. A few transitive
advisories are currently **ignored with justification** in
[`backend/deny.toml`](../backend/deny.toml) because the fix requires a
major upgrade of an intermediate dependency (or, for `rsa`, has no
upstream fix). This file tracks them so the acceptance is visible and
revisited, not silently buried.

Review this list on every `cargo update` and whenever the blocking
dependency cuts a new major version.

| RUSTSEC | Crate | Via | Why accepted | Fix path |
|---------|-------|-----|--------------|----------|
| 2023-0071 | `rsa` | `sqlx-mysql` | Marvin timing sidechannel; **no upstream fix** for the pure-Rust `rsa` crate. Exploit needs a malicious MySQL server + millions of timed oracles; Rampart connects out to operator-controlled DBs. | Upstream `rsa` constant-time work, or drop the MySQL probe's RSA auth path. |
| 2024-0421 | `idna` 0.5 | `validator` 0.18 | Punycode labels decoding to pure ASCII are wrongly accepted. Used only for monitor-name/email validation. | Bump `validator` to ≥0.19 (pulls `idna` 1.x) — API-breaking, needs validation-call review. |
| 2026-0119 | `hickory-proto` 0.24 | `hickory-resolver` (DNS probe) | O(n²) name compression → CPU-amplified DoS from a hostile DNS response. Self-hosted; operator picks the resolver target. | Bump `hickory-resolver` to a release pulling `hickory-proto` ≥0.26.1. |
| 2026-0049 | `rustls-webpki` 0.102 | `rumqttc` → `rustls` 0.22 | Cert name-constraint / CRL-parse panics. MQTT probe verifies an operator-specified broker cert; CRL path not exercised by default config. | Bump `rumqttc` to a release on `rustls` 0.23 (`webpki` 0.103). |
| 2026-0098 | `rustls-webpki` 0.102 | (same) | (same) | (same) |
| 2026-0099 | `rustls-webpki` 0.102 | (same) | (same) | (same) |
| 2026-0104 | `rustls-webpki` 0.102 | (same) | (same) | (same) |

## Planned upgrade pass

A dedicated PR should attempt, in order of risk:

1. `validator` 0.18 → latest (fixes idna; check derive + validation call sites).
2. `hickory-resolver` 0.24 → latest (fixes hickory-proto; re-test the DNS probe).
3. `rumqttc` 0.24 → a `rustls` 0.23 release (fixes all four webpki advisories;
   re-test the MQTT probe's TLS path).

Each bump needs the corresponding probe re-tested against a live target, then
the matching entry removed from the `ignore` list in `deny.toml`.

`rsa` (RUSTSEC-2023-0071) stays until upstream ships a constant-time fix.
