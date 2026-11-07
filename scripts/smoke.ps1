<#
 End-to-end smoke test in a throw-away folder: init -> record -> stop -> status/check/search/export/view.
 Usage:  .\scripts\smoke.ps1 [-Exe .\target\release\hermes.exe] [-Seconds 10] [-NonInteractive]
 While it records, switch between a few windows (the script tells you when).
#>
param(
  [string]$Exe = ".\target\release\hermes.exe",
  [int]$Seconds = 10,
  [switch]$NonInteractive
)
$ErrorActionPreference = "Continue"   # (native stderr must not become a terminating error in Windows PowerShell 5.1)
$Exe = (Resolve-Path $Exe).Path
$root = Join-Path ([IO.Path]::GetTempPath()) ("hermes-smoke-" + [guid]::NewGuid().ToString("N").Substring(0,8))
$data = Join-Path $root "data"
$usb  = Join-Path $root "usb-stand-in"        # a real run uses a USB stick: hermes init --usb E:\
$key  = Join-Path $usb  "hermes-share-b.key"
function Run($label, [scriptblock]$b) { Write-Host "`n--- $label"; & $b; if ($LASTEXITCODE -ne 0) { Fail "$label failed (exit $LASTEXITCODE)" } }
function Fail($m) { Write-Host "SMOKE TEST FAILED: $m" -ForegroundColor Red; exit 1 }

Run "init" { & $Exe init --data-dir $data --usb $key --allow-same-drive }
Write-Host "`n--- init again (must refuse to overwrite)"
& $Exe init --data-dir $data --usb "$usb\other.key" --allow-same-drive
if ($LASTEXITCODE -ne 1) { Fail "second init was not refused" }

if (-not $NonInteractive) { Write-Host "`n>>> Recording for $Seconds seconds: switch between a few windows now (Notepad, browser, ...) <<<" -ForegroundColor Yellow }
# a window of our own, so even a CI machine without anybody at the keyboard has something to record
$notepad = Start-Process notepad -PassThru -ErrorAction SilentlyContinue
$p = Start-Process $Exe -ArgumentList @("start","--quiet","--data-dir",$data,"--duration-secs",$Seconds) -PassThru
Start-Sleep -Seconds ([Math]::Max(3, $Seconds - 3))
if ($notepad) { try { Stop-Process -Id $notepad.Id -Force } catch {} }
$p.WaitForExit(($Seconds + 30) * 1000) | Out-Null
if (-not $p.HasExited) { & $Exe stop; Fail "collector did not stop by itself" }
if ($p.ExitCode -ne 0) { Fail "collector exit code $($p.ExitCode) (is there an interactive desktop? the tracker needs one)" }

Run "status"  { & $Exe status --data-dir $data }
Run "check"   { & $Exe check  --data-dir $data --share-b $key }
Run "search (any session boundary)" { & $Exe search session --data-dir $data --share-b $key --utc }
$csv = Join-Path $root "export.csv"
Run "export"  { & $Exe export --data-dir $data --share-b $key --out $csv --utc }
if (-not (Select-String -Path $csv -Pattern "SESSION START" -Quiet)) { Fail "export has no session start" }
Write-Host "`nview (scripted):"
$cmds = Join-Path $root "view-commands.txt"; "summary`nverify`nquit" | Set-Content $cmds
Get-Content $cmds | & $Exe view --data-dir $data --share-b $key --utc
Write-Host "`nSMOKE TEST PASSED (folder: $root, safe to delete)" -ForegroundColor Green
