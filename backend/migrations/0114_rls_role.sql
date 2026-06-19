-- ─── Row-Level Security, slice 1: the application role + grants (NO RLS yet) ──
--
-- Defense-in-depth for multi-tenancy. App-level `WHERE org_id = $org` already
-- ships and is tested; RLS is belt-and-suspenders that a missed filter (or a
-- raw query) can't leak across orgs. This migration ONLY creates the role and
-- grants — it does NOT enable or force RLS on any table, and it does NOT define
-- any policy (that is slice 5, also dormant). Policies are inert until an
-- operator opts in. The whole feature is gated behind the `RAMPART_RLS` env
-- flag at the application layer; with the flag off the role is simply never
-- assumed, so behaviour is byte-identical.
--
-- ── How enforcement will work (once turned on, NOT here) ────────────────────
-- The app binds the per-request tenant org onto the checked-out connection via
-- `SET ROLE rampart_app` + `set_config('app.current_org', <org>, false)` (see
-- rampart-db `connect()` before_acquire hook, slice 3). System loops that never
-- bind an org run as the OWNER login role and BYPASS RLS for free. So:
--
--   * `rampart_app` is a NOLOGIN role the app *switches into* per tenant
--     request. It is NOT a BYPASSRLS role — under FORCE it is subject to the
--     org_isolation policies.
--   * The OWNER / login role (the `db_user` in DATABASE_URL) must be granted
--     BYPASSRLS by the operator so the system loops keep working. We cannot do
--     that here: ALTER ROLE … BYPASSRLS needs superuser and we can't portably
--     name the login role from inside a migration. It is an OPERATOR
--     PREREQUISITE before flipping `RAMPART_RLS` on — see docs/SETUP.md and
--     docs/MULTITENANCY.md. (Note: a table OWNER is exempt-by-ownership from
--     its own table's policies UNLESS the table is set to FORCE ROW LEVEL
--     SECURITY — slice 7, not built. Until then ownership already exempts the
--     owner; BYPASSRLS makes that robust if ownership ever differs.)
--
-- ── Idempotency / test-cluster safety (CRITICAL) ────────────────────────────
-- `CREATE ROLE` is CLUSTER-GLOBAL, not per-database. This migration runs in
-- every `#[sqlx::test]` temp database, all in the SAME cluster. A bare
-- `CREATE ROLE rampart_app` would succeed in the first temp DB and then fail
-- with SQLSTATE 42710 (role already exists) in the second. So the role is
-- created only if absent. Likewise the membership grant uses the *current*
-- login role (whatever DATABASE_URL connects as), so the migration is
-- login-role-agnostic.

-- Create the application role if it does not already exist (cluster-global).
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'rampart_app') THEN
        CREATE ROLE rampart_app NOLOGIN;
    END IF;
END
$$;

-- Table privileges. Simplest correct grant — the role only *matters* once RLS
-- is forced (slice 7); until then these grants are harmless. Granting ALL
-- tables (vs hand-enumerating) keeps this future-proof and avoids drift.
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO rampart_app;

-- Sequence privileges (CRITICAL): `profiles` and `delivery_log` use BIGSERIAL,
-- whose DEFAULT calls nextval() on an owned sequence. Without USAGE/SELECT/
-- UPDATE on the sequence, an INSERT *as* rampart_app fails permission-denied.
-- Grant ALL sequences rather than enumerating, for the same future-proofing.
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO rampart_app;

-- Future tables/sequences created later inherit the same grants automatically,
-- so a new migration's table is usable by rampart_app without a re-grant.
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO rampart_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO rampart_app;

-- Let the connecting login role assume `rampart_app` (needed for SET ROLE).
-- Done dynamically against current_user so this works no matter which login
-- role runs the migration (prod operator, CI's postgres superuser, etc.).
DO $$
BEGIN
    EXECUTE format('GRANT rampart_app TO %I', current_user);
END
$$;
