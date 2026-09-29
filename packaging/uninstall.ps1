# SPDX-License-Identifier: Apache-2.0
#
# Removes Loadout for the current user.
#
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1 [-RemoveData]
#
# By default your data (%LOCALAPPDATA%\Loadout: settings, accounts, request history, backups)
# and the API keys in Windows Credential Manager are kept, so a reinstall picks them up.
# -RemoveData deletes both.

param(
    [string]$InstallDir = $PSScriptRoot,
    [switch]$RemoveData,
    [string]$ShortcutDir = [Environment]::GetFolderPath('Programs')
)
$ErrorActionPreference = 'Stop'
$cli = Join-Path $InstallDir 'loadout.exe'
if (-not (Test-Path $cli)) { throw "No Loadout installation in $InstallDir." }

Get-Process loadout-app -ErrorAction SilentlyContinue | Stop-Process -Force
& $cli daemon status *> $null
if ($LASTEXITCODE -eq 0) {
    Write-Host 'Stopping the gateway...'
    & $cli daemon stop
}
& $cli autostart disable | Out-Null

$shortcut = Join-Path $ShortcutDir 'Loadout.lnk'
if (Test-Path $shortcut) { Remove-Item $shortcut -Force }

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -and (@($userPath -split ';') -contains $InstallDir)) {
    $entries = @($userPath -split ';' | Where-Object { $_ -and $_ -ne $InstallDir })
    [Environment]::SetEnvironmentVariable('Path', ($entries -join ';'), 'User')
}

if ($RemoveData) {
    $data = Join-Path $env:LOCALAPPDATA 'Loadout'
    if (Test-Path $data) {
        Remove-Item -Recurse -Force $data
        Write-Host "Deleted $data."
    }
    # API keys stored by Loadout (Credential Manager targets starting with "loadout:").
    $targets = cmdkey /list | ForEach-Object {
        if ($_ -match 'target=(loadout:.*)$') { $Matches[1].Trim() }
    }
    foreach ($t in $targets) { cmdkey "/delete:$t" | Out-Null }
    Write-Host "Deleted $(@($targets).Count) stored API key(s)."
}

# This script runs from the folder it deletes: remove it after this process exits.
Start-Process -WindowStyle Hidden cmd.exe -ArgumentList '/c', "ping -n 3 127.0.0.1 >nul & rmdir /s /q `"$InstallDir`""
Write-Host "Loadout removed from $InstallDir."
if (-not $RemoveData) { Write-Host 'Your data and API keys were kept (use -RemoveData to delete them).' }
