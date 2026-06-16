# Security Debt — accepted advisories

cargo-deny gates the build on RUSTSEC advisories. A few transitive
advisories are currently **ignored with justification** in
[`backend/deny.toml`](../backend/deny.toml) because the fix requires a
major upgrade of an intermediate dependency. This file tracks them so
the acceptance is visible and revisited, not silently buried.

Review this list on every `cargo update` and whenever the blocking
dependency cuts a new major version.

| RUSTSEC | Crate | Via | Why accepted | Fix path |
|---------|-------|-----|--------------|----------|
| 2026-0049 | `rustls-webpki` 0.102 / 0.101 | `rumqttc` → `rustls` 0.22, `async-nats` 0.36 (0.102); `tiberius` → `rustls` 0.21 (0.101) | Cert name-constraint / CRL-parse panics. Only reachable on a TLS handshake to an operator-specified MQTT/NATS/MSSQL target presenting a malicious cert; the CRL path is not exercised by default config. | Needs `rustls` 0.23 (`webpki` 0.103) across these. **Blocked:** `rumqttc` 0.25 (the only rustls-0.23 release) pulls `aws-lc-rs`/cmake, breaking the pure-Rust / C-toolchain-free build (see `DEPENDENCIES.md`); `async-nats`/`tiberius` have no semver-compatible fixed release yet. |
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
- ✅ **`sqlx` 0.8 → 0.9** — cleared RUSTSEC-2023-0071 (`rsa` Marvin timing
  sidechannel). sqlx 0.9 reworked the MySQL auth path so the pure-Rust
  `rsa` crate is no longer a transitive dependency — `cargo tree -i rsa`
  now returns "did not match any packages". Removed from the ignore list.

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

## Pending TLS gaps

Probes whose plaintext path works end-to-end but whose TLS path is
deferred because enabling it would drag a non-pure-Rust crypto provider
into the build graph. Each row records the **attempt date**, what
broke, and the **fix that has to land upstream** before we can try
again. The probe code rejects a `tls: true` request with a clear error
pointing back here, so an operator who tries to flip the toggle gets a
heartbeat that explains the gap rather than a probe that silently runs
in plaintext.

| Probe | Upstream crate | What was tried | Why it failed | Unblocked when |
|-------|----------------|----------------|---------------|-----------------|
| `cassandra` (`scylla`) | [`scylla`](https://crates.io/crates/scylla) | Enabled `scylla = { default-features = false, features = ["rustls-023"] }` together with a `monitor.config.tls = true` field. | `scylla 1.6.0` declares `rustls = "0.23"` and `tokio-rustls = "0.26"` as plain optional deps **without** `default-features = false`; Cargo's feature unification then activates `aws_lc_rs` (a default feature of rustls 0.23) on the workspace's shared `rustls v0.23`, dragging `aws-lc-rs` + `cmake` (build-dep) into the graph. Reverted before commit — `cargo tree -i aws-lc-rs` still has to return "did not match any packages". Re-attempted 2026-06-09 against `scylla 1.6.0` (latest as of this date) **and** the upstream `main` branch: feature set unchanged, no `ring` opt-in exists. `SessionBuilder::tls_context()` exists (takes `impl Into<TlsContext>`), but the `From<Arc<rustls::ClientConfig>>` impl is itself feature-gated behind `rustls-023`, so the "thread our own ClientConfig" sidestep also requires enabling the offending feature. | Either (a) `scylla` adds `default-features = false` to its `rustls` + `tokio-rustls` deps so the workspace's `ring`-only choice wins, or (b) `scylla` introduces a `rustls-ring` / `rustls-aws-lc-rs` feature split so callers can pick the provider explicitly. |

When either of those lands, follow the same workflow as the
"Planned upgrade pass" section: enable the feature, run the probe
against a live cluster, drop the rejection branch in
`cassandra.rs`, and remove this row.

## Re-verification — 2026-06-09 (post-v0.4.0)

Re-checked the standing upstream blocks; all unchanged, no action:

- **Cassandra-TLS** (`scylla`): still no `ring`-only feature on the
  pinned `1.x`; enabling rustls drags `aws-lc-rs`. Blocked.
- **`rumqttc` 0.25**: still pulls `aws-lc-rs`. Pinned at 0.24
  (`use-rustls`). Blocked.
- **`rustls-webpki` advisories** (0.101.7 via `tiberius` 0.12 → rustls
  0.21; 0.102.8 via `async-nats` 0.36): deep transitive, CRL paths
  unreached in our config; ignored in `deny.toml`. No upstream bump
  that drops the old webpki yet.

Confirmed `cargo tree -i aws-lc-rs` / `-i cmake` / `-i openssl` still
return "did not match any packages".

## Re-verification — 2026-06-16 (post-v0.101.0)

Re-checked latest versions of every blocking crate. The conclusion is
unchanged — the `rustls-webpki` advisories stay ignored — but one
rationale line is now stale and is corrected here (and in `deny.toml`):

- **`async-nats`**: now **has** a fixed-release path. `async-nats 0.49.1`
  (latest; we pin 0.36.0) exposes an explicit `ring` feature
  (`ring = [dep:ring, tokio-rustls/ring]`) alongside `aws-lc-rs`, so it
  *can* move to rustls 0.23 / webpki 0.103 **without** a C crypto
  provider. The earlier note ("async-nats has no semver-compatible fixed
  release") is therefore wrong as of today.
- **…but bumping `async-nats` alone clears nothing.** The 0.102 advisory
  is also dragged in by **`rumqttc` 0.24 → rustls 0.22**, and `rumqttc`
  0.25.1 (latest) still only offers `use-rustls` with **no `ring` opt-in**
  → it forces `aws-lc-rs`. The 0.101 advisory is dragged in by
  **`tiberius` 0.12.3**, which is still the latest release (no fix exists
  upstream at all). So `rustls-webpki` 0.101/0.102 remain in the tree
  until *both* `rumqttc` gains a `ring` path **and** `tiberius` cuts a
  rustls-0.23 release — or until we drop those probes.
- **`scylla` / Cassandra-TLS**: unchanged — still no `ring`-only feature.
- Confirmed `cargo tree -i aws-lc-rs` / `-i aws-lc-sys` / `-i cmake` /
  `-i openssl` / `-i openssl-sys` all still return "did not match any
  packages" — the pure-Rust, C-toolchain-free build invariant holds.

**Decision:** keep the documented-debt stance. The only ways to fully
clear the alerts today are to accept `aws-lc-rs`+cmake (via `rumqttc`
0.25) or drop the MSSQL/NATS/MQTT probes — both trade away a deliberate
build constraint. The advisories are outbound-probe-TLS only and the CRL
paths are unreached by default config, so the residual risk is low.
Revisit when `rumqttc` ships a `ring` path and `tiberius` cuts a
rustls-0.23 release.
