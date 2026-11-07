<#
 Runs the automated tests, then the end-to-end smoke test, on this Windows machine.  Usage:  .\scripts\test.ps1
 Writes hermes-test-report-<time>.txt (no archive content) that you can share.
#>
$ErrorActionPreference = "Continue"
Set-Location (Split-Path -Parent $PSScriptRoot)
$report = "hermes-test-report-$(Get-Date -Format yyyyMMdd-HHmmss).txt"
function Step($t, [scriptblock]$b) { "`n=== $t" | Tee-Object $report -Append; & $b 2>&1 | Tee-Object $report -Append; "--- exit $LASTEXITCODE" | Tee-Object $report -Append }
Step "toolchain" { rustc --version; cargo --version; (Get-CimInstance Win32_OperatingSystem).Caption }
Step "cargo test --workspace" { cargo test --workspace --locked }
Step "DPAPI tests" { cargo test -p hermes-platform --locked -- --nocapture }
Step "real-hook tests (keep this window focused for ~15 s; don't click away)" { cargo test -p hermes-tracker --test win_smoke --locked -- --ignored --test-threads=1 --nocapture }
Step "filesystem + DPAPI check (NTFS)" { New-Item -ItemType Directory -Force -Path "$env:TEMP\hermes-fs" | Out-Null; cargo run --release --locked -q -p hermes-keytool -- fscheck --dir "$env:TEMP\hermes-fs" --local-dir $env:TEMP --events 20000 }
Step "end-to-end smoke test" { .\scripts\smoke.ps1 -Seconds 10 }
"`nReport: $report"
