-- Switch the high-volume text/JSONB telemetry columns from Postgres' default
-- pglz TOAST compression to lz4 (PG14+; faster, better ratio). This only
-- affects values written AFTER the change — existing rows keep their current
-- compression until rewritten — so it shrinks the table gradually as new
-- telemetry lands and old rows prune out.
--
-- Wrapped per-column in an exception-swallowing block: if this server's
-- Postgres was built without lz4, the column simply keeps pglz and startup is
-- never blocked.
DO $$
DECLARE
    cmd text;
    cols text[][] := ARRAY[
        ['logs', 'body'],
        ['logs', 'attributes'],
        ['spans', 'attributes'],
        ['error_events', 'context'],
        ['error_events', 'stacktrace']
    ];
    i int;
BEGIN
    FOR i IN 1 .. array_length(cols, 1) LOOP
        cmd := format('ALTER TABLE %I ALTER COLUMN %I SET COMPRESSION lz4',
                      cols[i][1], cols[i][2]);
        BEGIN
            EXECUTE cmd;
        EXCEPTION WHEN others THEN
            RAISE NOTICE 'skipped %: %', cmd, SQLERRM;
        END;
    END LOOP;
END $$;
