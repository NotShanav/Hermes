<#
 Builds Hermes on Windows (release) into .\dist.   Usage:  .\scripts\build.ps1
 Needs: Rust (https://rustup.rs) and "Visual Studio Build Tools 2022" with the "Desktop development with C++" workload.
#>
$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw "cargo not found: install Rust from https://rustup.rs and reopen PowerShell" }
rustc --version
cargo build --release --workspace --locked
if ($LASTEXITCODE -ne 0) { throw "build failed" }
New-Item -ItemType Directory -Force -Path dist | Out-Null
foreach ($e in "hermes","hermes-tray","hermes-collector","hermes-keytool","hermes-dump") { Copy-Item "target\release\$e.exe" dist -Force }
Write-Host "`nBuilt. Executables are in $(Resolve-Path dist)"
Write-Host "First steps:  .\dist\hermes.exe help"
