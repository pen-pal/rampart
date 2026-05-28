# Maintainer Setup

One-time repository configuration that can't live in a committed file.
Run these once after the scaffolding lands (requires `gh` authenticated as
a repo admin). Re-running is safe.

## 1. Labels

Labels are defined in [`.github/labels.yml`](../.github/labels.yml) and
synced by the `labels` workflow on push to `main` (or run it manually from
the Actions tab). No manual step needed — just merge the manifest.

## 2. Enable repository features

```bash
# Discussions (the issue template + SUPPORT.md route questions here)
gh api -X PATCH repos/pen-pal/Rampart -f has_discussions=true

# Dependabot alerts + security updates
gh api -X PUT repos/pen-pal/Rampart/vulnerability-alerts
gh api -X PUT repos/pen-pal/Rampart/automated-security-fixes
```

Private security advisories are enabled by default on public repos; confirm
under **Settings → Code security and analysis**. CodeQL + dependency review
run from the committed workflows once the repo is public (CodeQL requires
GitHub Advanced Security, which is free for public repos).

## 3. Branch protection on `main`

```bash
gh api -X PUT repos/pen-pal/Rampart/branches/main/protection \
  -H "Accept: application/vnd.github+json" \
  -F "required_status_checks[strict]=true" \
  -F "required_status_checks[contexts][]=backend · clippy + fmt" \
  -F "required_status_checks[contexts][]=backend · cargo test --workspace" \
  -F "required_status_checks[contexts][]=frontend · vitest + build" \
  -F "required_status_checks[contexts][]=cargo deny (advisories + licenses + bans)" \
  -F "required_pull_request_reviews[required_approving_review_count]=1" \
  -F "required_pull_request_reviews[require_code_owner_reviews]=true" \
  -F "enforce_admins=false" \
  -F "restrictions=null"
```

Adjust the `contexts` to match the exact job names CI reports after the
first run (the names above mirror the `name:` fields in `ci.yml` and
`security-audit.yml`). CodeQL's check can be added once it has run once.

## 4. Secrets

No repository secrets are required for the default workflows — they use the
built-in `GITHUB_TOKEN`. Add secrets only if you wire optional integrations
later (e.g. a container registry other than GHCR).

## 5. Before the first public release

- Replace the placeholder contact in [`SECURITY.md`](../SECURITY.md)
  (`security@CHANGE-ME.example`) with a real, monitored address.
- Fill or remove the commented platforms in
  [`.github/FUNDING.yml`](../.github/FUNDING.yml).
- Tag `v0.x.y` to trigger the `release` (binaries) and `docker` (image)
  workflows.
