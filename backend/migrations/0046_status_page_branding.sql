-- Status-page branding: custom domain + logo.
--
-- Two operator-facing knobs on a status page:
--
--   custom_domain  the hostname an operator points at the Rampart host
--                  (e.g. "status.acme.com"). Routing itself is left to the
--                  operator's reverse proxy / CNAME; we only store the
--                  canonical hostname so the public view can advertise it.
--                  A partial UNIQUE index keeps two pages from claiming the
--                  same domain while still allowing many pages with no
--                  custom domain (NULL) to coexist.
--
--   logo_url       either an external URL or a `data:` URI for an uploaded
--                  logo (the builder reads a picked image via FileReader and
--                  stores the base64 data URI inline). Nullable.
--
-- Both columns are nullable; existing pages keep working untouched.

ALTER TABLE status_pages ADD COLUMN IF NOT EXISTS custom_domain TEXT;
ALTER TABLE status_pages ADD COLUMN IF NOT EXISTS logo_url      TEXT;

-- One page per custom domain. Partial so the many NULL rows don't collide.
CREATE UNIQUE INDEX IF NOT EXISTS status_pages_custom_domain_uidx
    ON status_pages (custom_domain)
    WHERE custom_domain IS NOT NULL;
