# frontend/HACKING.md

React/Vite-specific conventions. Read this before touching code in `frontend/src/`. The top-level [`README`](../README.md) covers how to run.

## Stack

- **Vite 5** + React 18, JSX
- **lucide-react** for icons
- **recharts** for charts
- **No Tailwind, no CSS modules, no styled-components.** Inline CSS-in-JS via `<style>{css}</style>` at the top of each view. Each view is self-contained on purpose — easier for designers to iterate without grokking a framework.

## Design tokens (the operator UI)

All four views use the same token set, declared inline at the top of each. **Don't drift these.** If you change them, change them everywhere consistently.

```css
--bg:        #fafaf9    /* warm off-white page background */
--surface:   #ffffff    /* card backgrounds */
--surface-2: #f5f5f4    /* subtle hover / muted areas */
--border:    #e7e5e4    /* hairline borders */
--border-2:  #d6d3d1    /* slightly darker for emphasis */

--text:   #1c1917       /* primary text — warm near-black */
--text-2: #57534e       /* secondary */
--text-3: #a8a29e       /* tertiary, labels */

--accent:      #14b8a6  /* teal — the brand color, used sparingly */
--accent-2:    #0d9488  /* hover state */
--accent-soft: #ccfbf1  /* tinted backgrounds */

/* status semantics — used on dots, pills, bars */
--up:    #10b981   --up-soft:    #d1fae5
--down:  #ef4444   --down-soft:  #fee2e2
--warn:  #f59e0b   --warn-soft:  #fef3c7
--maint: #6366f1   --maint-soft: #e0e7ff
```

- **Font:** Inter (sans), JetBrains Mono (data/timestamps/codes). Both loaded via Google Fonts `@import` inside the inline `<style>` tag.
- **Border radius:** 12px on cards, 8px on inputs/buttons, 999px on pills, 6-7px on smaller elements.
- **Numbers** always get the `.tabular` class (CSS `font-variant-numeric: tabular-nums`) so digits don't shift width.
- **Mono codes / IDs / hostnames** always get the `.mono` class.

## The status page is intentionally different

`StatusPageBuilder.jsx` shows a **public-facing preview** that uses a different aesthetic on purpose:

- **Instrument Serif** (italic capable) for headlines — warmer, editorial feel
- Larger type, more whitespace
- Same teal accent but in a lighter, more spacious layout

This is deliberate. The operator UI is utilitarian; the public face is brand-y. Don't homogenize.

## File map

```
src/
├── main.jsx                  React bootstrap (DO NOT TOUCH for routing)
├── App.jsx                   Hash router + floating dev view-switcher
└── views/
    ├── Dashboard.jsx         Main monitor list + KPIs + response trend chart
    ├── MonitorDetail.jsx     Single-monitor drill-down: 90-day bars, heartbeats log
    ├── StatusPageBuilder.jsx Split editor (380px) + live public preview
    └── NewMonitorWizard.jsx  3-step wizard, all 20 monitor types
```

## Where mock data lives (and what to replace)

When wiring to the backend, the mock data declarations are all at the top of each view file with the comment `// ─── seed data`. Each one maps to a specific API call:

### `Dashboard.jsx`
- `monitors` array → `GET /v1/monitors`
- `counts` object → derived from monitors response
- `trendData` → needs new endpoint, e.g. `GET /v1/monitors/_trend?top=4&window=24h`
- `recentIncidents` → `GET /v1/incidents?active=true&limit=10`
- `upcoming` → `GET /v1/maintenance?upcoming=true&limit=10`
- `historyFor(id, status)` → either `GET /v1/monitors/:id/heartbeats?n=60` per row, OR a batch endpoint

### `MonitorDetail.jsx`
- `responseData` → `GET /v1/monitors/:id/heartbeats?window=24h&bucket=10m`
- `uptime90` → `GET /v1/monitors/:id/uptime?window=90d&bucket=1d`
- `heartbeatLog` → `GET /v1/monitors/:id/heartbeats?limit=50`
- `recentDowntime` → derived from heartbeats where `important=true AND status='down'`
- Cert info → need a `GET /v1/monitors/:id/cert` endpoint

### `StatusPageBuilder.jsx`
- `groups` → `GET /v1/status-pages/:id/groups` (with nested components)
- All form fields → `PUT /v1/status-pages/:id` on save

### `NewMonitorWizard.jsx`
- `types` array stays in code — it's the static catalog of supported kinds
- Form submission → `POST /v1/monitors` (DTO matches `NewMonitor` in `rampart-core`)
- Live test button → need `POST /v1/monitors/_test` (probe without persisting)

## Vite proxy

Already configured in `vite.config.js`:

```js
proxy: {
  '/v1':      'http://localhost:3000',
  '/healthz': 'http://localhost:3000',
  // ...
}
```

So `fetch('/v1/monitors')` from any view just works in dev. No CORS headers needed.

For production, `npm run build` produces `dist/`, which the `rampart-api` binary picks up via `rust-embed` (see `backend/crates/rampart-api/src/static_assets.rs`). Debug builds read `frontend/dist/` from disk at request time — so editing JSX, running `npm run build`, and refreshing the browser is enough during dev; no Rust rebuild needed. Release builds bake the bundle into the executable so `rampart-api` ships as a single file with no asset paths to wire up. Unknown paths fall back to `index.html` for SPA routing; if the bundle is missing the binary logs a warning at startup and returns 404 for non-API routes.

## Common patterns to reuse

Each view defines roughly the same shape of utility components:

- A `Kpi` component for the small stat tiles (label / big number / sub-text)
- A `pill` className family (`pill-up`, `pill-down`, `pill-warn`, `pill-maint`)
- A `dot` className with glowing variants
- A `tabs` className for the segmented switcher

If you find yourself rewriting these in a new view, lift them to a shared `components/` directory **only when** they're genuinely identical across 3+ views. Premature abstraction is worse than duplication.

## What about state management?

There isn't any yet. When wiring to the backend, start with plain `useState` + `useEffect` per view. If component state gets unwieldy:

- For data fetching → `@tanstack/react-query` (cache, refetch, error states)
- For client state → keep using `useState` until it actually hurts. Don't reach for Redux/Zustand prematurely.

## Adding a new view

1. Create `src/views/MyView.jsx` — copy the structure of an existing one (the design system imports, the `Kpi`-style helpers, the main component).
2. Add it to the `VIEWS` array in `App.jsx`:
   ```js
   { hash: '#/my-view', label: 'My view', component: MyView },
   ```
3. The floating switcher picks it up automatically. Long-term, this switcher is dev-only and should be replaced with real top-nav.

## Don't reach for these

- **Tailwind** — we use inline CSS-in-JS; the views are designed to be self-contained.
- **A UI library (Radix, shadcn, Mantine, etc.)** — the design is custom; component libs would constrain the aesthetic.
- **CSS-in-JS runtime libraries (Emotion, styled-components)** — the inline `<style>` approach is intentional, zero-runtime, and matches what designers can read.
- **React Router** — a hash router suffices until there's a reason for more. URL state is small.
- **TypeScript** — not yet. If you're adding it, do the whole project in one go, not piecemeal.
