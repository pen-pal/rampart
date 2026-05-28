<!--
  Thanks for contributing to Rampart!
  Keep PRs focused — one logical change per PR reviews faster and reverts cleaner.
  Read CONTRIBUTING.md for scope, style, and the local dev loop.
-->

## Summary

<!-- What does this change and why? Link the issue it closes. -->

Closes #

## Type of change

- [ ] Bug fix (non-breaking)
- [ ] New feature (non-breaking)
- [ ] Breaking change (migration / API / config compatibility)
- [ ] Refactor / internal (no behavior change)
- [ ] Docs only
- [ ] CI / tooling

## How was this tested?

<!-- Commands you ran, manual steps, screenshots for UI. "It compiles" is not a test. -->

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo fmt --all -- --check` clean
- [ ] Frontend `npm run build` succeeds (if touched)
- [ ] Manually exercised the change in a running instance

## Database changes

- [ ] No schema change
- [ ] Adds a migration in `backend/migrations/` (sequential number, idempotent where possible)
- [ ] Regenerated the sqlx offline cache (`cargo sqlx prepare --workspace`) and committed `.sqlx/`

## Checklist

- [ ] My change fits Rampart's [scope](../blob/main/CONTRIBUTING.md#scope-read-this-first)
- [ ] I added/updated tests where it made sense
- [ ] I updated docs (`README.md` / `docs/`) if behavior changed
- [ ] No secrets, credentials, or `.env` files are included
- [ ] Commits are reasonably scoped with clear messages
