# SPDX-License-Identifier: Apache-2.0
#
# Builds the Windows x64 release package (ExecPlan 007 §2):
#   dist\Loadout-<version>-windows-x64\   loadoutd.exe, loadout.exe, loadout-app.exe,
#                                          install / uninstall scripts, licenses
#   dist\Loadout-<version>-windows-x64.zip (+ .sha256)
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts\package.ps1 [-OutDir dist] [-SkipBuild]

param(
    [string]$OutDir = "dist",
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

function Invoke-Checked([string]$what, [scriptblock]$block) {
    & $block
    if ($LASTEXITCODE -ne 0) { throw "$what failed (exit $LASTEXITCODE)" }
}

$meta = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$version = ($meta.packages | Where-Object { $_.name -eq 'loadout' }).version
$name = "Loadout-$version-windows-x64"
Write-Host "Packaging $name"

if (-not $SkipBuild) {
    Invoke-Checked 'npm ci' { npm.cmd ci --no-audit --no-fund }
    Invoke-Checked 'frontend build' { npm.cmd run build }
    Invoke-Checked 'cargo build' { cargo build --release -p loadout -p loadout-desktop }
    # The bridge kit's process launcher, so installing the bridge needs no Rust (ExecPlan 033).
    Invoke-Checked 'spawn_detached build' { cargo build --release -p loadout-platform --example spawn_detached }
}

$stage = Join-Path $OutDir $name
$stage = [IO.Path]::GetFullPath($stage)
$outRoot = [IO.Path]::GetFullPath($OutDir).TrimEnd('\') + '\'
if (-not $stage.StartsWith($outRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid package target' }
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null

foreach ($exe in 'loadoutd.exe', 'loadout.exe', 'loadout-app.exe') {
    Copy-Item (Join-Path 'target\release' $exe) $stage
}
foreach ($file in 'LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.md') {
    Copy-Item $file $stage
}
Copy-Item 'packaging\install.ps1', 'packaging\uninstall.ps1', 'packaging\install.cmd', 'packaging\README.txt' $stage
Invoke-Checked 'license bundle' { python scripts\third_party_licenses.py --out (Join-Path $stage 'THIRD_PARTY_LICENSES.md') }

$zip = Join-Path $OutDir "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $stage -DestinationPath $zip
$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
Set-Content -Encoding ascii -Path "$zip.sha256" -Value "$hash  $name.zip"

# Optional OpenCode bridge kit (install and update): our scripts and launcher only; the
# installer fetches the pinned official binary itself. No credentials.
$bridgeStage = Join-Path $OutDir "$name-opencode-bridge"
if (Test-Path $bridgeStage) { Remove-Item -Recurse -Force $bridgeStage }
$null = New-Item -ItemType Directory -Force -Path $bridgeStage
foreach ($file in 'Install-OpenCodeBridge.ps1','Update-OpenCodeBridge.ps1','Start-OpenCodeBridge.ps1','opencode_bridge.py','opencode_protocol.py','opencode_events.py','opencode_controls.mjs','opencode_bridge_setup.py','workbuddy_bridge_key.py') {
    Copy-Item -LiteralPath (Join-Path 'scripts' $file) -Destination $bridgeStage -Force
}
Copy-Item -LiteralPath 'target\release\examples\spawn_detached.exe' -Destination $bridgeStage -Force
Copy-Item -LiteralPath 'LICENSE' -Destination $bridgeStage -Force
Copy-Item -LiteralPath (Join-Path $stage 'THIRD_PARTY_LICENSES.md') -Destination $bridgeStage -Force
Copy-Item -LiteralPath 'packaging/OPENCODE-BRIDGE.txt' -Destination (Join-Path $bridgeStage 'README.txt') -Force
$bridgeZip = Join-Path $OutDir "$name-opencode-bridge.zip"
Compress-Archive -Path $bridgeStage -DestinationPath $bridgeZip -Force
$bridgeHash = (Get-FileHash -Algorithm SHA256 $bridgeZip).Hash.ToLower()
Set-Content -Encoding ascii -Path "$bridgeZip.sha256" -Value "$bridgeHash  $name-opencode-bridge.zip"

Write-Host ""
Get-ChildItem $stage | Select-Object Name, @{ n = 'KB'; e = { [math]::Round($_.Length / 1KB) } } | Format-Table -AutoSize
Write-Host "Package: $zip"
Write-Host "SHA-256: $hash"
