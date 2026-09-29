# SPDX-License-Identifier: Apache-2.0
#
# Installs or upgrades Loadout for the current user (no administrator rights needed).
#
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-AddToPath] [-Autostart] [-NoStart]
#
# Programs go to %LOCALAPPDATA%\Programs\Loadout; your data stays in %LOCALAPPDATA%\Loadout.
# An upgrade stops the running gateway gracefully, replaces the programs and starts it again.

param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Loadout'),
    # Make `loadout` available in new terminals.
    [switch]$AddToPath,
    # Start the gateway when you sign in to Windows.
    [switch]$Autostart,
    # Do not start the gateway and the app after installing.
    [switch]$NoStart,
    # Where the Start Menu shortcut goes (tests use a temporary folder).
    [string]$ShortcutDir = [Environment]::GetFolderPath('Programs')
)
$ErrorActionPreference = 'Stop'
$source = $PSScriptRoot
$programs = 'loadoutd.exe', 'loadout.exe', 'loadout-app.exe'
$extras = 'LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_LICENSES.md', 'README.txt', 'uninstall.ps1'

foreach ($p in $programs) {
    if (-not (Test-Path (Join-Path $source $p))) {
        throw "$p is missing next to install.ps1; extract the whole zip first."
    }
}
$newCli = Join-Path $source 'loadout.exe'
$installedCli = Join-Path $InstallDir 'loadout.exe'

# 1. Stop what is running (the IPC pipe is per user, so the new CLI can stop an old daemon).
$wasRunning = $false
& $newCli daemon status *> $null
if ($LASTEXITCODE -eq 0) {
    $wasRunning = $true
    Write-Host 'Stopping the running gateway (waits for streaming requests to finish)...'
    & $newCli daemon stop
    if ($LASTEXITCODE -ne 0) { throw 'Could not stop the running gateway; stop it with "loadout daemon stop" and retry.' }
}
Get-Process loadout-app -ErrorAction SilentlyContinue | Stop-Process -Force
# Programs may stay locked for a moment after the processes exit.
Start-Sleep -Milliseconds 500

# Was autostart registered (possibly with an older path)?
$autostartWasOn = $false
if (Test-Path $installedCli) {
    $status = & $installedCli --json autostart status 2>$null | Out-String
    if ($status -match '"enabled":\s*true') { $autostartWasOn = $true }
}

# 2. Copy the programs.
New-Item -ItemType Directory -Force $InstallDir | Out-Null
foreach ($f in $programs + $extras) {
    $from = Join-Path $source $f
    if (Test-Path $from) {
        for ($try = 1; ; $try++) {
            try { Copy-Item $from $InstallDir -Force; break }
            catch {
                if ($try -ge 10) { throw }
                Start-Sleep -Milliseconds 500
            }
        }
    }
}

# Files extracted from a downloaded zip carry the "from the internet" mark.
Get-ChildItem $InstallDir -File | Unblock-File

# 3. Start Menu shortcut for the desktop app.
$shortcut = Join-Path $ShortcutDir 'Loadout.lnk'
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = Join-Path $InstallDir 'loadout-app.exe'
$link.WorkingDirectory = $InstallDir
$link.Description = 'Loadout - All your AI, one endpoint.'
$link.Save()

# 4. Optional PATH entry.
if ($AddToPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @($userPath -split ';' | Where-Object { $_ })
    if ($entries -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable('Path', (($entries + $InstallDir) -join ';'), 'User')
        Write-Host "Added $InstallDir to your PATH (new terminals)."
    }
}

# 5. Autostart: register, or refresh an existing registration to the new location.
$cli = Join-Path $InstallDir 'loadout.exe'
if ($Autostart -or $autostartWasOn) {
    & $cli autostart enable | Out-Null
}

# 6. Start.
if (-not $NoStart) {
    & $cli daemon start
    if ($LASTEXITCODE -ne 0) { throw 'The gateway did not start; see %LOCALAPPDATA%\Loadout\logs.' }
    Start-Process (Join-Path $InstallDir 'loadout-app.exe')
}

Write-Host ''
if ($wasRunning) { Write-Host "Loadout upgraded in $InstallDir." } else { Write-Host "Loadout installed in $InstallDir." }
Write-Host 'Open "Loadout" from the Start Menu. Uninstall with uninstall.ps1 in that folder.'
