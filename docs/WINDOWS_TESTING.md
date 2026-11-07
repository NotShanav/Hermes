# Testing on real Windows 11

Everything has so far only been run on Linux and under Wine (an emulator). Please run:

```powershell
.\scripts\test.ps1          # tests, DPAPI, real window hooks, NTFS check, end-to-end smoke test -> hermes-test-report-*.txt
```
Keep the console focused during the "real-hook tests" step (about 15 s): it creates and focuses its own windows.

## USB stick (exFAT / FAT32)
Format the stick, then:
```powershell
.\dist\hermes-keytool.exe fscheck --dir E:\ --local-dir $env:TEMP --events 20000
$env:HERMES_TEST_DIR = "E:\hermes-suite"; cargo test -p hermes-crypto -p hermes-db; Remove-Item Env:HERMES_TEST_DIR
```
`fscheck` reports PASS/FAIL for fsync, atomic replace of the key file, DPAPI key set, SQLite WAL, encrypted writes,
integrity and unclean-shutdown recovery on that volume. (The archive lives on the laptop's NTFS drive by default; only
the small key file lives on the stick.)

## RAM and CPU of the collector (requirement: 15-30 MB target, 50 MB ceiling)
No Windows measurement exists yet.
```powershell
.\dist\hermes.exe init --data-dir $env:LOCALAPPDATA\Hermes-test --usb E:\
.\scripts\measure.ps1 -Minutes 30 -DataDir "$env:LOCALAPPDATA\Hermes-test"
```
Use the computer normally; it prints average/peak working set and CPU %.

## Manual end-to-end
See README "Quick start". After `hermes start` for a minute while switching windows and locking the screen (Win+L),
`hermes view` should show your switches, the lock/unlock session boundaries, an excluded password manager as
`<excluded application>`, and `Integrity: CLEAN`.
