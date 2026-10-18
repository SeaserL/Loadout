# SPDX-License-Identifier: Apache-2.0
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$Loadout)
$ErrorActionPreference = 'Stop'
$Loadout = (Resolve-Path -LiteralPath $Loadout).Path
$bridgeDir = Join-Path $env:LOCALAPPDATA 'LoadoutWorkBuddyBridge'
$launcher = Join-Path $bridgeDir 'spawn_detached.exe'
$script = Join-Path $bridgeDir 'Start-WorkBuddyBridge.ps1'
$node = (Get-Command node -ErrorAction Stop).Source
$supervisor = Join-Path $bridgeDir 'workbuddy_supervisor.mjs'
foreach ($required in @($launcher, (Join-Path $bridgeDir 'binding.json'), (Join-Path (Split-Path $Loadout) 'loadoutd.exe'))) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw 'Required installed dependency is missing' }
}
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$backup = Join-Path $bridgeDir ('autostart-backup-' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfff'))
$null = New-Item -ItemType Directory -Path $backup
$old = Get-ItemProperty -LiteralPath $runKey
@{ Loadout = $old.Loadout; LoadoutWorkBuddyBridge = $old.LoadoutWorkBuddyBridge } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $backup 'run-values.json') -Encoding UTF8
foreach ($name in @('Start-WorkBuddyBridge.ps1', 'autostart.json', 'workbuddy_supervisor.mjs')) {
    $source = Join-Path $bridgeDir $name
    if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $backup }
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Start-WorkBuddyBridge.ps1') -Destination $script -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'workbuddy_supervisor.mjs') -Destination $supervisor -Force
@{ loadout = $Loadout } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $bridgeDir 'autostart.json') -Encoding UTF8
& $Loadout autostart enable
if ($LASTEXITCODE -ne 0) { throw 'Loadout autostart registration failed' }
$command = '"' + $launcher + '" "' + $node + '" "' + $supervisor + '"'
if ($command.Length -gt 260) { throw 'Login startup command exceeds Windows Run limit' }
Set-ItemProperty -LiteralPath $runKey -Name LoadoutWorkBuddyBridge -Value $command -Type String
$null = & $launcher $node $supervisor
if ($LASTEXITCODE -ne 0) { throw 'Watcher launcher failed' }
Write-Output "Autostart installed. Previous settings: $backup"
