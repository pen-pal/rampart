-- Pre-auth OIDC login state — the per-attempt CSRF + PKCE + nonce stash.
--
-- This table holds the short-lived state created at /v1/auth/oidc/login and
-- consumed at /callback, replacing the previous in-process HashMap so the flow
-- survives across nodes / restarts (HA) and is one-time-use under a row lock:
--
--   * `state`         — the opaque CSRF token echoed back by the IdP in the
--                       redirect; PRIMARY KEY so each value is used once.
--   * `pkce_verifier` — the RFC 7636 code_verifier; one-time-use (the row is
--                       atomically DELETEd on consume, so a replayed callback
--                       can't re-exchange the code).
--   * `nonce`         — bound into the id_token `nonce` claim and checked on
--                       callback (replay / token-injection defence).
--   * `return_to`     — optional post-login redirect target (unused today;
--                       reserved so a deep-link login can round-trip it).
--
-- This is PRE-AUTH, cross-tenant state: it is NOT org-scoped and intentionally
-- carries NO `org_id` and NO `org_isolation` RLS policy, so migration 0116's
-- enable-loop (which enables RLS only on tables carrying that policy) skips it.
-- Rows live ~10 minutes (STATE_TTL_SECS); the hourly prune sweep reaps expired
-- rows (and `consume` ignores any row past `expires_at`).
CREATE TABLE oidc_login_state (
    state           TEXT        PRIMARY KEY,
    pkce_verifier   TEXT        NOT NULL,
    nonce           TEXT,
    return_to       TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_oidc_login_state_expires_at ON oidc_login_state (expires_at);
