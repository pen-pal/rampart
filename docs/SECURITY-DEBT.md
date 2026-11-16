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
| 2026-0049, 0098, 0099, 0104, 0382 | `rustls-webpki` 0.101 / 0.102 | `tiberius` 0.12 → `rustls` 0.21 (0.101, MSSQL probe); `rumqttc` 0.25 (0.102, MQTT probe) | CRL / name-constraint / distrusted-intermediate issues in certificate verification. **0.102 is unreachable:** rumqttc only uses it for an error-enum variant; its handshake verifies through `rustls` 0.23 + `webpki` 0.103 (patched). **0.101 is reachable** only on a TLS connection to an operator-specified MSSQL target with a malicious cert, and the MSSQL probe defaults to `trust_cert: true`, which skips chain verification entirely. | **Blocked on `tiberius`:** 0.12.3 has no rustls-0.23 release; 0.13 dropped rustls for `native-tls` (OpenSSL on Linux), which breaks the pure-Rust build. The 0.102 entries go away when rumqttc drops its direct webpki 0.102 dep. |
| 2026-0346 | `rustls` 0.21 | `tiberius` (MSSQL probe) | `Debug` impls of client session values include TLS secrets. Only exposed by a custom `ClientSessionStore` that formats them; there is none in the tree. | Same as above (tiberius). |
| 2026-0349 | `rustls` 0.21 | `tiberius` (MSSQL probe) | Unauthenticated ServerHello can downgrade later key exchange to classical groups. rustls 0.21 has no post-quantum hybrid groups, so there is nothing to downgrade from. | Same as above (tiberius). |
| 2026-0387 | `crypto-bigint` 0.5 | `elliptic-curve` 0.13 via `p256` (Web Push VAPID ES256 **signing**) and `jsonwebtoken` `rust_crypto` (`p256`/`p384`, OIDC ES* verification) | Some operations meant to be constant-time may branch on secrets depending on compiler/CPU. The secret-key use is VAPID signing of outbound Web Push requests; an attacker can't trigger signings on demand or time them in-process. | `p256` 0.14 (+ `jsonwebtoken` 12). **Blocked:** `p256` 0.14 is only on the RustCrypto release-candidate channel and has to move with the rest of that bundle (see `DEPENDENCIES.md`). When stable: re-run the RFC 8291 known-answer test (Web Push) and an OIDC login. |
| 2023-0071 | `rsa` 0.9 | `jsonwebtoken` `rust_crypto` | Marvin timing side channel on private-key operations. OIDC uses RSA only to **verify** id_tokens (public key). The GCP Pub/Sub channel **signs** its service-account JWT with RS256, but only when minting an OAuth token (cached for its lifetime), outbound, with no attacker-triggered signing. | `jsonwebtoken` 12 if it moves to `rsa` 0.10 (fixed); verify before bumping. |
| 2026-0459 | `libsqlite3-sys` 0.37 (bundled SQLite < 3.54) | `sqlx-sqlite` 0.9.0, which caps it at `< 0.38` | Uninitialized page-cache read when opening a database together with a crafted `-wal` file of a different page size. Requires an attacker to place both files where Rampart opens them; the SQLite path is the operator's `DATABASE_URL`. Note: compiled into every build, because the workspace `sqlx` enables the `sqlite` feature. | A `sqlx` release that allows `libsqlite3-sys` ≥ 0.38. |

## Progress

- ✅ **2026-11-16 refresh: 41 → 0 unaccepted advisories.** `cargo deny` had
  failed on `main` since 2026-07-07. A semver-compatible `cargo update`
  cleared 32 (anyhow, crossbeam-epoch, ctutils, h2, hickory-net/-proto/
  -resolver, rustls 0.23, rustls-webpki 0.103, tokio, webpki-roots,
  webpki-root-certs). Bumping **`rumqttc` 0.24 → 0.25** (feature
  `use-rustls-no-provider`, no aws-lc-rs) and **`async-nats` 0.36 → 0.52**
  (`ring`) removed `rustls` 0.22 entirely and moved both probes' TLS onto
  rustls 0.23. Both probes re-tested live: local Mosquitto + NATS, and TLS with
  real certificate verification against test.mosquitto.org:8886 and
  demo.nats.io:4443. The remaining 7 are in the table above.
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

* **`tiberius`** — the last holder of `rustls` 0.21 / `webpki` 0.101. Watch for
  a release with a rustls-0.23 option and a `ring` (or no-provider) feature;
  0.13's `native-tls` is not acceptable.
* **`p256` 0.14 + `jsonwebtoken` 12** — would clear `crypto-bigint` and likely
  `rsa`. Waits on the coordinated RustCrypto stable release (`DEPENDENCIES.md`).
  Re-run the Web Push known-answer test and an OIDC login after bumping.
* **`sqlx`** — next release that lifts the `libsqlite3-sys < 0.38` cap.

After each, re-run `cargo deny check advisories` and remove the matching
entries from the `ignore` list in `deny.toml`.

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
