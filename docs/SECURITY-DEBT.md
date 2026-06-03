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
| 2026-0049 | `rustls-webpki` 0.102 | `rumqttc` → `rustls` 0.22 | Cert name-constraint / CRL-parse panics. MQTT probe verifies an operator-specified broker cert; CRL path not exercised by default config. | Bump `rumqttc` to a release on `rustls` 0.23 (`webpki` 0.103). |
| 2026-0098 | `rustls-webpki` 0.102 | (same) | (same) | (same) |
| 2026-0099 | `rustls-webpki` 0.102 | (same) | (same) | (same) |
| 2026-0104 | `rustls-webpki` 0.102 | (same) | (same) | (same) |
| GHSA-pwjx-qhcg-rvj4 | `rustls-webpki` 0.102 | (same) | CRLs not considered authoritative by `distributionPoint` due to faulty matching logic. Same chain — only affects callers that explicitly opt-in to CRL checking via `RevocationOptions`, which our MQTT probe does not. | (same) |
| GHSA-82j2-j2ch-gfr8 | `rustls-webpki` 0.102 | (same) | DoS via panic on malformed CRL `BIT STRING`. Same chain — also CRL-only; reachable only when CRL revocation is explicitly enabled, which our default config doesn't do. | (same) |

## Progress

- ✅ **`validator` 0.18 → 0.20** — cleared RUSTSEC-2024-0421 (idna 0.5 dropped).
  Compiled clean with no call-site changes; removed from the ignore list.
- ✅ **`mongodb` 3.2.0 → 3.7.0 + `hickory-resolver` 0.24 → 0.26** — cleared
  RUSTSEC-2026-0119 (`hickory-proto` O(n²) name compression). `mongodb`
  unpinned (the jni-version issue resolved upstream); now pulls hickory
  0.26 itself, so bumping our DNS-probe resolver to match left a single
  hickory-proto in the tree. `dns.rs` rewrite for the new `TokioResolver`
  / `ResolverBuilder` / `NameServerConfig` API.

## Planned upgrade pass

One dependency advisory remains **blocked upstream** — attempted and
reverted, do not re-try without the upstream change landing first:

* **`rustls-webpki` 2026-0049/0098/0099/0104** — `rumqttc` 0.25.1 (latest)
  *still* depends on `rustls-webpki 0.102.8` (does not clear the advisory)
  and additionally drags in `aws-lc-rs` + `cmake` (a C crypto provider,
  against the pure-Rust / lean-image stance). **Blocked until rumqttc
  moves off webpki 0.102.**

When this lands, do the bump, re-test the MQTT probe against a live
broker, and remove the matching entries from the `ignore` list in
`deny.toml`.

`rsa` (RUSTSEC-2023-0071) stays until upstream ships a constant-time fix.
