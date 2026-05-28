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

Both remaining dependency advisories were investigated and are currently
**blocked upstream** — attempted and reverted, do not re-try without the
upstream change landing first:

1. **`hickory-proto` 2026-0119** — fixed in hickory-proto ≥0.26.1, which
   needs `hickory-resolver` 0.26. But the vulnerable `hickory-proto 0.24.4`
   is *also* pulled independently by `mongodb 3.2.0` (its own bundled
   `hickory-resolver 0.24`). `mongodb` is pinned at `=3.2.0` because later
   3.x patches need a `jni` version not yet on crates.io. So bumping our
   resolver alone leaves mongodb's vulnerable proto in the tree — the
   advisory only clears once mongodb can be unpinned. **Blocked on the
   mongodb/jni upstream fix.**

2. **`rustls-webpki` 2026-0049/0098/0099/0104** — `rumqttc` 0.25.1 (latest)
   *still* depends on `rustls-webpki 0.102.8` (does not clear the advisory)
   and additionally drags in `aws-lc-rs` + `cmake` (a C crypto provider,
   against the pure-Rust / lean-image stance). **Blocked until rumqttc
   moves off webpki 0.102.**

When either upstream lands, do the bump, re-test the probe against a live
target, and remove the matching entries from the `ignore` list in
`deny.toml`.

`rsa` (RUSTSEC-2023-0071) stays until upstream ships a constant-time fix.
