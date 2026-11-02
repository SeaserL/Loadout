# SPDX-License-Identifier: Apache-2.0
# Builds the console and the window program that the package carries.
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
Push-Location $repo
try {
    & npm.cmd run fusion:ui
    if ($LASTEXITCODE -ne 0) { throw 'UI build failed' }
    $env:CARGO_TARGET_DIR = Join-Path $repo '.build\fusion-desktop-target'
    & cargo test -p loadout-fusion-desktop --release
    if ($LASTEXITCODE -ne 0) { throw 'Desktop tests failed' }
    & cargo build -p loadout-fusion-desktop --release
    if ($LASTEXITCODE -ne 0) { throw 'Desktop build failed' }
    Write-Host ('Desktop built: ' + (Join-Path $env:CARGO_TARGET_DIR 'release\Loadout.exe'))
} finally {
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Pop-Location
}
