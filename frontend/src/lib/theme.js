// Theme runtime. Each view injects its own `.rampart { --bg: ... }` block
// — we override those CSS variables with a higher-specificity selector
// (`[data-theme="dark"] .rampart`) so a single global stylesheet flips the
// entire app without touching per-view CSS.
//
// Storage:
//   localStorage.rampart_theme = "dark" | "light" | "system"  (default "system")
//
// System mode follows `prefers-color-scheme` and reacts to OS changes live.

const STORAGE_KEY = 'rampart_theme';

const DARK_CSS = `
/* Render native controls (select option popups, date pickers, scrollbars)
   in dark too — otherwise a native <select> shows an OS-light dropdown. */
[data-theme="dark"] { color-scheme: dark; }
[data-theme="dark"] .rampart {
  --bg:        #0c0a09;
  --surface:   #1c1917;
  --surface-2: #292524;
  --border:    #292524;
  --border-2:  #44403c;
  --text:      #fafaf9;
  --text-2:    #d6d3d1;
  --text-3:    #78716c;
  --accent:        #2dd4bf;
  --accent-2:      #14b8a6;
  --accent-soft:   #134e4a;
  --up:        #34d399;
  --up-soft:   #064e3b;
  --down:      #f87171;
  --down-soft: #7f1d1d;
}
[data-theme="dark"] .rampart .card,
[data-theme="dark"] .rampart .btn,
[data-theme="dark"] .rampart .input,
[data-theme="dark"] .rampart .select,
[data-theme="dark"] .rampart .search,
[data-theme="dark"] .rampart .kind-card {
  background: var(--surface);
  color: var(--text);
}
[data-theme="dark"] .rampart .input::placeholder,
[data-theme="dark"] .rampart .search::placeholder {
  color: var(--text-3);
}
[data-theme="dark"] .rampart .kbd {
  background: var(--surface-2);
  color: var(--text-3);
  border: 1px solid var(--border-2);
}
[data-theme="dark"] .rampart code {
  background: var(--surface-2);
  color: var(--text);
}
`;

function injectStylesheet() {
  if (document.getElementById('rampart-theme-css')) return;
  const style = document.createElement('style');
  style.id = 'rampart-theme-css';
  style.textContent = DARK_CSS;
  document.head.appendChild(style);
}

function systemPrefersDark() {
  return typeof window !== 'undefined'
    && window.matchMedia
    && window.matchMedia('(prefers-color-scheme: dark)').matches;
}

function resolve(pref) {
  if (pref === 'dark')  return 'dark';
  if (pref === 'light') return 'light';
  return systemPrefersDark() ? 'dark' : 'light';
}

function apply(pref) {
  const effective = resolve(pref);
  document.documentElement.dataset.theme = effective;
}

export function loadTheme() {
  injectStylesheet();
  const pref = localStorage.getItem(STORAGE_KEY) || 'system';
  apply(pref);
  // React to OS changes while pref is "system".
  if (window.matchMedia) {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => {
      const current = localStorage.getItem(STORAGE_KEY) || 'system';
      if (current === 'system') apply('system');
    };
    if (mq.addEventListener) mq.addEventListener('change', onChange);
    else mq.addListener(onChange);
  }
  return pref;
}

export function setTheme(pref) {
  localStorage.setItem(STORAGE_KEY, pref);
  apply(pref);
}

export function getTheme() {
  return localStorage.getItem(STORAGE_KEY) || 'system';
}

export function getResolvedTheme() {
  return resolve(getTheme());
}
