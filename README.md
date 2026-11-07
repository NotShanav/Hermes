# Hermes Beta 0.1

A **local** record of which applications and window titles you used, and when. Everything is encrypted before it
touches the disk; reading it back needs **two** secrets: one on this laptop (Windows-protected) and one on a USB stick.
No network, no cloud, no AI. It does **not** record keystrokes, clipboard, screen contents or typed text.

## Quick start (Windows 11, PowerShell)
```powershell
# 1. Build (or download the artifact from GitHub Actions: see BUILD.md)
.\scripts\build.ps1

# 2. One-time setup. E:\ is your USB stick. This creates the encrypted archive and the keys.
.\dist\hermes.exe init --usb E:\

# 3. Record (a visible console window; Ctrl+C or `hermes stop` ends it; the USB can stay unplugged)
.\dist\hermes.exe start
#    ...or with a system-tray icon (red = recording, grey = paused) and pause/resume menu:
.\dist\hermes.exe tray

# 4. Read it back (plug the USB stick in)
.\dist\hermes.exe view                      # interactive: date / app / find / summary / verify / export
.\dist\hermes.exe search budget --date today
.\dist\hermes.exe export --out history.csv --date 2025-01-01..2025-01-31   # PLAINTEXT file: protect it
.\dist\hermes.exe check                     # database + signature integrity
.\dist\hermes.exe status                    # no USB needed
.\dist\hermes.exe stop
```
`hermes help` lists every option. `scripts\test.ps1` runs the tests and an end-to-end smoke test on your machine.

## What you must know
* **Keep the USB key file away from the laptop** and keep a second copy. Lose it (or Share A: a Windows reinstall
  or a different Windows account) and the history can **never** be read again. There is no recovery, by design.
* Window titles often contain private text (document names, subjects, URLs). Password managers and credential
  prompts are excluded by default (`hermes start --exclude other.exe` adds more). Exports are plaintext.
* The recorder runs only when you start it, shows a console or tray icon, and has no hidden mode. If the tray icon
  cannot be shown, `hermes-tray` refuses to record. There is no auto-start unless you switch it on in the tray menu.

## What is protected, and what is not
Protected: contents of every record (app, title, session, kind) with HPKE (RFC 9180) to a public key; the
collector holds only the public key and cannot read what it wrote; checkpoints are signed (Ed25519) and verified
with a key from the USB file; tampering, forged and deleted records are detected (`hermes check`).
**Not protected / not detected** (details in `docs/SECURITY.md`): timestamps and activity volume are visible
without any key; deleting the *newest* records or restoring an *older copy* of the archive is **not** detected
(`docs/ANCHOR.md` proposes a fix; not built); malware running as your Windows user can read the DPAPI-protected
share and the signing key; whoever holds both shares reads everything; older-format ("legacy") records stay
readable but are unsigned and `legacy` migration to the new format is **not** implemented.

## Status: tested vs not
See `docs/STATUS.md`. In short: logic is covered by 100+ automated tests on Linux; the Windows binaries were run
under **Wine** (an emulator), including the complete `init → start → stop → view → export` flow. **Nothing has been
run on real Windows 11 hardware yet**; `scripts\test.ps1` is how you do it.

## Layout
`crates/hermes-cli` (the `hermes` command) · `hermes-viewer` (unlock, filter, export) · `hermes-collector`
(storage glue) · `hermes-tracker` (Windows foreground tracking) · `hermes-tray` (tray UI) · `hermes-db`
(encrypted SQLite store) · `hermes-crypto` (keys, HPKE, signatures) · `hermes-platform` (DPAPI) ·
`hermes-keytool` (rotate/backup/restore/fscheck) · `hermes-dump` (developer dump tool).
