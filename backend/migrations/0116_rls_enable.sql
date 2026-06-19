-- ─── Row-Level Security, slice 7 (the enforcement flip): ENABLE RLS ──────────
--
-- Slices 1–6 (migrations 0114/0115 + the rampart-db before_acquire plumbing)
-- created the `rampart_app` role, the `app_current_org()` helper, and the
-- `org_isolation` policies — but left every policy DORMANT (no table had RLS
-- enabled, so Postgres ignored the policies). This migration turns them on.
--
-- ── ENABLE, not FORCE — and why that needs no operator prerequisite ─────────
-- We use `ENABLE ROW LEVEL SECURITY`, NOT `FORCE`. Under ENABLE, the table
-- OWNER is exempt from its own policies (Postgres default). The Rampart app
-- connects as the role that ran these migrations, i.e. the table owner, so:
--
--   * RAMPART_RLS OFF  → the app never `SET ROLE rampart_app`; every checkout
--     stays the owner → exempt → behaviour BYTE-IDENTICAL to before this
--     migration. No blackout is possible with the flag off. (This is why
--     shipping the flip is safe even for installs that never opt in.)
--   * RAMPART_RLS ON, tenant request → before_acquire does `SET ROLE
--     rampart_app` (a NON-owner, non-BYPASSRLS role) + sets app.current_org →
--     the org_isolation policies are ENFORCED for that connection.
--   * RAMPART_RLS ON, system loop (scheduler/prune/notifier/…) → binds no org →
--     `RESET ROLE` leaves it the owner → exempt → full access, for free.
--
-- So tenant isolation is enforced (via the rampart_app role) WITHOUT requiring
-- `ALTER ROLE <db_user> BYPASSRLS` and without the silent-blackout failure mode
-- that FORCE would introduce for the system loops. FORCE would only matter if
-- you wanted the owner role ALSO constrained; tenant requests already run as
-- rampart_app, so it buys no isolation here and is deliberately not used.
-- (Caveat, documented in docs/SETUP.md: this relies on the app login role
-- OWNING the tables — the default, since it runs migrations. A deployment that
-- connects as a NON-owner role must grant that role BYPASSRLS before enabling
-- RAMPART_RLS, or its owner-less checkouts would be policy-subject.)
--
-- ── Enable exactly the policied tables (no drift) ───────────────────────────
-- Derive the target set from the policies created in 0115 rather than
-- re-listing table names: every table carrying the `org_isolation` policy gets
-- RLS enabled, and ONLY those. This guarantees we never enable RLS on a table
-- that lacks a policy (which would default-DENY all rows for rampart_app — a
-- blackout) and never miss a policied table (which would leave it unenforced).
--
-- Reversible: the symmetric DISABLE loop (kept commented for the rollback
-- migration) restores dormancy; `RAMPART_RLS=0` already neutralises enforcement
-- at the connection layer without any schema change.
DO $$
DECLARE
    t text;
BEGIN
    FOR t IN
        SELECT DISTINCT tablename
        FROM pg_policies
        WHERE schemaname = 'public' AND policyname = 'org_isolation'
    LOOP
        EXECUTE format('ALTER TABLE public.%I ENABLE ROW LEVEL SECURITY', t);
    END LOOP;
END
$$;

-- Rollback (a future migration, if ever needed):
--   DO $$ DECLARE t text; BEGIN
--     FOR t IN SELECT DISTINCT tablename FROM pg_policies
--              WHERE schemaname='public' AND policyname='org_isolation'
--     LOOP EXECUTE format('ALTER TABLE public.%I DISABLE ROW LEVEL SECURITY', t); END LOOP;
--   END $$;
