-- Banner-protocol monitor kinds. Each connects over TCP and validates
-- the server's greeting line:
--   ssh  → expects a line starting with "SSH-"
--   smtp → expects a "220" greeting
--   imap → expects "* OK"
-- A config.expect string overrides the default prefix per monitor.
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'ssh';
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'smtp';
ALTER TYPE monitor_kind ADD VALUE IF NOT EXISTS 'imap';
