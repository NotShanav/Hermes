# Hermes desktop app

Author: NotShanav

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

## Clean slate / reset (hidden on purpose)

For the case where a key is lost for good. It is **not visible anywhere on the lock screen**.

**How to open it:** on the lock screen, press and drag the Hermes logo (top left) to the right. It slides about 100 px
and reveals a keyhole where it was. Click the keyhole. (If left alone for 7 seconds the logo slides back.) It only
appears when an archive already exists.

**What it does:**
1. Asks you to tick an acknowledgement and type `RESET`.
2. Walks you through plugging in a USB drive and choosing it. A new key file `hermes-share-b.key` is written there.
3. Stops the recorder, moves the old `hermes.db*` and `keys\` to `<data-dir>\reset-<time>\`, runs the normal `init`
   (new archive, new key set, new signing key), and, if anything fails, puts everything back.
4. Optional: "also delete the old encrypted files" removes the `reset-...` folder and any renamed old key file.
5. Finally it starts the recorder and unlocks with the new key.

It never decrypts or reads the old history, and it never overwrites an old key file: an old file on the chosen drive is
renamed (`hermes-share-b.key.old-<time>`) unless you chose to delete the old data. Old data is unreadable afterwards
unless you still have its keys. Your app preferences, category rules and recorder settings are kept.

## Interface styles: Modern and Fluidity

Settings, Interface style. **Modern** is the original solid look and is the default. **Fluidity** adds:

* a living backdrop of slowly drifting colour blobs (pure CSS, no WebGL, no extra packages);
* frosted-glass chrome: sidebar, top bar, popovers, drawer, modal, toast and the lock screen;
* near-opaque data cards, so charts and category colours keep their contrast;
* liquid-glass accents with real refraction (an in-house SVG displacement filter whose map is a `data:` URL): the
  active-nav pill, the segmented-control thumb, the recording pill, the date-range button and the Unlock button;
* fluid motion: the nav pill and segmented thumb glide between options, pages lift in, stat values roll, the drawer
  and dialogs spring in, and the Unlock button leans toward the cursor.

**Intensity.** *Full* uses five blobs, liquid refraction and the lock-screen blob halo. *Subtle* keeps frosted glass and two
quiet blobs only. Animation pauses when the window is hidden or unfocused and while the archive is locked.
Windows' reduced-transparency setting turns glass into opaque panels. Nothing in Fluidity makes a network request,
and the CSP is unchanged. The preferences are stored as `ui_style` and `effects` in `hermes-app.json`; older files
without them load as Modern / Full.

Design choices that differ from the research brief: no new npm packages (blobs, glass, refraction and motion are
written in-house), and a self-contained violet backdrop instead of Windows Mica, so it looks the same on every PC.

## Day ribbon and timeline lanes: zoom

On the Overview ribbon and the Timeline lanes: scroll to zoom (the instant under the cursor stays put), drag to move,
double-click to reset, or use the buttons: zoom out, zoom in, *fit to the active hours*, *whole day*. A minimap shows
where you are. Down to 15 seconds across the full width; tick labels switch to seconds. Lanes now show application names.
