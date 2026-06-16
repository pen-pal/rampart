-- Optional user identity on RUM beacons, so the RUM view can answer
-- "who experienced this page load". The browser snippet sets it best-effort
-- from `window.__rampartUser` (the app assigns it after login). Nullable —
-- anonymous traffic just has no user_id.
ALTER TABLE rum_events ADD COLUMN user_id TEXT NULL;

CREATE INDEX rum_user_idx ON rum_events (user_id) WHERE user_id IS NOT NULL;
