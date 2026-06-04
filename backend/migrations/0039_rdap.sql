-- RDAP probe kind (RFC 7480 / 9082).
-- Queries a domain via an RDAP server, asserts a 200 + valid
-- `application/rdap+json` payload, surfaces days-until-expiry when the
-- response carries an `eventAction = "expiration"` entry.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'rdap';
