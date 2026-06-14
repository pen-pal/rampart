-- Continuous profiling tier. One row per ingested profile; `folded` holds the
-- gzipped folded-stack map (stack -> value), the uniform internal form every
-- wire format (folded text, pprof, OTLP profiles) is lowered into before
-- storage. Flamegraphs are assembled on read by merging the folded maps in a
-- time window. See docs/design/PROFILING.md.

CREATE TABLE profiles (
    id           BIGSERIAL   PRIMARY KEY,
    received_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    service_name TEXT        NOT NULL,
    -- cpu | alloc_space | inuse_space | wall | ... (producer-defined)
    profile_type TEXT        NOT NULL,
    period_ns    BIGINT      NOT NULL DEFAULT 0,  -- sampling period
    duration_ns  BIGINT      NOT NULL DEFAULT 0,  -- wall span the profile covers
    sample_count INTEGER     NOT NULL DEFAULT 0,  -- merged sample total
    labels       JSONB       NOT NULL DEFAULT '{}'::jsonb,
    folded       BYTEA       NOT NULL             -- gzipped folded-stack text
);

-- The picker + flamegraph read path: "profiles for (service, type) newest-first
-- in a window".
CREATE INDEX profiles_service_type_time_idx
    ON profiles (service_name, profile_type, received_at DESC);

-- The prune sweep deletes by age across all services.
CREATE INDEX profiles_received_at_idx ON profiles (received_at);
