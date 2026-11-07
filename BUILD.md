# Building Hermes

## Option A: GitHub Actions (no local toolchain)
1. Create a **private** GitHub repository and push this folder's contents (the included `.gitignore` keeps keys and
   archives out). `Cargo.lock` is included and used (`--locked`).
2. Open the repository's **Actions** tab. The workflow *Build Hermes (Windows)* runs on every push
   (or press *Run workflow*). It installs stable Rust for `x86_64-pc-windows-msvc`, builds the whole workspace,
   runs the tests, runs a filesystem/DPAPI check and an end-to-end smoke test (these two are informational: they
   need an interactive desktop), and packages the executables.
3. Open the finished run, download the artifact **hermes-beta-0.1-windows-x64** (a ZIP with `hermes.exe`,
   `hermes-tray.exe`, `hermes-collector.exe`, `hermes-keytool.exe`, `hermes-dump.exe`, docs and scripts).

## Option B: build locally
Prerequisites: [Rust](https://rustup.rs) (stable, ≥ 1.88) and **Visual Studio Build Tools 2022** with the
*Desktop development with C++* workload (the bundled SQLite is compiled from C).
```powershell
.\scripts\build.ps1                 # release build -> .\dist\*.exe
# or manually:
cargo build --release --workspace --locked
```

## Verify on your machine
```powershell
.\scripts\test.ps1                  # all tests + DPAPI + real-hook tests + filesystem check + end-to-end smoke test
.\scripts\smoke.ps1 -Seconds 10     # just the end-to-end flow in a temporary folder (switch windows while it records)
```
`test.ps1` writes `hermes-test-report-*.txt` (no archive content). To test with a real USB stick formatted as exFAT/FAT32:
```powershell
.\dist\hermes-keytool.exe fscheck --dir E:\ --local-dir $env:TEMP --events 20000
```

## Troubleshooting
* `link.exe not found` / `cl.exe` errors: install the VS Build Tools workload above and reopen PowerShell.
* `hermes start` says another instance is running: `hermes stop`, or look for the tray icon.
* `hermes view` says *could not unlock*: you need the USB key file **and** the same Windows account that ran `hermes init`.
* The tray icon is not visible: Windows may place it in the `^` overflow; Settings > Personalization > Taskbar.
