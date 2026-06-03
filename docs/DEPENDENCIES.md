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
| RustCrypto bundle (`rand`, `sha2`, `hmac`, `hkdf`, `p256`, `aes-gcm`) | 0.8 / 0.10 / 0.12 / 0.12 / 0.13 / 0.10 | 0.10 / 0.11 / 0.13 / 0.13 / 0.14 / 0.11 | **Blocked on the upstream RustCrypto coordinated stable release.** These six crates have to bump together — `rand` 0.10 ships `rand_core` 0.9, the elliptic-curve / aead / digest traits in the rest of the bundle still want `rand_core` 0.6 in their current stable revs, so handing `SecretKey::random(&mut rand::rng())` a `ThreadRng` from `rand 0.10` fails the trait bound. As of 2026-06-04 the upstream coordinated rev exists only on the RC channel (e.g. `aes-gcm 0.11.0-rc.4`, `p256 0.14.0-pre`). Pinning to RC revs is not appropriate for a published security surface — wait for the matched stables. Mechanical changes when the rev lands: `thread_rng()` → `rng()`, `gen_range` → `random_range`, `use rand::Rng;` → `use rand::RngExt;`, and the `GenericArray::{as_slice,from_slice}` → `hybrid-array::Array` migration in `rampart-notifier/src/channels/webpush_crypto.rs`. |
| `rumqttc`             | 0.24    | 0.25        | **Rejected, not just deferred.** 0.25 unconditionally adds `aws-lc-rs` + `cmake` to the build graph, which contradicts the pure-Rust crypto invariant the project advertises (see README "Why Rampart" + `docs/DESIGN-ORIGINAL.md`). Will revisit only if upstream restores a featureless rustls path. |
| `reqwest`             | 0.12    | 0.13        | Attempted; rejected on the same crypto-stack grounds as `rumqttc`. The `rustls` feature in reqwest 0.13 routes through `hyper-rustls` 0.27, whose default provider is `aws-lc-rs` rather than `ring`. The features exposed (`rustls`, `rustls-no-provider`, private `__rustls-aws-lc-rs` and `__rustls`) do not give a clean public path to "rustls + ring + webpki-roots + no aws-lc-rs". `form` + `query` are also now per-method features that the workspace needed. Reverted to 0.12 (`93cbc9a` … this commit). Revisit when reqwest 0.13 exposes a public ring-only path or when the workspace migrates the rest of the rustls stack to `aws-lc-rs` deliberately. |

### npm

| Dependency            | Current   | Latest    | Why deferred                                                                                                                                                                                                                                                                                                                                       |
| :-------------------- | :-------- | :-------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|

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
