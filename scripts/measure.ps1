<#
 Samples the running collector's RAM and CPU. The collector exits by itself (gracefully) after the duration.
 Prereq: hermes-keytool init --data-dir <DataDir> --share-b <usb file>
 Usage: .\scripts\measure.ps1 [-Minutes 30] [-DataDir ...] [-IntervalSec 5]
#>
param(
  [int]$Minutes = 30,
  [string]$DataDir = (Join-Path $env:LOCALAPPDATA "Hermes-test"),
  [int]$IntervalSec = 5,
  [string]$Exe = (Join-Path (Split-Path -Parent $PSScriptRoot) "target\release\hermes-collector.exe")
)
$csv = Join-Path (Get-Location) ("hermes-measure-{0}.csv" -f (Get-Date -Format "yyyyMMdd-HHmmss"))
$secs = $Minutes * 60
$p = Start-Process -FilePath $Exe -ArgumentList @("run","--data-dir",$DataDir,"--quiet","--duration-secs",($secs + 5)) -PassThru -WindowStyle Minimized
Start-Sleep -Seconds 2
$cores = [Environment]::ProcessorCount
$prev = 0.0
$rows = @()
$start = Get-Date
"t_sec,working_set_mb,private_mb,cpu_percent_of_one_core,cpu_percent_of_machine" | Out-File $csv -Encoding ascii
while (-not $p.HasExited -and ((Get-Date) - $start).TotalSeconds -lt $secs) {
  Start-Sleep -Seconds $IntervalSec
  try { $p.Refresh() } catch { break }
  if ($p.HasExited) { break }
  $cpu = $p.TotalProcessorTime.TotalSeconds
  $one = [math]::Round(($cpu - $prev) / $IntervalSec * 100, 3)
  $prev = $cpu
  $row = [pscustomobject]@{
    t = [int]((Get-Date) - $start).TotalSeconds
    ws = [math]::Round($p.WorkingSet64 / 1MB, 2)
    priv = [math]::Round($p.PrivateMemorySize64 / 1MB, 2)
    one = $one
    mach = [math]::Round($one / $cores, 3)
  }
  $rows += $row
  ("{0},{1},{2},{3},{4}" -f $row.t, $row.ws, $row.priv, $row.one, $row.mach) | Out-File $csv -Append -Encoding ascii
}
if (-not $p.HasExited) { $p.WaitForExit(15000) | Out-Null }
if ($rows.Count -gt 0) {
  $ws = $rows | Measure-Object ws -Average -Maximum
  $pr = $rows | Measure-Object priv -Average -Maximum
  $cp = $rows | Measure-Object one -Average -Maximum
  "RESULT samples=$($rows.Count) interval=${IntervalSec}s duration=$([int]((Get-Date)-$start).TotalMinutes)min"
  "RESULT working_set_mb avg=$([math]::Round($ws.Average,1)) max=$([math]::Round($ws.Maximum,1))   (target 15-30, ceiling 50)"
  "RESULT private_mb     avg=$([math]::Round($pr.Average,1)) max=$([math]::Round($pr.Maximum,1))"
  "RESULT cpu_percent_of_one_core avg=$([math]::Round($cp.Average,3)) max=$([math]::Round($cp.Maximum,3))"
  "CSV: $csv"
} else { "No samples collected (did the collector start? is the key set initialised in $DataDir ?)" }
