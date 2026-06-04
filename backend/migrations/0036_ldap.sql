-- LDAP probe kind.
-- Connects to a `ldap://host:port` (or `ldaps://`) directory and runs
-- a simple bind. Optional `bind_dn` + `bind_password` config switches
-- the probe from anonymous to authenticated bind.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'ldap';
