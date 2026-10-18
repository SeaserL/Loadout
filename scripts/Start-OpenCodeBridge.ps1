# SPDX-License-Identifier: Apache-2.0
[CmdletBinding()]
param([string]$BridgeDir = (Join-Path $env:LOCALAPPDATA 'LoadoutOpenCodeBridge'), [switch]$OfficialOnly)
$ErrorActionPreference = 'Stop'
$BridgeDir = [IO.Path]::GetFullPath($BridgeDir)
$binding = Get-Content -LiteralPath (Join-Path $BridgeDir 'binding.json') -Raw | ConvertFrom-Json
# Stable releases keep a single official worker; the experimental pool is not deployed.
$binding | Add-Member -NotePropertyName official_ports -NotePropertyValue @($binding.official_port) -Force
$mutex = [Threading.Mutex]::new($false, $(if ($OfficialOnly) { 'Local\LoadoutOpenCodeWorker-' + $binding.official_port } else { 'Local\LoadoutOpenCodeBridge-Startup' }))
$held = $false
try {
    try { $held = $mutex.WaitOne(20000) } catch [Threading.AbandonedMutexException] { $held = $true }
    if (-not $held) { throw 'Bridge startup busy' }
    $env:LOADOUT_OPENCODE_DIR = $BridgeDir
    $secretJson = & $binding.python (Join-Path $BridgeDir 'opencode_bridge_setup.py') read
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read bridge secrets' }
    $keys = $secretJson | ConvertFrom-Json
    $headers = @{ Authorization = 'Bearer ' + $keys.bridge }
    if (-not $OfficialOnly) { try {
        $health = Invoke-RestMethod "http://127.0.0.1:$($binding.port)/health" -Headers $headers -TimeoutSec 2
        if ($health.status -eq 'ok') { Write-Output 'OpenCode bridge ready'; return }
    } catch { } }
    function Assert-PortFree([int]$Port) {
        $socket = [Net.Sockets.TcpClient]::new()
        try {
            $pending = $socket.ConnectAsync('127.0.0.1', $Port)
            try { $null = $pending.Wait(300) } catch { }
            if ($socket.Connected) { throw 'Port occupied; stop the owned instance before restarting' }
        } finally { $socket.Dispose() }
    }
    if (-not $OfficialOnly) { Assert-PortFree $binding.port }
    $basic = @{ Authorization = 'Basic ' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('opencode:' + $keys.official)) }
    $officialReady = $false
    try {
        $info = Invoke-RestMethod "http://127.0.0.1:$($binding.official_port)/api/info" -Headers $basic -TimeoutSec 2
        if ($info.version -ne '2.0.32') { throw 'Unsupported official version' }
        $officialReady = $true
    } catch { }
    if (-not $officialReady) {
        Assert-PortFree $binding.official_port
        $env:XDG_DATA_HOME = Join-Path $BridgeDir 'data'
        $env:XDG_CONFIG_HOME = Join-Path $BridgeDir 'config'
        $env:XDG_CACHE_HOME = Join-Path $BridgeDir 'cache'
        $env:XDG_STATE_HOME = Join-Path $BridgeDir 'state'
        $env:OPENCODE_TEST_HOME = Join-Path $BridgeDir 'home'
        $env:OPENCODE_CONFIG_DIR = Join-Path $BridgeDir 'config\opencode'
        $env:OPENCODE_DISABLE_PROJECT_CONFIG = '1'
        $env:OPENCODE_DISABLE_CLAUDE_CODE = '1'
        $env:OPENCODE_DISABLE_AUTOUPDATE = '1'
        $env:OPENCODE_DB = ':memory:'
        $env:LOADOUT_OPENCODE_CONTROL_URL = "http://127.0.0.1:$($binding.port)"
        $pluginDir = Join-Path $BridgeDir 'controls-plugin'
        $null = New-Item -ItemType Directory -Force -Path $pluginDir
        Copy-Item -LiteralPath (Join-Path $BridgeDir 'opencode_controls.mjs') -Destination (Join-Path $pluginDir 'index.js') -Force
        '{"name":"loadout-request-controls","type":"module","main":"index.js"}' | Set-Content (Join-Path $pluginDir 'package.json')
        $env:OPENCODE_CONFIG_CONTENT = @{permissions=@(@{action='*';resource='*';effect='ask'});plugins=@($pluginDir)} | ConvertTo-Json -Depth 5 -Compress
        $env:OPENCODE_SERVER_PASSWORD = $keys.official
        Set-Location (Join-Path $BridgeDir 'workspace')
        $officialExe = if ($binding.official_exe) { $binding.official_exe } else { Join-Path $BridgeDir 'opencode.exe' }
        & (Join-Path $BridgeDir 'spawn_detached.exe') $officialExe serve --hostname 127.0.0.1 --port "$($binding.official_port)" | Set-Content (Join-Path $BridgeDir 'official.pid')
        if ($LASTEXITCODE -ne 0) { throw 'Official launch failed' }
        for ($i = 0; $i -lt 30; $i++) {
            Start-Sleep -Milliseconds 300
            try {
                $info = Invoke-RestMethod "http://127.0.0.1:$($binding.official_port)/api/info" -Headers $basic -TimeoutSec 1
                if ($info.version -eq '2.0.32') { $officialReady = $true; break }
            } catch { }
        }
        if (-not $officialReady) { throw 'Official server did not start' }
    }
    if ($OfficialOnly) { Write-Output 'Official worker ready'; return }
    if ($binding.official_ports) {
        if ($binding.official_ports.Count -gt 2 -or $binding.official_ports[0] -ne $binding.official_port) { throw 'Invalid worker configuration' }
        for ($index = 1; $index -lt $binding.official_ports.Count; $index++) {
            $workerDir = Join-Path $BridgeDir "workers\$index"
            $null = New-Item -ItemType Directory -Force -Path (Join-Path $workerDir 'workspace')
            foreach ($file in @('spawn_detached.exe','secrets.dpapi','opencode_bridge_setup.py','workbuddy_bridge_key.py','opencode_controls.mjs')) {
                Copy-Item -LiteralPath (Join-Path $BridgeDir $file) -Destination (Join-Path $workerDir $file) -Force
            }
            @{port=$binding.port;official_port=$binding.official_ports[$index];python=$binding.python;official_exe=(Join-Path $BridgeDir 'opencode.exe')} | ConvertTo-Json | Set-Content (Join-Path $workerDir 'binding.json')
            & powershell -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath -BridgeDir $workerDir -OfficialOnly
            if ($LASTEXITCODE -ne 0) { throw 'Additional official worker failed' }
        }
    }
    $env:LOADOUT_OPENCODE_DIR = $BridgeDir
    $env:OPENCODE_SERVER_PASSWORD = $null
    & (Join-Path $BridgeDir 'spawn_detached.exe') $binding.python (Join-Path $BridgeDir 'opencode_bridge.py') | Set-Content (Join-Path $BridgeDir 'bridge.pid')
    if ($LASTEXITCODE -ne 0) { throw 'Bridge launch failed' }
    for ($i = 0; $i -lt 20; $i++) {
        Start-Sleep -Milliseconds 300
        try {
            $health = Invoke-RestMethod "http://127.0.0.1:$($binding.port)/health" -Headers $headers -TimeoutSec 1
            if ($health.status -eq 'ok') { Write-Output 'OpenCode bridge ready'; return }
        } catch { }
    }
    throw 'Bridge did not start'
} finally {
    $env:OPENCODE_SERVER_PASSWORD = $null
    $keys = $null
    $secretJson = $null
    if ($held) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
