-- Password-protected (private) status pages.
--
--   password_hash  an Argon2id PHC string hashed in the db layer from a
--                  plaintext password the operator sets in the builder.
--                  NULL means the page is public (the default — existing
--                  pages keep working untouched). A non-NULL value flips
--                  the page private: the unauthenticated public view then
--                  returns only a locked stub until a visitor proves the
--                  password through the `/unlock` endpoint.
--
-- The hash itself is NEVER serialized out of the API — the public/admin
-- projections expose only a derived read-only `private` boolean.

ALTER TABLE status_pages ADD COLUMN IF NOT EXISTS password_hash TEXT;
