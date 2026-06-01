-- NTP probe — SNTPv4 over UDP. Sends a minimal client packet and asserts
-- the server replies with a valid response.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'ntp';
