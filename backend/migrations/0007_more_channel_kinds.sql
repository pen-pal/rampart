-- Extend channel_kind for the next batch of native adapters.
-- Postgres requires ALTER TYPE ... ADD VALUE outside a transaction
-- block; sqlx runs each statement separately so this just works.

ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'bark';
ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'pushbullet';
ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'sendgrid';
ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'resend';
ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'brevo';
ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'opsgenie';
ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'pagertree';
ALTER TYPE channel_kind ADD VALUE IF NOT EXISTS 'squadcast';
