# Dependency policy

This document is the record of how we decide which dependency upgrades to take, when, and why some are deliberately held back. The Dependabot config in [`.github/dependabot.yml`](../.github/dependabot.yml) implements the *cadence* of the policy; this file explains the *judgement calls*.

## Cadence (what Dependabot proposes)

Dependabot is configured with three groups per ecosystem:

| Group              | What it bundles                                                                                   | Default disposition       |
| :----------------- | :------------------------------------------------------------------------------------------------ | :------------------------ |
| `<eco>-routine`    | Semver minor + patch only. Compatible under the ecosystem's stability contract.                    | Merge after CI passes.    |
| `<eco>-major`      | Semver-major bumps grouped together.                                                                | Review and split per dep. |
| `<eco>-security`   | Vulnerability fixes flagged by Dependabot's advisory database.                                      | Land same day if possible.|

A previous revision of the policy merged routine + major into one group. That produced PRs bundling 18 semver-major bumps each — including silently dragging in toolchains we'd explicitly rejected (e.g. `rumqttc 0.25` pulls `aws-lc-rs` + cmake). Splitting routine vs major is the lesson learned.

## How a routine bump is reviewed

1. Open the Dependabot PR.
2. Confirm only minor/patch versions changed (the PR title prefix should be `deps(<eco>)` and no entry in the diff crosses a semver-major boundary).
3. CI's lint + test + e2e matrix is the gate. If it goes green, merge.
4. If it goes red, fix forward — the routine group is supposed to be quiet, so a red routine PR means something either drifted on `main` or the bump exposed an existing bug worth investigating.

## How a major bump is reviewed

Major bumps go in the `<eco>-major` Dependabot stream. Each PR is expected to need splitting — that's the cost of taking the bump deliberately. The reviewer:

1. Reads the changelog of *every* upgraded crate in the PR — not just headline ones.
2. Identifies the transitive cost (does this dep drag in a new crypto stack, a new C build toolchain, a new MSRV floor, a new runtime?).
3. Either:
   - **Cherry-picks** the safe bumps into the workspace and closes the rest of the PR with a comment; or
   - **Defers** the whole PR, recording the reason in the [Deferred majors](#deferred-majors) section below; or
   - **Migrates** all of them in a separate PR with proper testing + a CHANGELOG entry, then closes the Dependabot PR as superseded.

## Deferred majors

Living list of dependency major bumps we are aware of and deliberately holding back. When something here gets merged into `main`, delete the row.

### Cargo

| Dependency            | Current | Latest      | Why deferred                                                                                                                                                                                                                                                                              |
| :-------------------- | :------ | :---------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `axum`                | 0.7     | 0.8         | Route-handler trait signatures changed; the rewrite ports cleanly but every handler in `rampart-api::routes::*` needs a touch + a regression pass. Move scheduled together with the tower 0.5→0.6 bump it pairs with.                                                                       |
| `axum-extra`          | 0.9     | 0.12        | Tied to `axum` 0.8 — bump together or not at all.                                                                                                                                                                                                                                          |
| `sqlx`                | 0.8     | 0.9         | The `.sqlx/` offline cache binary format changed. Regenerating the cache is mechanical (`cargo sqlx prepare`); the wait is for the `query!` macro's edition2024 dependency to stop double-resolving features against `time` (currently noisy diagnostic on every recompile).               |
| `thiserror`           | 1       | 2           | Derive-macro signature changed for `#[error(transparent)]`. About 35 enum variants across `rampart-core`, `rampart-db`, `rampart-api`, `rampart-notifier` need to be re-checked.                                                                                                            |
| `rand`                | 0.8     | 0.10        | `RngCore` moved to a default-method-on-trait layout that breaks the few sites where we name `RngCore` directly. Pure renames; deferred only because it pairs with the `sha2 0.11` / `hmac 0.13` / `hkdf 0.13` migration below.                                                              |
| `sha2`, `hmac`, `hkdf` | 0.10 / 0.12 / 0.12 | 0.11 / 0.13 / 0.13 | These force migration to `generic-array 1.x`. Our webpush_crypto module currently pins `generic-array 0.14.7` because 0.14.9 marks the old slice-accessor API as deprecated (turned into a hard error by `-D warnings`). The 1.x migration is straightforward; deferred until we batch it with the rest of the RustCrypto bumps. |
| `redis`               | 0.27    | 1.2         | Wholesale API rewrite. The redis probe in `rampart-checker/src/redis.rs` needs to be re-written, not patched.                                                                                                                                                                              |
| `tonic`               | 0.12    | 0.13        | Generated-code prefix changed; `prost` 0.14 is required in lockstep. Re-run codegen, audit the gRPC health probe.                                                                                                                                                                          |
| `prost`               | 0.13    | 0.14        | Tied to `tonic` 0.13.                                                                                                                                                                                                                                                                      |
| `rumqttc`             | 0.24    | 0.25        | **Rejected, not just deferred.** 0.25 unconditionally adds `aws-lc-rs` + `cmake` to the build graph, which contradicts the pure-Rust crypto invariant the project advertises (see README "Why Rampart" + `docs/DESIGN-ORIGINAL.md`). Will revisit only if upstream restores a featureless rustls path. |
| `tokio-tungstenite`   | 0.24    | 0.29        | Default features changed; we rely on the `rustls-tls-webpki-roots` feature being available without aws-lc-rs. Verify the feature set still composes the same crypto stack before bumping.                                                                                                  |
| `bollard`             | 0.17    | 0.21        | Docker API client. Multiple breaking changes across the path from 0.17 → 0.21. The docker probe is one file; the migration is mechanical, just hasn't been scheduled.                                                                                                                       |

### npm

| Dependency            | Current   | Latest    | Why deferred                                                                                                                                                                                                                                                                                                                                       |
| :-------------------- | :-------- | :-------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `react`, `react-dom`  | 18.3.1    | 19.x      | React 19 ships the React Compiler, removes `defaultProps` for function components, changes Suspense semantics, and removes `PropTypes` from `react`. The codebase doesn't use `defaultProps` or `PropTypes` but does have ~6 lazy-loaded views whose Suspense boundaries need a regression pass. Pair with `vite` 8 + `@vitejs/plugin-react` 6.       |
| `vite`                | 7.3.5     | 8.x       | Vite 8 requires Node 22+ at build time (the Dockerfile is now node:26-alpine so the floor is satisfied — but Vite 8 also tightens the plugin API in ways that may need `@vitejs/plugin-react` 6, which in turn wants React 19). Move all three together.                                                                                              |
| `@vitejs/plugin-react`| 5.2.0     | 6.0.2     | Tied to `vite` 8.                                                                                                                                                                                                                                                                                                                                  |
| `lucide-react`        | 0.383.0   | 1.17.0    | Package re-released as 1.x with the icon import path changed from `lucide-react` to `lucide-react/icons/<Name>`. Tree-shaking is *better* under 1.x but every icon import in the codebase (~60 sites) needs a path rewrite. Mechanical, but deserves its own PR.                                                                                    |
| `recharts`            | 2.15.4    | 3.8.1     | Chart props renamed (`stroke` color tokens), `ResponsiveContainer` semantics changed, and the new release dropped IE-era polyfills the dashboard didn't use anyway. Audit the response-time chart in `Dashboard.jsx` + the per-monitor latency chart in `MonitorDetail.jsx` before bumping.                                                          |

When you take any of these on, the workflow is:

1. Open a PR titled `deps: bump <name> X→Y`.
2. Touch only what the bump requires + a regression test that exercises the changed surface.
3. Update this file: remove the row from the deferred list; add a sentence to [`CHANGELOG.md`](../CHANGELOG.md) under `[Unreleased]`'s `Changed` heading.

## Adding a new dependency

Before adding a crate or npm package:

1. **Check the license** is in `backend/deny.toml`'s allow-list (Cargo) or is OSI-approved (npm). AGPL-3.0 is the project license; copyleft incompatibility with deps means we can't ship.
2. **Check the build toolchain** it drags in. Pure-Rust + pure-JS is the goal; introducing `cmake`, `openssl-sys`, `nan`, or a system header dependency needs a justification in the PR body.
3. **Check the maintenance signal** — a crate with a single commit a year ago and no responsive issues queue is a future security debt. The project leans toward fewer, better-maintained deps over more, fresher ones.
4. **Mention the addition in `CHANGELOG.md`'s `[Unreleased] → Added`** so the release notes capture the transitive cost.
