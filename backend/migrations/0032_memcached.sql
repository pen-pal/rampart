-- Add memcached to the monitor_kind enum. The probe is a banner-style
-- text-protocol check: TCP connect to host:port, send "version\r\n",
-- expect "VERSION " (or a configurable override) at the start of the
-- server's response.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'memcached';
