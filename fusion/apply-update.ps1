# SPDX-License-Identifier: Apache-2.0
# Swaps the program folder for a verified new version, starts it, and restores the old one if it
# will not run (ADR-020). Runs from a copy outside the package, because it replaces that folder.
# Comes from the version being replaced: nothing from the download runs until it has been checked.
param(
    [Parameter(Mandatory = $true)][string]$Package,
    [Parameter(Mandatory = $true)][string]$Staged,
    [Parameter(Mandatory = $true)][string]$Instance,
    [Parameter(Mandatory = $true)][string]$UpdateDir,
    [Parameter(Mandatory = $true)][string]$From,
    [Parameter(Mandatory = $true)][string]$To,
    # Tests move folders only: no processes, so the owner's own window is never touched.
    [switch]$TestNoProcess,
    [switch]$TestFailStart,
    [switch]$Relaunched
)
$ErrorActionPreference = 'Stop'
$Package = [IO.Path]::GetFullPath($Package).TrimEnd('\')
$Staged = [IO.Path]::GetFullPath($Staged).TrimEnd('\')
$Instance = [IO.Path]::GetFullPath($Instance).TrimEnd('\')
$UpdateDir = [IO.Path]::GetFullPath($UpdateDir).TrimEnd('\')
$previous = "$Package.previous-$From"
$backup = Join-Path $UpdateDir "database-before-$To"
$database = Join-Path $Instance 'data'

# Node puts the processes it starts in a job that ends them when Node exits, and the updater exits
# right after starting this script. What this script starts falls outside that job, so the work
# happens in a second copy of it. Not "detached" from Node: without a console PowerShell does nothing.
if (-not $Relaunched) {
    $arguments = @(
        '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Relaunched',
        '-Package', "`"$Package`"", '-Staged', "`"$Staged`"", '-Instance', "`"$Instance`"", '-UpdateDir', "`"$UpdateDir`"",
        '-From', $From, '-To', $To
    )
    if ($TestNoProcess) { $arguments += '-TestNoProcess' }
    if ($TestFailStart) { $arguments += '-TestFailStart' }
    # Windows locks a process's working directory, so it is not one of the owner's folders.
    Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList $arguments -WindowStyle Hidden -WorkingDirectory ([Environment]::SystemDirectory)
    exit 0
}

function Write-Step([string]$Text) {
    $line = "$([DateTime]::UtcNow.ToString('s'))Z $Text`r`n"
    [IO.File]::AppendAllText((Join-Path $UpdateDir 'apply-update.log'), $line, [Text.UTF8Encoding]::new($false))
}
function Write-Result([string]$Status, [string]$Reason) {
    $text = @{ status = $Status; from = $From; to = $To; reason = $Reason; at_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() } | ConvertTo-Json
    [IO.File]::WriteAllText((Join-Path $UpdateDir 'result.json'), $text, [Text.UTF8Encoding]::new($false))
}
function Set-Phase([string]$Phase) {
    # ReadAllText, not Get-Content: Windows PowerShell would read this UTF-8 file as ANSI.
    $file = Join-Path $UpdateDir 'status.json'
    $status = @{}
    if (Test-Path -LiteralPath $file) {
        ([IO.File]::ReadAllText($file) | ConvertFrom-Json).PSObject.Properties | ForEach-Object { $status[$_.Name] = $_.Value }
    }
    $status.phase = $Phase
    $status.error = $null
    [IO.File]::WriteAllText($file, ($status | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
}
# Antivirus and indexers hold files for a moment; a folder move is retried, not abandoned at once.
function Move-Folder([string]$Source, [string]$Target) {
    for ($attempt = 0; ; $attempt++) {
        try { [IO.Directory]::Move($Source, $Target); return }
        catch {
            if ($attempt -ge 40) { throw "FOLDER_BUSY: $Source" }
            Start-Sleep -Milliseconds 500
        }
    }
}
function Get-PackageProcess {
    Get-CimInstance Win32_Process -Filter "Name='node.exe' OR Name='Loadout.exe'" |
        Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith("$Package\", [StringComparison]::OrdinalIgnoreCase) }
}
# Not Start-Process -Wait: it also waits for what the command leaves running, and "prepare" leaves
# the backend running on purpose.
function Invoke-Helper([string]$Command) {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = "$Package\node.exe"
    $info.Arguments = "`"$Package\tools\desktop.mjs`" $Command `"$Instance`""
    $info.WorkingDirectory = $Package
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $process = [Diagnostics.Process]::Start($info)
    if (-not $process.WaitForExit(180000)) {
        $process.Kill()
        return 1
    }
    return $process.ExitCode
}
function Stop-Package {
    try { $null = Invoke-Helper 'stop' } catch { }
    # Only what runs from this folder: another Loadout elsewhere on the machine is not ours to stop.
    Get-PackageProcess | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    for ($i = 0; $i -lt 60 -and @(Get-PackageProcess).Count -gt 0; $i++) { Start-Sleep -Milliseconds 500 }
    if (@(Get-PackageProcess).Count -gt 0) { throw 'PROCESSES_STILL_RUNNING' }
}
function Start-Backend {
    if ($TestNoProcess) { return -not $TestFailStart }
    return (Invoke-Helper 'prepare') -eq 0
}
function Open-Window {
    if ($hadWindow -and -not $TestNoProcess) {
        Start-Process -FilePath "$Package\Loadout.exe" -ArgumentList @('--instance', "`"$Instance`"") -WorkingDirectory $Package
    }
}
function Save-Database {
    if (-not (Test-Path -LiteralPath $database)) { return }
    Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue
    $null = New-Item -ItemType Directory -Force -Path $backup
    Get-ChildItem -LiteralPath $database -File -Filter 'storage.sqlite*' | Copy-Item -Destination $backup -Force
}
function Restore-Database {
    if (-not (Test-Path -LiteralPath $backup)) { return }
    Get-ChildItem -LiteralPath $database -File -Filter 'storage.sqlite*' -ErrorAction SilentlyContinue | Remove-Item -Force
    Get-ChildItem -LiteralPath $backup -File | Copy-Item -Destination $database -Force
}

$hadWindow = $false
$swapped = $false
try {
    Write-Step "updating $From to $To"
    if (-not (Test-Path -LiteralPath (Join-Path $Staged 'fusion-manifest.json'))) { throw 'STAGED_MISSING' }
    # A copy an earlier update could not delete. This version is running, so nothing needs that one.
    Get-ChildItem -LiteralPath (Split-Path $Package) -Directory -Filter "$(Split-Path $Package -Leaf).previous-*" | Remove-Item -Recurse -Force
    if (-not $TestNoProcess) {
        $hadWindow = @(Get-PackageProcess | Where-Object { $_.Name -eq 'Loadout.exe' }).Count -gt 0
        # Let the response that started this reach the window first.
        Start-Sleep -Seconds 1
        Stop-Package
    }
    Write-Step 'stopped; saving the database'
    Save-Database
    Move-Folder $Package $previous
    $swapped = $true
    Move-Folder $Staged $Package
    Write-Step 'swapped; starting the new version'
    if (-not (Start-Backend)) { throw 'NEW_VERSION_DID_NOT_START' }
    Write-Step 'running'
    Write-Result 'installed' ''
    Open-Window
    # The new version runs: nothing below can harm it, so failures here are only leftovers.
    Remove-Item -LiteralPath (Join-Path $UpdateDir 'status.json') -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $previous -Recurse -Force -ErrorAction SilentlyContinue
} catch {
    $reason = $_.Exception.Message
    try {
        Write-Step "failed: $reason"
        if ($swapped) {
            if (-not $TestNoProcess -and (Test-Path -LiteralPath (Join-Path $Package 'node.exe'))) { Stop-Package }
            if (Test-Path -LiteralPath $Package) { Move-Folder $Package $Staged }
            Move-Folder $previous $Package
            Restore-Database
            Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue
        }
        # The result comes last: once it is there, the old version is back and answering.
        if (-not $TestNoProcess) {
            $restarted = Invoke-Helper 'prepare'
            Write-Step "restored; backend start exit $restarted"
        }
        # The download stays, so a retry does not fetch it again.
        Set-Phase 'ready'
        Write-Result 'failed' $reason
        Open-Window
    } catch {
        Write-Result 'failed' ("$reason; restore: " + $_.Exception.Message)
        exit 2
    }
    exit 1
}
