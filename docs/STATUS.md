# Status: Hermes Beta 0.1 (honest ledger)

**[L]** = run natively on Linux. **[W\*]** = Windows binaries (x86_64-pc-windows-gnu) run under **Wine 9.0**, an
emulator: evidence that the Windows-API code runs, **not** evidence about real Windows 11, real DPAPI, real
NTFS/exFAT/FAT32 or real hardware. **[C]** = compiles only. **Nothing has been run on real Windows hardware.**

## Tested
| What | Evidence |
|---|---|
| 133 automated tests (crypto 36, database 25, tracker engine 25, tray logic 14, viewer 13, CLI 12, collector 5, dump 2, platform 1) | [L] all pass |
| Same suites as Windows binaries, plus the Windows-only DPAPI, single-instance and registry-autostart tests (139 tests) | [W\*] all pass |
| Real Win32 hooks with real windows (foreground, title change, close, session end) and fail-closed indicator | [W\*] 2/2 pass* |
| The real `hermes-tray.exe` driven by its menu commands: pause/resume/exit durable and clean, start-paused records nothing, exit while paused | [W\*] 3/3 pass |
| **Full product flow with the real commands:** `init` (and refusal to overwrite) → `start` while a second process creates windows → `stop` (graceful, via running window) → `status` → `check` (4/4 signed checkpoints) → `search` → `export` (CSV, JSON, refusal to overwrite) → interactive `view` (app filter, summary, find, date, export, verify) → `view` without a key file is refused | [W\*] run by hand, outputs inspected |
| Mutation checks (deliberately break encryption binding, signature checks, exclusions, ghost-focus guard, title rate limit: tests must fail) | [L] each caught |
| Pipeline benchmark: ~110 µs/event (HPKE + signing + SQLite), ~9,000 events/s, 6 MiB peak RSS for 50k synthetic events | [L] synthetic, **not** the Windows collector |

\*Wine does not raise `EVENT_SYSTEM_FOREGROUND` / name-change events itself; that test raises them with `NotifyWinEvent`.
Piping input into `hermes view` through `cmd`'s `|` did not work under Wine (file redirection did); not investigated on real Windows.

## Compiled but never run
The **MSVC build** and the **GitHub Actions workflow** (written carefully, never executed); the PowerShell scripts (never run
in real PowerShell); real DPAPI; NTFS/exFAT/FAT32 behaviour of WAL, fsync and rename; lock/unlock/sleep/resume
notifications; the Ctrl+C handler; UWP app resolution; the `access_denied` path; UAC/secure desktop; real foreground
event generation; the tray icon's actual appearance, overflow-area behaviour and balloons; start-with-Windows at a real
logon; Windows time-zone detection for the viewer's local times (`--tz-offset-minutes` / `--utc` override it).

## Known limitations
* **Not detected:** deletion of the newest records; restoring an older copy of the archive (`docs/ANCHOR.md` is a proposal only).
* Timestamps and activity volume are visible without any key (`docs/METADATA.md`). Window titles can contain secrets; exclusions are by program name only.
* Malware running as your Windows user can use DPAPI and the signing key (`docs/SECURITY.md`).
* No Windows RAM/CPU measurement exists (target 15-30 MB / ceiling 50 MB is unverified): `scripts\measure.ps1`.
* The viewer decrypts the whole archive into memory when it opens (fine for hundreds of thousands of events, slow beyond).
* Terminal viewer only (no GUI). `hermes stop` and the tray's single-instance guard find the collector by its hidden window.

## Not implemented
Legacy-record re-encryption tool (older-format records stay readable and are shown as `LEGACY`; they are unsigned and keep
their plaintext session/kind columns: `docs/MIGRATION.md`); USB checkpoint anchor; typed-text collection; GUI viewer;
installer; idle detection; collector-side USB anchoring.
