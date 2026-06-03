# Screenshots

This directory holds the step-by-step walkthrough screenshots referenced from [`docs/WALKTHROUGH.md`](../../WALKTHROUGH.md) and the README hero shots (`docs/assets/dashboard.png` + `docs/assets/dashboard-dark.png`).

**They are generated, not hand-shot.** Re-running the generator on every UI change keeps the screenshots in lockstep with the actual product — no more drift between the docs and what the user actually sees.

## Regenerate

From a clean checkout, with Docker available (Postgres comes from `docker compose`):

```bash
cd backend && cargo build -p rampart-api        # one-time + when api changes
cd ../frontend
npm ci                                          # one-time
npm run build                                   # embed assets the api will serve
npx playwright install --with-deps chromium     # one-time
npm run screenshots                             # 60–90s, writes PNGs into this dir
```

The script:

- Resets and migrates the `rampart_test` database (via `e2e/start-api.sh`).
- Brings up `rampart-api` on `:3001`.
- Drives the first-run journey through Playwright in Chromium at 1440×900 with `deviceScaleFactor=2` (so the PNGs are sharp on retina renders).
- Writes 11 numbered PNGs into `docs/assets/screenshots/` plus the two README hero shots (`docs/assets/dashboard.png`, `docs/assets/dashboard-dark.png`).

Commit the result. The CI matrix does **not** run this spec — it's explicitly excluded via `testIgnore` in `playwright.config.js` and re-enabled only when `SCREENSHOTS_RUN=1` is set by the npm script.

## Inventory

| File                              | Surface                                                        |
| :-------------------------------- | :------------------------------------------------------------- |
| `01-setup.png`                    | First-run admin creation form                                  |
| `02-login.png`                    | Returning-user sign-in screen                                  |
| `03-dashboard-empty.png`          | Dashboard before any monitors exist                            |
| `04-wizard-kind.png`              | New-monitor wizard, step 1 (pick a probe type)                 |
| `05-wizard-target.png`            | New-monitor wizard, step 2 (target URL + name)                 |
| `06-wizard-schedule.png`          | New-monitor wizard, step 3 (cadence + thresholds)              |
| `07-monitor-detail.png`           | Monitor detail view after a few heartbeats land                |
| `08-dashboard-populated.png`      | Dashboard with the demo monitor in the list                    |
| `09-notifications.png`            | Notification channels list with one webhook channel attached   |
| `10-status-pages.png`             | Status page builder view                                       |
| `11-dashboard-dark.png`           | Dashboard in dark theme                                        |

## Updating the walkthrough copy

The walkthrough text lives in [`docs/WALKTHROUGH.md`](../../WALKTHROUGH.md). When you change the UI in a way that invalidates a step description (new field, removed pane, renamed button), update both the screenshot (re-run the generator) **and** the prose in `WALKTHROUGH.md` in the same PR.
