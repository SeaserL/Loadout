# SPDX-License-Identifier: Apache-2.0
# Upgrade an existing user-owned bridge; never installs or downloads the official client.
[CmdletBinding()]
param([string]$BridgeDir=(Join-Path $env:LOCALAPPDATA 'LoadoutOpenCodeBridge'))
$ErrorActionPreference='Stop'
$BridgeDir=[IO.Path]::GetFullPath($BridgeDir)
$binding=Get-Content -LiteralPath (Join-Path $BridgeDir 'binding.json') -Raw | ConvertFrom-Json
$files=@('opencode_bridge.py','opencode_protocol.py','opencode_events.py','opencode_controls.mjs','Start-OpenCodeBridge.ps1')
foreach($name in $files){ if(-not(Test-Path -LiteralPath (Join-Path $PSScriptRoot $name))){throw 'Incomplete bridge update package'} }
$owned=@()
foreach($pair in @(@('bridge.pid',$binding.python),@('official.pid',(Join-Path $BridgeDir 'opencode.exe')))) {
    $pidFile=Join-Path $BridgeDir $pair[0]
    if(Test-Path -LiteralPath $pidFile){
        $process=Get-Process -Id ([int](Get-Content -LiteralPath $pidFile -Raw)) -ErrorAction SilentlyContinue
        if($process){
            if($process.Path -ne [IO.Path]::GetFullPath($pair[1])){throw 'Process ownership mismatch'}
            if($pair[0] -eq 'bridge.pid'){
                $command=(Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)").CommandLine
                if(-not $command.Contains((Join-Path $BridgeDir 'opencode_bridge.py'))){throw 'Bridge command ownership mismatch'}
            }
            $owned += $process
        }
    }
}
$backup=Join-Path $BridgeDir ('protection-backup-'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfff'))
$null=New-Item -ItemType Directory -Path $backup
foreach($name in $files){Copy-Item -LiteralPath (Join-Path $BridgeDir $name) -Destination $backup}
foreach($process in $owned){Stop-Process -Id $process.Id -Force;$process.WaitForExit()}
try {
    foreach($name in $files){Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $BridgeDir $name) -Force}
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $BridgeDir 'Start-OpenCodeBridge.ps1') -BridgeDir $BridgeDir
    if($LASTEXITCODE -ne 0){throw 'Updated bridge did not start'}
    Write-Output "Protected bridge installed. Backup: $backup"
} catch {
    # Stop only newly owned processes before restoring executable scripts.
    foreach($pair in @(@('bridge.pid',$binding.python),@('official.pid',(Join-Path $BridgeDir 'opencode.exe')))){
        $pidFile=Join-Path $BridgeDir $pair[0]
        if(Test-Path -LiteralPath $pidFile){
            $process=Get-Process -Id ([int](Get-Content -LiteralPath $pidFile -Raw)) -ErrorAction SilentlyContinue
            if($process -and $process.Path -eq [IO.Path]::GetFullPath($pair[1])){Stop-Process -Id $process.Id -Force;$process.WaitForExit()}
        }
    }
    foreach($name in $files){Copy-Item -LiteralPath (Join-Path $backup $name) -Destination (Join-Path $BridgeDir $name) -Force}
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $BridgeDir 'Start-OpenCodeBridge.ps1') -BridgeDir $BridgeDir
    throw
}
