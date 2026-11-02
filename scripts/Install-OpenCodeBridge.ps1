# SPDX-License-Identifier: Apache-2.0
# Fresh install of the OpenCode Free bridge (ADR-017, ExecPlan 033). Uses only the pinned
# official @opencode/cli-windows-x64 2.0.32 (downloaded from the npm registry or a local copy)
# after verifying its hashes. An existing bridge is upgraded with Update-OpenCodeBridge.ps1.
[CmdletBinding()]
param(
    [string]$BridgeDir = (Join-Path $env:LOCALAPPDATA 'LoadoutOpenCodeBridge'),
    # A local official opencode.exe 2.0.32 instead of downloading it.
    [string]$OpenCodeExe,
    # python.exe 3.9+; found with the py launcher or PATH when empty.
    [string]$Python,
    [string]$Loadout = (Join-Path $env:LOCALAPPDATA 'Programs\Loadout\loadout.exe'),
    # Install files and secrets only: start nothing and leave Loadout untouched.
    [switch]$NoStart,
    # Finish an install whose start or Loadout step failed: start and register only.
    [switch]$Resume
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$version = '2.0.32'
$tarball = 'https://registry.npmjs.org/@opencode/cli-windows-x64/-/cli-windows-x64-2.0.32.tgz'
$integrity = 'sha512-5VdKolj2/NkSk3BuM4qeiYpnNnGRT7szmd2RIv3+aSDPMwD9yRySYkJ2f1WIM7dTDfO5lZHAXVoipt6+i6AD4A=='
$exeSha256 = '045C13859180608981B87ABB901A31F9FB0D1845E1C40F273F8D421A16182903'

$BridgeDir = [IO.Path]::GetFullPath($BridgeDir)
$bindingPath = Join-Path $BridgeDir 'binding.json'
if ($Resume) {
    if ($NoStart -or -not (Test-Path -LiteralPath $bindingPath)) { throw 'Nothing to resume; run the install without -Resume' }
} elseif (Test-Path -LiteralPath $bindingPath) {
    throw "A bridge is already installed in $BridgeDir; use Update-OpenCodeBridge.ps1 to upgrade it"
}
$files = @('Start-OpenCodeBridge.ps1','opencode_bridge.py','opencode_protocol.py','opencode_events.py',
    'opencode_controls.mjs','opencode_bridge_setup.py','workbuddy_bridge_key.py','spawn_detached.exe')
foreach ($name in $files) {
    if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot $name))) { throw "Incomplete bridge package: $name is missing" }
}
if (-not $NoStart -and -not (Test-Path -LiteralPath $Loadout)) { throw 'loadout.exe not found; pass -Loadout <path to loadout.exe>' }

# A real interpreter, not the Microsoft Store alias that only opens the store.
function Resolve-Python {
    $candidates = if ($Python) { ,@{ exe = $Python; args = @() } } else { @{ exe = 'py'; args = @('-3') }, @{ exe = 'python'; args = @() } }
    foreach ($candidate in $candidates) {
        $command = Get-Command $candidate.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $command) { continue }
        try {
            $out = @(& $command.Source @($candidate.args) -c 'import sys; print(sys.executable); print(sys.version_info >= (3, 9))')
        } catch { continue }
        if ($LASTEXITCODE -eq 0 -and $out.Count -eq 2 -and $out[1] -eq 'True' -and (Test-Path -LiteralPath $out[0])) {
            return [IO.Path]::GetFullPath($out[0])
        }
    }
    throw 'Python 3.9 or newer was not found; install it from python.org or pass -Python <path to python.exe>'
}
$pythonExe = if ($Resume) { (Get-Content -LiteralPath $bindingPath -Raw | ConvertFrom-Json).python } else { Resolve-Python }
$env:LOADOUT_OPENCODE_DIR = $BridgeDir

if (-not $Resume) {
    $temp = Join-Path ([IO.Path]::GetTempPath()) ('loadout-opencode-' + [Guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $temp
    try {
        if (-not $OpenCodeExe) {
            Write-Output "Downloading official OpenCode $version (about 70 MB) from registry.npmjs.org"
            [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
            $tgz = Join-Path $temp 'cli-windows-x64.tgz'
            Invoke-WebRequest -Uri $tarball -OutFile $tgz -UseBasicParsing
            $sha = [Security.Cryptography.SHA512]::Create()
            $stream = [IO.File]::OpenRead($tgz)
            try { $actual = 'sha512-' + [Convert]::ToBase64String($sha.ComputeHash($stream)) } finally { $stream.Dispose(); $sha.Dispose() }
            if ($actual -ne $integrity) { throw 'The downloaded package failed the npm integrity check' }
            $unpacked = Join-Path $temp 'package'
            $null = New-Item -ItemType Directory -Path $unpacked
            & (Join-Path $env:SystemRoot 'System32\tar.exe') -xzf $tgz -C $unpacked
            if ($LASTEXITCODE -ne 0) { throw 'Cannot unpack the official package' }
            $found = @(Get-ChildItem -LiteralPath $unpacked -Recurse -File -Filter 'opencode.exe')
            if ($found.Count -ne 1) { throw 'The official package has no opencode.exe' }
            $OpenCodeExe = $found[0].FullName
        }
        $OpenCodeExe = [IO.Path]::GetFullPath($OpenCodeExe)
        if ((Get-FileHash -Algorithm SHA256 -LiteralPath $OpenCodeExe).Hash -ne $exeSha256) {
            throw "opencode.exe is not the official $version Windows x64 build"
        }

        $null = New-Item -ItemType Directory -Force -Path $BridgeDir
        foreach ($dir in @('workspace', 'data', 'config\opencode', 'cache', 'state')) {
            $null = New-Item -ItemType Directory -Force -Path (Join-Path $BridgeDir $dir)
        }
        foreach ($name in $files) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $BridgeDir -Force }
        Copy-Item -LiteralPath $OpenCodeExe -Destination (Join-Path $BridgeDir 'opencode.exe') -Force
        Get-ChildItem -LiteralPath $BridgeDir -File | Unblock-File
    } finally {
        Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
    }

    & $pythonExe (Join-Path $BridgeDir 'opencode_bridge_setup.py') init
    if ($LASTEXITCODE -ne 0) { throw 'Cannot create the bridge secrets' }
    # Written last: the start and update scripts treat binding.json as a finished install.
    [ordered]@{ port = 18789; version = $version; python = $pythonExe; official_port = 18790 } |
        ConvertTo-Json | Set-Content -Encoding UTF8 -LiteralPath $bindingPath
    Write-Output "Bridge files installed in $BridgeDir"
}
if ($NoStart) { return }

& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $BridgeDir 'Start-OpenCodeBridge.ps1') -BridgeDir $BridgeDir
if ($LASTEXITCODE -ne 0) {
    throw 'The bridge did not start; fix the cause, then run this script again with -Resume'
}
& $pythonExe (Join-Path $BridgeDir 'opencode_bridge_setup.py') account $Loadout
if ($LASTEXITCODE -ne 0) { throw 'Cannot register opencode-free/main in Loadout; start Loadout, then run this script again with -Resume' }
Write-Output 'Done. In Loadout open Accounts > OpenCode Free > main and choose "Turn routing on" to use it.'
Write-Output 'After a reboot run Start-OpenCodeBridge.ps1 in the bridge directory again.'
