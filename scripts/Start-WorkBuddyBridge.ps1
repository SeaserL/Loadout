# SPDX-License-Identifier: Apache-2.0
[CmdletBinding()]
param([string]$BridgeDir = (Join-Path $env:LOCALAPPDATA 'LoadoutWorkBuddyBridge'))
$ErrorActionPreference = 'Stop'
$BridgeDir = [IO.Path]::GetFullPath($BridgeDir)
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$sha = [Security.Cryptography.SHA256]::Create()
try { $suffix = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($BridgeDir.ToLowerInvariant()))).Replace('-', '') } finally { $sha.Dispose() }
$mutexBase = "Local\LoadoutWorkBuddy-$sid-$suffix"
function Enter-Lock($Mutex, $Timeout) {
    try { return $Mutex.WaitOne($Timeout) }
    catch [Threading.AbandonedMutexException] { return $true }
}
function Ensure-Bridge {
    $mutex = [Threading.Mutex]::new($false, "$mutexBase-start")
    $held = $false
    $bridgeKey = $null
    try {
        $held = Enter-Lock $mutex 20000
        if (-not $held) { throw 'Bridge startup is busy' }
        $binding = Get-Content -LiteralPath (Join-Path $BridgeDir 'binding.json') -Raw | ConvertFrom-Json
        $bridgeKey = & $binding.python (Join-Path $BridgeDir 'workbuddy_bridge_key.py') read 2>$null
        if ($LASTEXITCODE -ne 0 -or "$bridgeKey".Length -lt 32) { throw 'Protected bridge key unavailable' }
        $uri = "http://127.0.0.1:$($binding.port)/health"
        try {
            $health = Invoke-RestMethod -Uri $uri -Headers @{ Authorization = "Bearer $bridgeKey" } -TimeoutSec 2
            if ($health.status -eq 'ok') { return 'ready' }
        } catch { }
        # Never repeatedly spawn children against an occupied but unhealthy listener.
        $socket = [Net.Sockets.TcpClient]::new()
        try {
            $pending = $socket.ConnectAsync('127.0.0.1', [int]$binding.port)
            try { $null = $pending.Wait(300) } catch { }
            if ($socket.Connected) { throw 'Bridge port occupied but authentication or account health failed' }
        } finally { $socket.Dispose() }
        $node = (Get-Command node -ErrorAction Stop).Source
        $null = & (Join-Path $BridgeDir 'spawn_detached.exe') $node (Join-Path $BridgeDir 'workbuddy_bridge.mjs')
        if ($LASTEXITCODE -ne 0) { throw 'Bridge launcher failed' }
        for ($attempt = 0; $attempt -lt 20; $attempt++) {
            Start-Sleep -Milliseconds 500
            try {
                $health = Invoke-RestMethod -Uri $uri -Headers @{ Authorization = "Bearer $bridgeKey" } -TimeoutSec 1
                if ($health.status -eq 'ok') { return 'ready' }
            } catch { }
        }
        throw 'Bridge did not become ready'
    } finally {
        $bridgeKey = $null
        if ($held) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
}
try { $null = Ensure-Bridge; Write-Output 'WorkBuddy bridge is ready.' }
catch { Write-Error 'Bridge startup failed. Check the client account, local port and installed dependencies.' }
