# SAML 2.0 SP + SCIM 2.0 provisioning — design & phased plan

Status: **DESIGN / NOT STARTED.** Task #104 — enterprise SSO (SAML 2.0 Service
Provider) + automated user/group provisioning (SCIM 2.0). This document is the
plan of record; **no implementation has begun** and the first real phase is
owner-gated. It is written in the same spirit as `docs/design/MULTI_DB.md`:
honest framing first, every load-bearing claim grounded in the *existing* tree
with file:line, integration points, a slice plan, and the explicit owner
sign-offs the work cannot proceed without.

The honest framing, up front: **the SAML half has a real dependency-policy
collision, and the SCIM half is mostly a thin protocol shim over CRUD that
already exists.** SAML's only mature Rust SP crate (`samael`) pulls a C crypto
toolchain (`xmlsec1` → `libxml2` + `openssl`), which directly violates the
workspace's pure-Rust / ring-only crypto invariant that is enforced today across
~10 deps and `docs/DEPENDENCIES.md`. That collision — not the protocol — is the
hard decision in this doc. SCIM, by contrast, maps almost one-for-one onto the
existing `users` + `org_members` model and the bearer-token auth pattern already
shipped for API keys; its risk is semantic (deprovisioning, last-admin, JIT race)
not architectural.

---

## Why SAML is the hard part (the facts, verified against the tree)

### 1. The dependency-policy collision (the central decision)

The workspace advertises and *enforces* a pure-Rust, ring-only crypto stance.
This is not aspirational — it is wired into nearly every TLS-bearing dependency:

| Evidence | Where |
|---|---|
| reqwest pinned to `rustls-no-provider` specifically to avoid `aws-lc-rs` | `backend/Cargo.toml:95-101` |
| ring `CryptoProvider` installed as the global default at boot | `backend/crates/rampart-api/src/main.rs:55-63` |
| `async-nats`, `ldap3`, `lapin`, `tonic`, `tokio-rustls`, `rustls`, cassandra driver all hand-pick `ring` and explicitly reject the `aws-lc-rs`/`openssl` feature | `backend/Cargo.toml:138-209` |
| `rumqttc 0.25` **rejected** (not deferred) solely because it "unconditionally adds `aws-lc-rs` + `cmake`" | `docs/DEPENDENCIES.md:44` |
| Adding-a-dependency checklist requires justification for any `cmake`/`openssl-sys`/system-header dep | `docs/DEPENDENCIES.md:62` |
| OIDC was built precisely to avoid this — `jsonwebtoken = "10"` is pure-Rust, JWKS verify is in-process (`oidc.rs:376-428`), no C linkage | `oidc.rs`, `Cargo.toml:238` |

`samael` (the de-facto Rust SAML SP crate) depends on `xmlsec` for XML-DSig
signature verification, which links `xmlsec1` + `libxml2` + `openssl` as **system
C libraries**. Adopting it would:
- introduce a `cmake`/system-header build dependency the project has explicitly
  rejected at least twice (`DEPENDENCIES.md:44`, `:62`);
- add a second crypto provider (`openssl`) alongside the ring default installed
  in `main.rs:63` — exactly the multi-provider situation the reqwest/nats/ldap
  pins exist to prevent;
- break the "no system packages, single static-ish binary / homelab-friendly"
  posture the README leans on.

**This is the owner decision SAML hinges on.** There is no pure-Rust crate today
that does XML-DSig + SAML SP assertion validation to a security-acceptable
standard. The options are laid out in "Crate choice" below; none is free.

### 2. SAML is XML-DSig, and XML signature validation is a security minefield

Unlike OIDC's signed-JWT (a compact, canonical, single-line token that
`jsonwebtoken` verifies with no canonicalization ambiguity), a SAML assertion is
a signed XML *document*. The validation surface is notoriously dangerous:

- **XML Signature Wrapping (XSW):** the classic SAML attack — wrap a forged
  assertion around (or alongside) the legitimately-signed one so the signature
  check passes over element A while the SP reads identity from element B. Mitigated
  only by validating the signature over *exactly* the element whose contents are
  consumed, with schema-hardening and a single-assertion policy.
- **XXE / entity expansion / DTD:** the XML parser must disable external entities
  and DTDs entirely.
- **Canonicalization (C14N):** must be exact; this is the part `samael` defers to
  `xmlsec1` (the C dep) and the part a hand-rolled Rust impl is most likely to get
  subtly wrong.
- **Algorithm allow-listing:** reject SHA-1 digests/signatures and any
  symmetric/`none` transform — the SAML analog of OIDC's alg-confusion defence
  (which `oidc.rs:74-84,394-407` already does for JWT). We must replicate that
  rigor for XML-DSig.

The upshot: the cryptographic core of SAML is the risky part, and it is *exactly*
the part the C dep exists to handle. A "just parse the XML ourselves" path trades
a dependency-policy violation for a far worse security-correctness risk. This
tension is the spine of the recommendation.

### 3. What SAML does NOT need to reinvent (the good news)

Everything *after* a validated assertion is plumbing OIDC already built. The
post-validation flow is byte-for-byte the OIDC callback's second half
(`oidc.rs:538-665`): take verified identity claims → find-or-provision user →
map org by slug claim → mint a server-side session → set the cookie. SAML SP is
"replace the front half (discovery/token/JWKS-verify) with
(metadata/AuthnRequest/XML-DSig-verify), reuse the entire back half."

---

## SCIM is mostly a shim over existing CRUD (the facts)

SCIM 2.0 (RFC 7642/7643/7644) is a REST/JSON protocol an IdP (Okta, Entra,
OneLogin) calls to push user + group lifecycle into the SP. Mapping it to
Rampart:

| SCIM concept | Existing Rampart primitive | File:line |
|---|---|---|
| `User` resource (core schema) | `users` row: `email`, `name`, `active` ↔ anonymize/disable | `rampart-db/src/users.rs:10-46` |
| Create `User` | `create_user(NewUser)` (already seeds Default-org membership atomically) | `users.rs:55-107`, store seam `store.rs:1592` |
| Read/list `User` | `get_user`, `list_users`, `get_user_by_email` | `users.rs:109-187,287` |
| Patch `User.active=false` (deprovision) | `anonymize` + session revoke (GDPR-erase already does exactly this) | `users.rs:394-414`, `routes/users.rs:245-271` |
| `Group` resource | `organizations` row | `rampart-db/src/orgs.rs:28-54` |
| Group membership add/remove | `upsert_member` / `remove_member` | `orgs.rs:101-128,236-245` |
| Group member role | `org_members.role` (Admin/Editor/Readonly) | `orgs.rs:131-140` |
| Bearer-token auth for the SCIM endpoint | `api_keys::lookup` (SHA-256 hash, unique index, single-shot) is the exact precedent | `rampart-db/src/api_keys.rs:133-159` |

The membership invariant Rampart already maintains — "every user is a member of
at least one org" (`users.rs:60-95`) — and the last-admin protection
(`orgs.rs:249-258`, `routes/orgs.rs:297-308`) are the two semantic guards SCIM
must respect rather than reimplement.

**The gaps SCIM genuinely adds** (not reuse): the SCIM wire schema
(`urn:ietf:params:scim:schemas:core:2.0:User`/`:Group`), the SCIM error envelope
(`urn:ietf:params:scim:api:messages:2.0:Error`), `PATCH` with the SCIM
path/op grammar, `/ServiceProviderConfig` + `/ResourceTypes` + `/Schemas`
discovery endpoints, and list filtering (`filter=userName eq "x@y.com"`). These
are protocol surface, not new business logic.

---

## Crate choice — SAML (the decision matrix)

| Option | Pure-Rust? | C/toolchain deps | XML-DSig maturity | Verdict |
|---|---|---|---|---|
| **`samael`** | No | `xmlsec1`+`libxml2`+`openssl` (system) | Mature (delegates to `xmlsec1`) | Correct crypto, **violates `DEPENDENCIES.md:44,62` + the ring-only invariant**. Owner-gated. |
| **Pure-Rust hand-roll** (`quick-xml` + `rsa`/`ring` for RSA-SHA256 + a C14N impl) | Yes | none | We own the XSW/C14N risk surface | Policy-clean but **shifts the danger to us** (§2). Largest engineering + security-review cost. |
| **`xml-rs`/`roxmltree` + manual DSig** | Yes | none | Same as above | Same tradeoff. |
| **Decline SAML; OIDC-only** | n/a | n/a | n/a | Most enterprise IdPs (Okta/Entra/Ping/OneLogin) speak OIDC too — OIDC already ships (`oidc.rs`). Honest "use OIDC for SSO" stance. |
| **`samael` behind an off-by-default cargo feature** | conditional | only when feature on | Mature | Keeps the default binary pure-Rust; SAML is an opt-in build for shops that need it and accept the C toolchain. **Recommended compromise.** |

**Recommendation: the cargo-feature compromise (`saml` feature, default-off).**
It preserves the pure-Rust default binary that the README and `DEPENDENCIES.md`
promise, confines the `xmlsec1`/`openssl` toolchain to operators who explicitly
opt in (mirroring how probe TLS features are hand-picked per-crate), and avoids
both the dependency-policy violation *for the default build* and the
unbounded-security-risk of a hand-rolled XML-DSig. The owner must still sign off
that a *feature-gated* C-crypto path is acceptable at all (it is a softer version
of the `rumqttc 0.25` rejection — the difference is it never enters the default
graph). If the owner rejects even feature-gated C deps, fall back to
**decline SAML, ship SCIM + lean on OIDC for SSO** — which still satisfies the
"enterprise provisioning" half of #104 cleanly.

> A `cargo deny` allow-list entry and a `CHANGELOG [Unreleased] → Added` note are
> mandatory if `samael` lands even feature-gated (`DEPENDENCIES.md:61,64`).

---

## SAML SP — the flow (grounded in the OIDC plumbing it reuses)

Routes mount **public**, exactly like OIDC (`routes/mod.rs:89`
`.nest("/auth/oidc", oidc::router())`), under `/v1/auth/saml`, and like OIDC the
pre-auth state INSERT must sit behind the existing per-IP rate limiter
(`routes/mod.rs:84` note about unauthenticated `oidc_login_state` inserts):

```
GET  /v1/auth/saml/config    → { enabled }            (mirror oidc config_endpoint, oidc.rs:432)
GET  /v1/auth/saml/metadata  → SP metadata XML (EntityID, ACS URL, SP signing cert)
GET  /v1/auth/saml/login     → 302 redirect-binding AuthnRequest to the IdP SSO URL
POST /v1/auth/saml/acs       → Assertion Consumer Service: validate → provision → session
```

1. **Metadata** (`/metadata`): emit static SP metadata XML — EntityID, the ACS
   URL, NameIDFormat (`emailAddress`), and the SP's signing/encryption cert. This
   is what the operator uploads to their IdP. Config is env-driven, mirroring
   `oidc.rs:118-133`:
   - `RAMPART_SAML_IDP_METADATA_URL` (or inline XML) — the IdP's EntityID, SSO
     URL, and signing certificate;
   - `RAMPART_SAML_SP_ENTITY_ID`, `RAMPART_SAML_SP_ACS_URL`;
   - `RAMPART_SAML_SP_CERT` / `RAMPART_SAML_SP_KEY` (PEM) for signing AuthnRequests
     + decrypting EncryptedAssertions;
   - `RAMPART_SAML_DEFAULT_ROLE` (reuse the `oidc.rs:126-130` parse);
   - `RAMPART_SAML_ORG_ATTRIBUTE` — the SAML attribute carrying org slugs,
     **reusing `claim_org_slugs` + `normalize_slug` verbatim** (`oidc.rs:225-260`).
   `enabled` = all required vars present (mirror `config()` `oidc.rs:118`).

2. **AuthnRequest** (`/login`): generate the request id + RelayState, **stash
   them in the existing `oidc_login_state` table** (rename to `sso_login_state`,
   slice S0) using the already-shipped one-time-use stash/consume pattern
   (`rampart-db/src/oidc_state.rs:27-83`). The `pkce_verifier` column is unused for
   SAML; `state` holds the AuthnRequest id, `return_to` holds RelayState. Redirect
   the browser (HTTP-Redirect binding) to the IdP SSO URL with the deflated,
   base64'd, URL-encoded AuthnRequest. The existing `urlencoding` helper
   (`oidc.rs:670-681`) and the SSRF-guarded client (`oidc.rs:150-157`) for the
   metadata fetch are reused.

3. **ACS** (`/acs`, POST form): the IdP POSTs `SAMLResponse` (base64 XML) +
   `RelayState`. This is the security-critical handler. In order — the SAML analog
   of `validate_id_token` (`oidc.rs:384-428`):
   1. base64-decode; parse with **external entities + DTD disabled** (XXE);
   2. verify the XML-DSig signature over the assertion against the IdP's metadata
      cert, with a **single-assertion policy** and signature-references-the-consumed-
      element check (XSW defence, §2);
   3. allow-list digest/signature algorithms (reject SHA-1 / `none` —
      mirror the ASYM allow-list intent of `oidc.rs:74-84`);
   4. validate conditions: `Audience` == SP EntityID, `NotBefore`/`NotOnOrAfter`
      window (clock skew leeway like the JWT `leeway 60`, `oidc.rs:417`),
      `Recipient` == ACS URL, `InResponseTo` == the stashed AuthnRequest id
      (consume it one-time from `sso_login_state` — replay defence, exactly the
      `consume_oidc_state` discipline `oidc.rs:488-492`);
   5. extract NameID (email) + attributes (name, org attribute).
   - **Refuse an unverified/absent email** — SAML has no `email_verified`, so the
     trust model is "the IdP asserted this NameID"; treat a successfully-validated
     assertion's NameID as authoritative, but still require it to look like an
     email (`oidc.rs:591-596`) and lowercase it.

4. **Provision + session — REUSE OIDC verbatim** (`oidc.rs:598-665`): find user by
   email or `create_user` (first user → Admin, else `DEFAULT_ROLE`), grant org
   membership for each matched slug via `upsert_org_member` + pick the first as
   active org, `mark_user_login`, `create_session`, `set_session_active_org`,
   `build_session_cookie`. This back half should be **extracted into a shared
   `sso::provision_and_session(...)` helper** (slice S1) so OIDC and SAML call one
   function — avoids the find-or-provision logic drifting between the two.

---

## SCIM 2.0 — endpoint design (grounded in users/orgs CRUD)

Mount under `/v1/scim/v2`, **protected by a dedicated SCIM bearer-token layer**
(not `require_session`, not the user-facing api-key path — see auth below).

```
GET    /v1/scim/v2/ServiceProviderConfig          (static capabilities doc)
GET    /v1/scim/v2/ResourceTypes  /Schemas        (static)
GET    /v1/scim/v2/Users?filter=userName eq "…"   → list_users / get_user_by_email
GET    /v1/scim/v2/Users/{id}                      → get_user
POST   /v1/scim/v2/Users                           → create_user (JIT provision)
PUT    /v1/scim/v2/Users/{id}                      → name/active replace
PATCH  /v1/scim/v2/Users/{id}                      → active=false ⇒ anonymize+revoke
DELETE /v1/scim/v2/Users/{id}                      → anonymize (NOT hard delete; FK + audit chain)
GET    /v1/scim/v2/Groups  /Groups/{id}            → orgs::get / list
POST   /v1/scim/v2/Groups                          → create_org_with_owner (or create)
PATCH  /v1/scim/v2/Groups/{id}                     → members add/remove ⇒ upsert/remove_member
```

### Mapping decisions
- **`User.userName` ↔ `email`** (the citext unique key, `users.rs:115`). `id` is
  the Rampart `UserId` UUID.
- **`User.active=false` ⇒ deprovision = `anonymize` + session/recovery revoke**,
  reusing the exact GDPR-erase path (`routes/users.rs:245-271`). Rationale already
  documented in `users.rs:386-393`: a hard DELETE is impossible because
  `audit_log.actor_user_id` RESTRICTs. SCIM `DELETE` maps to the same anonymize.
  This is the single most important semantic decision — an IdP deprovision MUST
  revoke access, and anonymize-in-place is the only correct path given the schema.
- **`Group` ↔ `organization`**, `Group.displayName` ↔ org name,
  `externalId`/membership maps to `org_members`. `Group` member ops respect
  **last-admin protection** (`orgs.rs:249`, `routes/orgs.rs:297-308`) — a SCIM
  member-removal that would orphan an org returns SCIM 409, not a 500.
- **Role mapping:** SCIM has no native role concept for our 3-role model. Map via
  a configured default (`RAMPART_SCIM_DEFAULT_ROLE`) for JIT-created users; group
  membership role defaults to the same, with the existing `set_member_role`
  reachable only via the app UI for fine-grained changes. (Okta/Entra "role push"
  via SCIM enterprise extension is a later slice — call it out as out-of-scope v1.)
- **Org scoping (multi-tenancy):** the SCIM bearer token is **minted for and pinned
  to one org** (the api-key org-pinning precedent, `api_keys.rs:133-159`,
  `auth.rs:230-235`). A token's `POST /Users` JIT-provisions the user AND grants
  them membership in the token's org — so SCIM provisioning is tenant-scoped by
  construction, consistent with the Phase-6 "keys pinned to their minting org"
  model (`auth.rs:228-235`).

### SCIM bearer-token auth
Reuse the api-key shape but a **separate token type + table** (`scim_tokens`,
one per org, SHA-256 at rest, unique index — clone `api_keys.rs:133-159`):
- a dedicated middleware layer (sibling to `require_session`, `auth.rs:196`) that
  resolves the SCIM token → `(org_id)` and rejects with the **SCIM error
  envelope** (not the generic `ApiError`) on failure;
- the SCIM layer does NOT resolve a `User`/`OrgContext` the way `require_session`
  does — SCIM acts *as the IdP*, on behalf of the org, not as a logged-in user.
  All writes are stamped with the token's org and a synthetic actor for audit.
- audit every SCIM mutation through the existing `crate::audit::record`
  (`routes/users.rs:94-103` pattern) with an `actor` representing the SCIM
  integration, so provisioning is on the tamper-evident chain.

### JIT provisioning
SAML/OIDC do JIT at login (`oidc.rs:598-618`). SCIM does JIT ahead of login via
`POST /Users`. Both must converge on the **same** find-or-provision helper (the
S1 `sso::provision_and_session` back-half, minus the session for SCIM) so a user
provisioned by SCIM and one auto-created at first SAML login are identical rows
(same Default-membership invariant, same first-user→Admin rule). The race —
SCIM POST and a first SAML login arriving near-simultaneously for the same email
— is handled by the existing unique-violation→`Conflict` mapping in
`users::create` (`users.rs:81-86`): whoever loses the insert re-reads by email.

---

## Schema / migrations (next number: 0121)

Grounded in `backend/migrations/` (latest `0120_logs_org_received_at_index.sql`):

1. **`0121_sso_login_state.sql`** — rename/generalize `oidc_login_state`
   (migration `0119`) to `sso_login_state` (or add a `kind` column:
   `oidc`/`saml`). The columns already fit: `state`, `pkce_verifier` (NULL for
   SAML), `nonce` (NULL for SAML — InResponseTo lives in `state`), `return_to`
   (RelayState), `expires_at`. The hourly prune (`oidc_state.rs:87-92`) covers it
   unchanged. **This is a rename of a pre-auth, non-org-scoped, no-RLS table
   (`oidc_state.rs:11-12`)** — low blast radius.
2. **`0122_scim_tokens.sql`** — `scim_tokens(id, org_id, token_hash UNIQUE,
   name, created_by, created_at, last_used_at, expires_at)`. `org_id` NOT NULL +
   FK `ON DELETE RESTRICT` to match the `0112`/`0108` org-column convention and the
   RLS posture (`0114`–`0116`). Mirror `api_keys` exactly.
3. **(optional) `0123_users_external_id.sql`** — `users.scim_external_id TEXT`
   (the IdP's stable user id) so a userName/email change at the IdP can re-link by
   external id rather than email. Needed for correct rename handling; can defer to
   a later slice if v1 keys on email only.

No new enum, no RLS-policy change for the SCIM path beyond the standard org-scoped
columns (`scim_tokens` follows the `0114-0116` pattern). The `sso_login_state`
rename deliberately stays out of RLS (it is pre-auth, `oidc_state.rs:11-12`).

---

## Store-seam integration

Both features sit cleanly behind the existing `Arc<dyn Store>` seam
(`rampart-db/src/store.rs`, multi-DB P0 complete). New methods follow the
established mirror-the-free-fn pattern (`store.rs:103-117`):
- SCIM tokens: `create_scim_token` / `lookup_scim_token` / `list_scim_tokens` /
  `revoke_scim_token` — clone the api-key methods (`store.rs:355`).
- SAML reuses the already-seam'd `create_user`, `get_user_by_email`, `count_users`,
  `upsert_org_member`, `org_by_slug`, `create_session`, `set_session_active_org`,
  `mark_user_login` (`store.rs:1592-4201`) — **zero new store methods for SAML
  identity**, which is the whole point of reusing the OIDC back half.
- `oidc_state` seam methods (`stash_oidc_state`/`consume_oidc_state`,
  `oidc.rs:451,489`) are reused as-is; only the underlying table renames.

The XML-DSig + assertion parsing lives **above** the store boundary in
`rampart-api` (it is app-layer crypto, like `secrets.rs` in MULTI_DB §"Secrets")
— the store never sees XML.

---

## Phased plan

- **S0 — Generalize pre-auth login state (PG-only, no new feature).** Rename
  `oidc_login_state` → `sso_login_state` (migration 0121) + the
  `rampart-db/src/oidc_state.rs` module; keep OIDC green. Pure refactor, behind
  the existing tests (`oidc_state.rs:94-152`). Ships nothing user-visible but
  unblocks SAML's reuse of the one-time-use state machine.
- **S1 — Extract the SSO back half.** Pull `oidc.rs:598-665` (find-or-provision +
  org-map + session) into `sso::provision_and_session`; OIDC calls it. Zero
  behavior change, regression-covered by the OIDC flow. Prereq for SAML and SCIM
  JIT to share one code path.
- **S2 — SCIM read + auth (no writes).** `scim_tokens` table (0122), the SCIM
  bearer middleware + error envelope, `ServiceProviderConfig`/`Schemas`/
  `ResourceTypes`, `GET /Users`/`/Groups`. Lowest-risk SCIM slice — read-only,
  proves the token auth + wire schema before any mutation.
- **S3 — SCIM write + lifecycle.** `POST/PUT/PATCH/DELETE Users` (JIT via the S1
  helper, deprovision via anonymize), `PATCH Groups` membership (respecting
  last-admin). The semantically risky slice — gate on the deprovision sign-off.
- **S4 — SAML SP (owner-gated on the crate decision).** Only after the §"Crate
  choice" owner decision. Feature-gated `saml` build with `samael` (or the chosen
  alternative): metadata, AuthnRequest, ACS validation, calling the S1 helper.
- **S5 — Frontend + docs.** Admin UI: SCIM-token mint/list (clone the api-keys
  view), SAML enable + metadata download, an SSO button (mirror the OIDC
  `/config` `enabled` toggle, `oidc.rs:432`). Docs: IdP setup guides
  (Okta/Entra/OneLogin) for both protocols.

Order rationale: **SCIM first** (S2/S3) — it is the policy-clean, mostly-reuse
half that delivers the "enterprise provisioning" value of #104 without touching
the C-dep question. SAML (S4) is sequenced last and explicitly gated.

---

## Named risks the owner must accept explicitly (not absorbed as scope)

1. **The `samael` C-dependency policy collision (the headline).** Adopting
   `samael` introduces `xmlsec1`+`libxml2`+`openssl` — a `cmake`/system-header
   toolchain the project has rejected twice (`DEPENDENCIES.md:44,62`) and a second
   crypto provider beside the ring default (`main.rs:63`). The
   feature-gated-default-off compromise contains it but does **not** eliminate it
   for operators who enable SAML. Owner must sign off that *any* C-crypto, even
   opt-in, is acceptable — or accept "decline SAML, OIDC-only for SSO."
2. **Hand-rolled XML-DSig is a worse risk than the C dep.** If the owner rejects
   `samael` entirely, the pure-Rust alternative means *we* own XSW, C14N, XXE, and
   algorithm-confusion correctness for SAML — a security surface OIDC deliberately
   avoided by using compact JWTs (§2). This needs a dedicated security review and
   should not be undertaken to satisfy a policy preference if it raises real
   compromise risk.
3. **SCIM deprovision semantics = anonymize, not delete.** An IdP `active=false`
   or `DELETE` maps to `anonymize` + session revoke (`users.rs:394-414`), NOT a
   row delete (impossible: `audit_log.actor_user_id` RESTRICTs, `users.rs:387-389`).
   The owner must accept that SCIM-deprovisioned users persist as anonymized
   tombstones (correct for audit integrity, but an IdP admin may expect the row to
   vanish). Document this in the IdP setup guide.
4. **SCIM token = org-wide provisioning credential.** A leaked `scim_tokens` row
   can create/disable users and reshape org membership for its org. It must be
   SHA-256-at-rest (`api_keys.rs` precedent), revocable, audited on every
   mutation, and ideally scoped narrower than a full admin api-key. Treat it as a
   high-value secret on par with an admin session.
5. **Last-admin / membership-invariant interaction.** SCIM group-membership and
   user-deprovision must not orphan an org's last admin (`orgs.rs:249`) nor violate
   "every user in ≥1 org" (`users.rs:60-95`). These guards exist for the UI path;
   SCIM must route through the same store methods (S1 helper + `remove_member`
   with the `count_org_admins` check), returning SCIM 409 — not bypass them with
   raw SQL.
6. **No `email_verified` in SAML.** OIDC refuses unverified emails
   (`oidc.rs:575-581`); SAML has no equivalent claim. The trust anchor shifts to
   "the assertion's signature validated against the configured IdP cert." If the
   IdP federates unverified identities, Rampart inherits that — call it out in the
   SAML setup guide and keep the IdP cert/EntityID pinning strict.
7. **Maintenance signal of `samael`.** Per `DEPENDENCIES.md:63`, a SAML crate with
   thin maintenance is future security debt on the most security-sensitive code
   path in the app. Verify the crate's issue-queue responsiveness before S4.

---

## Recommendation (owner-gated)

1. **Ship SCIM first (S0→S3), independent of the SAML decision.** It is
   policy-clean (pure-Rust JSON/REST, no new crypto), ~80% reuse of existing
   `users`/`orgs`/api-key plumbing, and delivers the "enterprise provisioning"
   value of #104. Gate S3 only on risk #3 (anonymize-deprovision) sign-off.
2. **Decide the SAML crate question before any SAML code (S4).** Preferred:
   `samael` behind a default-off `saml` cargo feature + a `cargo deny` allow-list
   entry + `CHANGELOG` note — containing the C toolchain to opt-in builds while
   keeping the default binary pure-Rust. Acceptable fallback: **decline SAML, lean
   on the shipped OIDC flow for SSO** (every major enterprise IdP speaks OIDC).
   Reject: a hand-rolled XML-DSig stack (risk #2) chosen purely to satisfy the
   no-C-dep preference.
3. **Do S0 + S1 regardless.** The login-state generalization and the SSO-back-half
   extraction are pure, regression-covered refactors that are good hygiene and
   shrink S4 to "front-half only" — worth doing even if SAML is ultimately
   declined.

### Explicit owner sign-offs required before work starts
- [ ] **SAML crate path:** feature-gated `samael` (C deps, opt-in) vs. decline
      SAML (OIDC-only) vs. pure-Rust hand-roll (risks #1/#2).
- [ ] **SCIM deprovision = anonymize-tombstone**, not hard delete (risk #3).
- [ ] **SCIM token is an org-wide provisioning secret** (risk #4) — acceptable.
- [ ] **SAML inherits the IdP's identity-verification posture** (no
      `email_verified`, risk #6).
- [ ] **Sequencing:** SCIM before SAML; S4 gated on the crate decision.

**Bottom line for the owner:** SCIM is a clean, mostly-reuse build that should
ship and satisfies the provisioning half of #104 on its own. SAML's only hard
question is the `samael` C-crypto dependency vs. the project's enforced pure-Rust
invariant — best resolved with a default-off cargo feature, but a legitimate
"OIDC is our SSO story" decline is also defensible given OIDC already ships.
