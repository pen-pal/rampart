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
| 2026-0119 | `hickory-proto` 0.24 | `hickory-resolver` (DNS probe) | O(n²) name compression → CPU-amplified DoS from a hostile DNS response. Self-hosted; operator picks the resolver target. | Bump `hickory-resolver` to ≥0.25 — drops the `tokio-runtime` feature and changes the resolver API, so it needs a `dns.rs` rewrite + a check against the `mongodb = "=3.2.0"` pin. |
| 2026-0049 | `rustls-webpki` 0.102 | `rumqttc` → `rustls` 0.22 | Cert name-constraint / CRL-parse panics. MQTT probe verifies an operator-specified broker cert; CRL path not exercised by default config. | Bump `rumqttc` to a release on `rustls` 0.23 (`webpki` 0.103). |
| 2026-0098 | `rustls-webpki` 0.102 | (same) | (same) | (same) |
| 2026-0099 | `rustls-webpki` 0.102 | (same) | (same) | (same) |
| 2026-0104 | `rustls-webpki` 0.102 | (same) | (same) | (same) |

## Progress

- ✅ **`validator` 0.18 → 0.20** — cleared RUSTSEC-2024-0421 (idna 0.5 dropped).
  Compiled clean with no call-site changes; removed from the ignore list.

## Planned upgrade pass

Remaining, in order of risk:

1. `hickory-resolver` 0.24 → ≥0.25 (clears 2026-0119). Drops the
   `tokio-runtime` feature and reshapes the resolver API → rewrite `dns.rs`,
   verify the `mongodb = "=3.2.0"` pin still resolves, re-test the DNS probe.
2. `rumqttc` 0.24 → a `rustls` 0.23 release (clears the four webpki
   advisories; re-test the MQTT probe's TLS path). The workspace already
   pins `rustls = 0.23` for the other probes — only rumqttc drags in 0.22.

Each bump needs the corresponding probe re-tested against a live target, then
the matching entry removed from the `ignore` list in `deny.toml`.

`rsa` (RUSTSEC-2023-0071) stays until upstream ships a constant-time fix.
