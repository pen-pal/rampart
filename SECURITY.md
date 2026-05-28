# Security Policy

## Supported versions

Rampart is pre-1.0 and ships from `main`. Security fixes land on `main` and
in the next tagged release. Only the latest release is supported — please
upgrade before reporting against an older tag.

## Reporting a vulnerability

**Do not open a public issue, discussion, or PR for a security problem.**

Two private channels, in order of preference:

1. **GitHub private advisory (preferred):**
   [Report a vulnerability](https://github.com/pen-pal/Rampart/security/advisories/new).
   This keeps the report private until a fix ships and gives us a place to
   collaborate on the patch.

2. **Email fallback:** `unameme@proton.me`
   Encrypt if you can; otherwise plain email is acceptable for the initial
   contact and we'll move to a private advisory.

Please include:

- A description of the issue and its impact.
- Steps to reproduce (a minimal monitor config, request, or PoC).
- Affected version / commit.
- Any suggested remediation.

## What to expect

- **Acknowledgement:** within 72 hours.
- **Triage + severity assessment:** within 7 days.
- **Fix target:** critical issues prioritized; coordinated disclosure once a
  patch is available.
- **Credit:** we'll credit you in the advisory unless you prefer to remain
  anonymous.

## Scope

In scope:

- The Rampart server (`rampart-api` and the crates it links).
- The official Docker image and `compose.yaml`.
- Authentication, session handling, API keys, 2FA, notification secrets.

Out of scope:

- Vulnerabilities in third-party services you point Rampart at (your SMTP
  server, an external headless-browser renderer, a notification provider).
- Issues that require an already-compromised host or database.
- Missing hardening headers on a deployment you misconfigured — see the
  production checklist in `docs/SETUP.md`.

## Hardening notes

- Always run behind TLS (reverse proxy or a TLS-terminating load balancer).
- Set a strong first-run admin password; sessions are HttpOnly + SameSite=Strict.
- Treat notification channel configs (webhook secrets, SMTP creds, cloud keys)
  as secrets — they are stored in the database.
- Restrict database network access to the Rampart process.
