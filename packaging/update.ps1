# SPDX-License-Identifier: Apache-2.0
# Embedded in the running Core. Never execute scripts from the downloaded package.
param([Parameter(Mandatory=$true)][string]$Manifest, [string]$TestRoot, [switch]$TestFailAfterCopy)
$ErrorActionPreference = 'Stop'
trap {
    [Console]::Error.WriteLine($_.Exception.Message)
    @{status='failed';error='Update validation or installation failed; see local update transaction'} | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path (Split-Path -Parent $Manifest) 'result.json')
    exit 1
}
$plan = Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json
$target = [IO.Path]::GetFullPath($plan.install_dir).TrimEnd('\')
$expected = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs\Loadout')).TrimEnd('\')
$testing = -not [string]::IsNullOrWhiteSpace($TestRoot)
if ($testing) {
    $boundary = [IO.Path]::GetFullPath($TestRoot).TrimEnd('\') + '\'
    if (-not $target.StartsWith($boundary, [StringComparison]::OrdinalIgnoreCase)) { throw 'Test target escapes test root' }
} elseif ($target -ne $expected) { throw 'Installation target is not the standard Loadout directory' }
if ((Get-Item -LiteralPath $target).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse install directory refused' }
$version = [string]$plan.version
if ($version -notmatch '^v?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') { throw 'Invalid release version' }
$version = $version.TrimStart('v')
$packageRoot = "Loadout-$version-windows-x64"
$programs = @('loadout.exe','loadoutd.exe','loadout-app.exe')
$allowed = $programs + @('LICENSE','NOTICE','THIRD_PARTY_NOTICES.md','THIRD_PARTY_LICENSES.md','README.txt','install.ps1','uninstall.ps1','install.cmd')
$hashStream = [IO.File]::OpenRead($plan.zip)
$hasher = [Security.Cryptography.SHA256]::Create()
try { $actualHash = [BitConverter]::ToString($hasher.ComputeHash($hashStream)).Replace('-','').ToLowerInvariant() }
finally { $hashStream.Dispose(); $hasher.Dispose() }
if ($actualHash -ne $plan.sha256) { throw 'Release checksum mismatch' }
$work = Join-Path (Split-Path -Parent $Manifest) ('transaction-' + [guid]::NewGuid().ToString('N'))
$stage = Join-Path $work 'stage'
$backup = Join-Path $work 'backup'
$null = New-Item -ItemType Directory -Force -Path $stage,$backup
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($plan.zip)
$seen = @{}
$total = 0L
try {
    foreach ($entry in $archive.Entries) {
        $name = $entry.FullName.Replace('\','/')
        if ($name.EndsWith('/') -and $entry.Length -eq 0 -and $name -eq "$packageRoot/") { continue }
        $parts = $name.Split('/')
        if ($parts.Length -ne 2 -or $parts[0] -ne $packageRoot -or $allowed -cnotcontains $parts[1] -or $seen.ContainsKey($parts[1])) { throw 'Unexpected or duplicate archive path' }
        $total += $entry.Length
        if ($entry.Length -gt 268435456 -or $total -gt 536870912) { throw 'Expanded update exceeds size limit' }
        $seen[$parts[1]] = $true
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $stage $parts[1]), $false)
    }
    foreach ($file in $programs) { if (-not $seen.ContainsKey($file)) { throw 'Incomplete release package' } }
} finally { $archive.Dispose() }
$cli = Join-Path $target 'loadout.exe'
$wasRunning = $false
$wasApp = $false
$mutated = $false
function Copy-Retry([string]$From, [string]$To) {
    for ($attempt=0; $attempt -lt 40; $attempt++) {
        try { Copy-Item -LiteralPath $From -Destination $To -Force; return }
        catch { if ($attempt -eq 39) { throw }; Start-Sleep -Milliseconds 500 }
    }
}
try {
    if (-not $testing) {
        $statusText = & $cli --json daemon status 2>$null | Out-String
        if ($LASTEXITCODE -eq 0) {
            $status = $statusText | ConvertFrom-Json
            $owner = Get-Process -Id $status.pid -ErrorAction Stop
            if ($owner.Path -ne (Join-Path $target 'loadoutd.exe')) { throw 'Running gateway belongs to another installation' }
            $wasRunning = $true
            & $cli daemon stop *> $null
            if ($LASTEXITCODE -ne 0) { throw 'Gateway did not stop' }
        }
        Get-Process loadout-app -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $target 'loadout-app.exe') } | ForEach-Object { $wasApp=$true; Stop-Process -Id $_.Id -Force }
    }
    foreach ($file in $seen.Keys) {
        $old = Join-Path $target $file
        if (Test-Path -LiteralPath $old) {
            if ((Get-Item -LiteralPath $old).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse installation file refused' }
            Copy-Retry $old (Join-Path $backup $file)
        }
    }
    $mutated = $true
    foreach ($file in $seen.Keys) { Copy-Retry (Join-Path $stage $file) (Join-Path $target $file) }
    if ($testing -and $TestFailAfterCopy) { throw 'Synthetic failure after copy' }
    if (-not $testing) {
        $reported = & $cli --version | Out-String
        if ($LASTEXITCODE -ne 0 -or $reported.Trim() -ne "loadout $version") { throw 'Installed version check failed' }
        & $cli daemon start *> $null
        if ($LASTEXITCODE -ne 0) { throw 'Updated gateway did not start' }
        Start-Process -FilePath (Join-Path $target 'loadout-app.exe') -WindowStyle Hidden
    }
    @{status='installed';version=$version;backup=$backup} | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path (Split-Path -Parent $Manifest) 'result.json')
} catch {
    $failure = $_
    if ($mutated) {
        if (-not $testing) {
            Get-Process loadoutd,loadout-app -ErrorAction SilentlyContinue | Where-Object { $_.Path -in @((Join-Path $target 'loadoutd.exe'),(Join-Path $target 'loadout-app.exe')) } | Stop-Process -Force
        }
        foreach ($file in $seen.Keys) {
            $saved = Join-Path $backup $file
            if (Test-Path -LiteralPath $saved) { Copy-Retry $saved (Join-Path $target $file) }
            elseif (Test-Path -LiteralPath (Join-Path $target $file)) { Remove-Item -LiteralPath (Join-Path $target $file) -Force }
        }
    }
    if (-not $testing) {
        if ($wasRunning) { & $cli daemon start *> $null }
        if ($wasApp) { Start-Process -FilePath (Join-Path $target 'loadout-app.exe') -WindowStyle Hidden }
    }
    @{status='failed';rolled_back=$mutated;error='Update failed; previous programs restored when replacement had started';backup=$backup} | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path (Split-Path -Parent $Manifest) 'result.json')
    [Console]::Error.WriteLine($failure.Exception.Message)
    exit 1
}
