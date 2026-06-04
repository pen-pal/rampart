-- DNS-over-HTTPS probe kind (RFC 8484, JSON variant).
-- GETs the DoH endpoint with `?name=<query>&type=<rtype>` and asserts
-- the response indicates NOERROR with a non-empty answer.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'doh';
