# Releasing Rampart

This document is the operating manual for cutting a release. It is the only place that needs reading when shipping a version — the rest of the repo is referenced by name from here.

The goal: a release is **one line bumped, one tag pushed**.

---

## Source of truth

A single field drives the version everywhere:

```toml
# backend/Cargo.toml
[workspace.package]
version = "X.Y.Z"
```

That value flows through automatically:

- **Every backend crate** inherits via `version.workspace = true`.
- **The running binary** reads it through `env!("CARGO_PKG_VERSION")` in `rampart-api/src/routes/health.rs`.
- **`/healthz`** returns it: `{"status":"alive","version":"X.Y.Z"}`.
- **`/metrics`** interpolates it into `rampart_build_info{version="X.Y.Z"} 1`.
- **The dashboard header pill** fetches it from `/healthz` at mount and renders `vX.Y.Z`.

`frontend/package.json` carries its own `version` field (npm requires one) — keep it in lockstep with the workspace version. There is no code path that reads it, but it's the answer if a contributor types `npm version` looking for the current value.

---

## Semantic versioning

| Bump  | When to use |
| :---  | :--- |
| **MAJOR** (`1.0.0`) | Breaking API change, schema migration that requires a manual operator step, or removal of a probe / channel kind. |
| **MINOR** (`0.2.0`) | New probe kind, new notification channel, new endpoint, new dashboard view, new CLI flag — anything additive. |
| **PATCH** (`0.1.1`) | Bug fix, security fix, dependency bump, docs, packaging, CI. |

The version starts at `0.x.y` — under `1.0.0` we reserve the right to break a thing if the lesson learned is worth it, but every such break still bumps the MINOR. A breaking change without a MINOR bump is a bug.

---

## Procedure

### 1. Decide the bump

Read [`CHANGELOG.md`](../CHANGELOG.md)'s `[Unreleased]` section. Pick MAJOR / MINOR / PATCH per the table above. If `[Unreleased]` is empty, there is nothing to release.

### 2. Bump the version

```bash
# Replace 0.2.0 with the new version.
sed -i '' 's/^version      = "[^"]*"$/version      = "0.2.0"/' backend/Cargo.toml
sed -i ''   's/"version": "[^"]*"/"version": "0.2.0"/'        frontend/package.json
cd backend && cargo update --workspace --offline   # refresh Cargo.lock metadata
```

Confirm nothing else moved:

```bash
git diff --stat
# Expect exactly: backend/Cargo.toml | backend/Cargo.lock | frontend/package.json
```

### 3. Promote `[Unreleased]` to a versioned section

In [`CHANGELOG.md`](../CHANGELOG.md):

1. Rename the `## [Unreleased]` heading to `## [X.Y.Z] — YYYY-MM-DD`.
2. Insert a fresh empty `## [Unreleased]` block above it.
3. Update the link references at the bottom — add a new `[X.Y.Z]` line pointing to the tag, and point `[Unreleased]` at the new compare range:

   ```markdown
   [Unreleased]: https://github.com/pen-pal/rampart/compare/vX.Y.Z...HEAD
   [X.Y.Z]:      https://github.com/pen-pal/rampart/releases/tag/vX.Y.Z
   ```

### 4. Verify locally

```bash
# Backend
cd backend
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
DATABASE_URL=postgres://rampart:rampart@localhost:5432/rampart cargo test --workspace
cargo deny check

# Frontend
cd ../frontend
npm ci
npm test
npm run build
```

Run the binary and curl `/healthz` to confirm the version you just bumped is what comes out the other end:

```bash
cd backend && cargo run -p rampart-api &
sleep 5 && curl -s http://localhost:3000/healthz | jq .
# {"status":"alive","version":"X.Y.Z"}
```

### 5. Commit and tag

One commit, one tag, both signed:

```bash
git add backend/Cargo.toml backend/Cargo.lock frontend/package.json CHANGELOG.md
git commit -m "release: vX.Y.Z"
git tag -s vX.Y.Z -m "Rampart vX.Y.Z

<paste the changelog section for this version here>
"
git push origin main
git push origin vX.Y.Z
```

### 6. Cut the GitHub release

```bash
gh release create vX.Y.Z \
  --title "Rampart vX.Y.Z" \
  --notes-file <(awk "/^## \[X.Y.Z\]/,/^## \[/" CHANGELOG.md | sed '$d')
```

The release body should be the same content that landed in the changelog section — the changelog is authoritative; the GitHub release page is a mirror.

### 7. Build + publish artifacts

The release tag triggers `.github/workflows/release.yml` which builds:

- The static binary (Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64) and uploads each as a release asset.
- A multi-arch Docker image to `ghcr.io/pen-pal/rampart:X.Y.Z` and `ghcr.io/pen-pal/rampart:latest`.

If the workflow fails, fix forward — don't move the tag.

---

## Patch a previous release

Sometimes a security fix needs to land on the prior MINOR without picking up unreleased work. The flow:

```bash
git checkout -b release/X.Y vX.Y.0          # branch off the tag
git cherry-pick <fix-commit>
# bump backend/Cargo.toml + frontend/package.json to X.Y.1
# update CHANGELOG.md with a new [X.Y.1] section, mention the fix
git commit -am "release: vX.Y.1"
git tag -s vX.Y.1
git push origin release/X.Y vX.Y.1
```

Then forward-port the fix to `main` separately. Don't try to share the commit between branches — cherry-picking keeps the history honest.

---

## What `[Unreleased]` should look like

Categorise entries under sub-headings — only include the ones that apply for a given release:

- **Added** — new probe / channel / endpoint / view / flag.
- **Changed** — observable behaviour different from the previous version.
- **Deprecated** — still works but is on the way out; planned removal version called out.
- **Removed** — gone in this release.
- **Fixed** — bug fix.
- **Security** — CVE / advisory addressed; cross-reference [`docs/SECURITY-DEBT.md`](SECURITY-DEBT.md) if applicable.

Write entries in the present tense, with enough context for a reader who has never touched the codebase to understand the impact. Reference issue / PR numbers where they add signal.

```markdown
### Added
- New `radius` probe kind exercises RFC 2865 Access-Request → Accept/Reject against a RADIUS server (#211).

### Fixed
- Heartbeats list now paginates via an `?before=<rfc3339>` cursor instead of a 50-row hard cap. Detail-view "Load older" now keeps going past the first page (#214).
```
