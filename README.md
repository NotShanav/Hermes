<div align="center">

<img src="app/src/assets/logo.png" alt="Hermes" width="120" />

# Hermes

**A private, local, encrypted record of which apps and windows you used, and when.**
Two keys to read it. Nothing leaves your computer.

`v1.3.0` · Windows 10/11 · Rust + Tauri + React · by **NotShanav**

</div>

---

Hermes records the **application** and **window title** in front of you and the time you spent there. Every record is
encrypted before it touches the disk. Reading the history back needs **two** secrets: one tied to your Windows account
and one on a **USB drive**. There is no network code, no cloud and no AI.

> **Private project.** Hermes is built for personal use on the author's own computer. It is not a public release and
> comes with no warranty.

## What it does and does not record

| Recorded | Never recorded |
|---|---|
| Name and path of the foreground application | Keystrokes or typed text |
| Title of its window, as the app shows it | Clipboard contents |
| When focus starts and ends, and title changes | Screen contents or screenshots |
| Lock, sleep, pause and log-off boundaries | Web addresses or page content |
| Process id and window class | Anything sent over a network |

Password managers and Windows credential prompts are excluded by default (their time is stored as a private block
with no name or title). You can exclude more programs. Window titles can still contain private text such as document
names, so treat the archive and any export accordingly.

## Features

**Desktop app** (`hermes-viewer.exe`)

| Page | What you get |
|---|---|
| **Unlock** | Detects the USB key file, unlocks, verifies signatures. First run creates the archive and keys. Locks itself after inactivity and **the moment the USB drive is removed**. |
| **Overview** | A zoomable day ribbon, active time, focus periods, longest focus, hourly chart, category split, top apps and windows, recorder status, and change versus the previous period. |
| **Timeline** | One day as per-application lanes or a ribbon. Scroll to zoom, drag to move, minimap, down to seconds. |
| **Applications** | Sortable table and a detail page per app (per-day chart, hour profile, weekday-by-hour grid, window titles, sessions). |
| **Pages & windows** | Time per window title, with browser names cleaned up. Titles only, never web addresses. |
| **Insights** | 53-week calendar heatmap, streaks, period comparisons, biggest movers, weekly rhythm. |
| **Search & explore** | Every event, filtered by text, application, event type and date, with a detail drawer and export. |
| **Categories & rules** | Built-in and custom categories, rules by program name. Display-only: the archive is never modified. |
| **Privacy & recording** | Start, pause, resume, stop and restart the recorder, exclusions, start-up options. |
| **Archive & export** | Integrity report with re-check, key sets, CSV/JSON export (plain text, with a warning). |
| **Settings** | Theme (dark, light, system), interface style, time zone, auto-lock. |

**Two interface styles** (Settings → Interface style)

* **Modern** — clean solid panels (default).
* **Fluidity** — liquid glass: a living colour-blob backdrop, frosted translucent surfaces with gradient rims, real lens
  refraction on small controls, and fluid motion. *Full* or *Subtle* intensity. Respects Windows' reduced-motion and
  reduced-transparency settings.

**Recorder** (`hermes-tray.exe`) — a visible tray icon and an on-screen dot while recording. There is no hidden mode:
if the indicator cannot be shown, it refuses to record. Its tray menu opens with a coloured status line (click it for
details), groups the rest into *Pause for…* and *Settings* submenus, follows the Windows light/dark theme, and is drawn
at full resolution on scaled displays.

**Command line** (`hermes.exe`) — `init`, `start`, `tray`, `view`, `search`, `export`, `check`, `status`, `stop`.

## Getting started

### 1. Get the build

* **Releases** — open the *Releases* page and download `Hermes-vX.Y.Z-windows-x64.zip`, or
* **Actions** — open the latest successful run and download the artifact named `Hermes-vX.Y.Z-windows-x64`.

Unzip it somewhere permanent. **Keep every `.exe` in the same folder**: the app starts the recorder from there.

### 2. First run

1. Plug in the USB drive that will hold the second key.
2. Run `hermes-viewer.exe`. On first run it shows **Set up Hermes**: choose the USB drive and create the archive.
3. **Copy the key file** (`hermes-share-b.key`) to a second USB drive straight away and keep both away from this computer.
4. Press **Start recording** (Privacy & recording, or the lock screen). The tray icon appears.

Data lives in `%LOCALAPPDATA%\Hermes`. Use `--data-dir <folder>` to choose another location.

### 3. Every day

Recording runs on its own and **does not need the USB drive**. To look at your history, plug the USB in, open the app and
press **Unlock**. Unplugging the drive locks the app again and clears the history from memory.

## Security model

| | |
|---|---|
| Record encryption | HPKE (RFC 9180): X25519 key exchange, HKDF-SHA256, ChaCha20-Poly1305. A fresh key for every record; payloads padded to 256-byte buckets. |
| Key protection | One private key split in two (Shamir 2-of-2). **Share A** is wrapped with Windows DPAPI on the laptop; **Share B** is the key file on the USB drive. Either alone reveals nothing. |
| Integrity | SHA-256 hash chain plus Ed25519 signed checkpoints, verified with a key held on the USB. Detects edited, inserted, reordered and mid-history deleted records. |
| What the recorder holds | Only the public key and its signing key. It cannot read what it wrote. |

**Visible without any key:** timestamps, sequence numbers, record-size buckets and public keys. Someone with your disk can
tell *when* the computer was used, not *what* was used.

**Not protected or not detected:**

* Deleting only the **newest** records, or restoring an **older copy** of the whole archive, is not detected.
* Malware running as your Windows user can use DPAPI like Hermes does.
* Whoever holds both keys reads everything. Exports are **plain text**.
* **Lose the USB key file** (and have no copy), or reinstall Windows without a Share A backup, and that history is
  **gone for good**. There is no recovery or backdoor.

More detail: [`docs/SECURITY.md`](docs/SECURITY.md), [`docs/KEY_MANAGEMENT.md`](docs/KEY_MANAGEMENT.md),
[`docs/METADATA.md`](docs/METADATA.md).

## Building from source

GitHub Actions does everything on every push (`.github/workflows/build.yml`).

To build locally on Windows you need Rust (MSVC), Node 20+ and WebView2 (included with Windows 11):

```powershell
# core: recorder, tray, command line (see BUILD.md)
cargo build --release --workspace --locked

# desktop app
cd app
npm install
npx tauri build --no-bundle      # -> app\src-tauri\target\release\hermes-app.exe
```

The desktop app is its **own Cargo workspace** (`app/src-tauri`), so the core build and its `--locked` lockfile are
unaffected. In a package, `hermes-app.exe` is named `hermes-viewer.exe`, the name the tray menu launches.

Details for the app: [`app/README.md`](app/README.md).

## Versions and releases

Package names follow the app version: **`Hermes-v<major>.<minor>.<patch>-windows-x64`**.

| Part | Meaning |
|---|---|
| Major | A big change to the product |
| Minor | New features that keep old behaviour |
| Patch | Bug fixes only |

The version lives in three files that **must match** (the build fails if they do not):
`app/package.json`, `app/src-tauri/Cargo.toml`, `app/src-tauri/tauri.conf.json`. The core crates keep their own numbers.

To publish a release:

1. Change the version in those three files and describe the change in `VERSION.txt`.
2. Commit and push. Actions builds the `Hermes-vX.Y.Z-windows-x64` artifact.
3. For a downloadable Release, tag it: `git tag vX.Y.Z` then `git push origin vX.Y.Z`. The tag must equal the app version.

## Repository layout

```
.github/workflows/   CI: build, test, package (name comes from the app version)
app/                 Desktop app (Tauri 2 + React): src/ (UI), src-tauri/ (Rust back end)
crates/
  hermes-cli         the `hermes` command
  hermes-tray        visible recorder: tray icon, on-screen dot, settings
  hermes-collector   turns events into encrypted rows
  hermes-tracker     Windows foreground-window tracking
  hermes-viewer      unlock, decrypt, verify, filter, export
  hermes-db          encrypted SQLite storage
  hermes-crypto      keys, HPKE, Shamir, signatures
  hermes-platform    Windows DPAPI
  hermes-keytool     key rotation, Share A backup / restore, filesystem checks
  hermes-dump        developer diagnostic tool
docs/                security, key management, metadata, status notes
scripts/             build, test, smoke and measurement scripts
VERSION.txt          what changed in this version
```

## Status

The core has 100+ automated tests and the app builds and runs on Windows 11. Some real-hardware behaviours of the core
(DPAPI edge cases, removable-drive journaling, sleep and resume) are still listed as unverified in
[`docs/STATUS.md`](docs/STATUS.md). Known limitations:

* Idle time is not recorded, so gaps on the timeline mean "not recording".
* The whole archive is decrypted into memory when you unlock: fine for hundreds of thousands of events, slower for
  multi-year archives.
* No installer yet; the recorder and app must share a folder.
* Key rotation creates a new key set for new records; it does not re-encrypt old history.

## Author

**NotShanav**
