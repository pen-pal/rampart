-- More banner-protocol kinds (handled by the same banner probe):
--   ftp  → "220" greeting (port 21)
--   pop3 → "+OK" greeting (port 110)
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'ftp';
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'pop3';
