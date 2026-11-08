# Hermes desktop app

A desktop interface for Hermes: unlock screen, dashboards, timeline, application and window analytics,
search, categories, privacy and recorder controls, integrity check and export. Built with Tauri 2 and React.

It **reads** the archive through the existing `hermes-viewer` crate and **controls** the existing tray
recorder (`hermes-tray.exe`). It records nothing itself, and the database, encryption, key handling and
collection code are unchanged.

## What the app does

| Page | What it shows (all from your real archive) |
|---|---|
| Unlock / Setup | First run: creates keys + archive (`hermes init`). Afterwards: finds `hermes-share-b.key` on any drive, unlocks with both shares, verifies signatures. |
| Overview | Day ribbon (colour by app or category), active time, focus periods, longest focus, hour-of-day chart, category split, top apps, top windows, recorder status, recent focus. Deltas vs the previous period. |
| Timeline | One day, per-application lanes or a ribbon, session list, click for details. |
| Applications | Sortable table; per-app page with per-day chart, hour profile, weekday x hour heat grid, window titles, recent sessions. |
| Pages & windows | Time per window title, browser pages cleaned of the browser suffix. Titles only: Hermes never records URLs. |
| Insights | 53-week calendar heatmap, streaks, period comparisons (today/yesterday, week, 7 days, month), movers, weekly rhythm. |
| Search & explore | Every event with text / application / event-type / date filters, detail drawer, export of the filtered results. |
| Categories & rules | Built-in categories, custom categories, rules by program name. Display-only: the archive is never modified. |
| Privacy & recording | Start / pause / resume / stop / restart the recorder, live state, exclusions, start-up options, honest notes on retention. |
| Archive & export | Size, events, shutdown state, integrity report, re-verify, key sets, CSV/JSON export with a plaintext warning. |
| Settings | Theme (dark default, light, system), time zone, auto-lock. |

## Deliberately not in the app

* **Keyboard activity.** Hermes does not record keystrokes (see the main README), so there is no such page.
* **Web addresses.** Only window titles exist in the data.
* **Idle time.** Not recorded by Hermes yet, so not charted. Gaps on the timeline mean "not recording".
* **Delete / retention controls.** The archive is append-only and signed; deleting rows would read as
  tampering. The Privacy page says so and points to the data folder.

## Small changes to existing code

* `crates/hermes-tray/src/live.rs` (new) and two small hooks in `ui.rs`: the tray writes
  `<data-dir>/collector-state.json` (state name + counters, never content) so the app can tell *paused* from
  *recording*. Nothing else about recording changed.
* `.github/workflows/build.yml`: builds the app and ships it as `hermes-viewer.exe`, the name the tray menu's
  "Open viewer" already looks for.

## Build

The app is its **own Cargo workspace** (`app/src-tauri`), so the main workspace and its `--locked` build are
untouched. `app/src-tauri/Cargo.lock` was seeded from the main lockfile; Cargo adds the Tauri crates on first build.

Requirements: Windows, Rust (MSVC), Node 20+, WebView2 (included with Windows 11).

```powershell
cd app
npm install
npx tauri build --no-bundle        # -> src-tauri\target\release\hermes-app.exe
# development with hot reload:
npx tauri dev -- -- --data-dir C:\path\to\data
```

Keep `hermes-app.exe` (or `hermes-viewer.exe`) in the same folder as `hermes-tray.exe`: the app starts the
recorder from there. Optional: `--data-dir DIR` like the CLI.

## Status

Written without a compiler in the loop. The first GitHub Actions run is the first real build, so expect to
fix a few compile errors. `cargo test --manifest-path app/src-tauri/Cargo.toml` covers the segment builder,
aggregations, time slicing and category rules.
