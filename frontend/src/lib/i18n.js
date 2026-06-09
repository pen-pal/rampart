// Tiny in-house i18n layer.
//
// Why not react-intl / i18next? Both ship 20-40kB of runtime + lifecycle
// machinery (ICU pluralization, context providers, hot-reload of message
// bundles, plural-form CLDR tables). For Rampart's needs — flat strings,
// `{name}` interpolation, a handful of locales — that's ~5% of the
// frontend bundle for ~0 features we use. This file is ~30 lines and
// covers everything we need today.
//
// Design notes:
//   - All dictionaries are imported up-front. We ship ~6 locales totalling
//     a few KB; lazy-loading would save almost nothing and add a render
//     gate. Move to dynamic import() if/when bundles grow non-trivial.
//   - Locale lookup misses fall back to the English source string, so a
//     freshly-added key works in every locale without a translator
//     round-trip. The miss is silent in prod and console.warn'd in dev so
//     CI can surface untranslated keys eventually.
//   - setLocale() persists then reloads — there is no live re-render on
//     change. That keeps the layer 100% synchronous (no context, no
//     subscriptions) and matches how the theme toggle's "system" mode
//     already triggers a reload when the OS theme flips.

import en from './i18n/en.js';
import es from './i18n/es.js';
import fr from './i18n/fr.js';
import de from './i18n/de.js';
import ja from './i18n/ja.js';
import zh from './i18n/zh.js';

export const SUPPORTED = ['en', 'es', 'fr', 'de', 'ja', 'zh'];

// Map locale code -> dictionary. Stub locales re-export `en` so missing
// keys silently fall through to English (the stub IS English right now,
// but the indirection lets translators replace each file in isolation
// without touching this resolver).
const DICTS = { en, es, fr, de, ja, zh };

const STORAGE_KEY = 'rampart_locale';

/** Read the persisted locale, falling back to the browser default, then "en". */
export function getLocale() {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && SUPPORTED.includes(stored)) return stored;
  } catch { /* localStorage disabled (private mode, etc.) — fall through */ }
  // navigator.language is "en-US" / "fr-CA" / "zh-Hans-CN" — we just want
  // the primary subtag. Lowercased to match SUPPORTED.
  const nav = (typeof navigator !== 'undefined' && navigator.language) || 'en';
  const primary = nav.toLowerCase().slice(0, 2);
  return SUPPORTED.includes(primary) ? primary : 'en';
}

/** Persist a new locale. Caller is responsible for reloading the page. */
export function setLocale(code) {
  if (!SUPPORTED.includes(code)) return;
  try { localStorage.setItem(STORAGE_KEY, code); } catch { /* ignore quota */ }
}

/**
 * Look up `key` in the active dictionary, then interpolate `{name}`
 * placeholders from `vars`.
 *
 *   t('common.save')                      // → "Save"
 *   t('hello.greeting', { name: 'Ada' })  // → "Hello, Ada"
 *
 * Misses return the raw key string — louder than empty output, and easy
 * to grep for in screenshots ("dashboard.something") when QA spots one.
 */
export function t(key, vars) {
  const dict = DICTS[getLocale()] || DICTS.en;
  // Two-step lookup: active locale, then English. Stubs already re-export
  // English so step 2 is a no-op today, but real translation files will
  // be partial — the fallback keeps the UI usable mid-translation.
  let str = dict[key];
  if (str == null) str = DICTS.en[key];
  if (str == null) {
    // Loud in dev, silent in prod. import.meta.env.DEV is the Vite flag.
    if (typeof import.meta !== 'undefined' && import.meta.env?.DEV) {
      // eslint-disable-next-line no-console
      console.warn(`[i18n] missing key: ${key}`);
    }
    return key;
  }
  if (!vars) return str;
  // Match `{name}` — no nested braces, no escape syntax. If a string ever
  // needs a literal `{`, we'll add an escape; deliberately not now.
  return str.replace(/\{(\w+)\}/g, (_, k) => (vars[k] != null ? String(vars[k]) : `{${k}}`));
}
