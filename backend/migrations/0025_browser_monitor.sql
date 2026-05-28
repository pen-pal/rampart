-- Headless-browser-rendered monitor kind.
-- Calls an external rendering service (browserless/chrome compatible)
-- to get JS-rendered HTML, then runs a keyword assertion on the result.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'browser';
