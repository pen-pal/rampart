-- Per-page custom CSS.
--
--   custom_css  operator-authored CSS injected into a <style> tag AFTER
--               the built-in stylesheet on the public page, so operators
--               can override the default theme. Nullable; capped at 64 KB
--               at the API validation layer. The frontend strips any
--               </style> / <script close-out before injecting it so the
--               operator can't break out of the style context.
--
-- Nullable; existing pages keep working untouched.

ALTER TABLE status_pages ADD COLUMN IF NOT EXISTS custom_css TEXT;
