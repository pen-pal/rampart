// Spanish stub — re-exports the English dictionary so every key resolves
// to the source string until a translator fills this file in. Keeping the
// stubs as explicit re-exports (rather than `null` + fallback chains in
// t()) means we can grep `import './i18n/es.js'` to find untranslated
// locales, and lazy-loading them via dynamic import still works the same.
export { default } from './en.js';
