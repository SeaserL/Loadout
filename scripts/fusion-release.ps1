# SPDX-License-Identifier: Apache-2.0
[CmdletBinding()]
param(
    [ValidateSet('release','verify','preflight','desktop','package')][string]$Action = 'release',
    [string]$SourceDb = "$env:LOCALAPPDATA\Loadout\data\loadout.db",
    [string]$Node,
    [string]$Python = 'python',
    [string]$RunRoot
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$area = Join-Path $repo 'development\fusion'
New-Item -ItemType Directory -Force -Path $area | Out-Null
$lock = [IO.File]::Open((Join-Path $area 'release.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
$priorRoot=$env:FUSION_RELEASE_ROOT
$priorCache=$env:FUSION_SWC_CACHE
$oldLocation=Get-Location
function Invoke-Checked([string]$Exe, [string[]]$Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Release step failed: $($Arguments[0]) (exit $LASTEXITCODE)" }
}
try {
    Set-Location -LiteralPath $repo
    if (!$Node) {
        Invoke-Checked $Python @('scripts/fusion/runtime.py')
        $Node=Join-Path $area 'tools\node.exe'
    }
    if ((& $Node --version) -ne 'v24.22.1') {throw 'Use -Node <path to Node v24.22.1>; this release pins the runtime and its license.'}
    if (!$RunRoot) { $RunRoot=Join-Path $area ('runs\'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfff')+'-'+[guid]::NewGuid().ToString('N').Substring(0,8)) }
    $RunRoot=[IO.Path]::GetFullPath($RunRoot)
    if (!$RunRoot.StartsWith((Join-Path $area 'runs')+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'RUN_SCOPE' }
    if ($Action -in @('release','desktop','package') -and (Test-Path -LiteralPath $RunRoot)) { throw 'RELEASE_REQUIRES_FRESH_RUN' }
    if ($Action -eq 'verify' -and !(Test-Path -LiteralPath (Join-Path $RunRoot 'runs\release-latest.json'))) { throw 'VERIFY_REQUIRES_EXISTING_RUN' }
    New-Item -ItemType Directory -Force -Path (Join-Path $RunRoot 'runs'),(Join-Path $RunRoot 'evidence') | Out-Null
    $env:FUSION_RELEASE_ROOT=$RunRoot
    Write-Host "Release workspace: $RunRoot"
    if ($Action -eq 'desktop') {
        & (Join-Path $PSScriptRoot 'fusion\build-desktop.ps1')
        $baseline=Join-Path $area 'runtime\current'
        Invoke-Checked (Join-Path $baseline 'node.exe') @((Join-Path $baseline 'tools\cli.mjs'),'verify')
        $old=Get-Content (Join-Path $area 'current.json') -Raw | ConvertFrom-Json
        $manifest=Get-Content (Join-Path $baseline 'fusion-manifest.json') -Raw | ConvertFrom-Json
        if ($manifest.releaseId -ne $old.releaseId -or $old.profile -notin @('064.1','066.1')) {throw 'DESKTOP_REFRESH_BASELINE'}
        $old | Add-Member -NotePropertyName directory -NotePropertyValue $baseline -Force
        [IO.File]::WriteAllText((Join-Path $RunRoot 'runs\old-baseline.json'),($old | ConvertTo-Json -Depth 20))
        $stage=Join-Path $RunRoot 'packages\desktop'
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $stage),(Join-Path $RunRoot 'build') | Out-Null
        Copy-Item -LiteralPath $baseline -Destination $stage -Recurse
        Copy-Item -LiteralPath (Join-Path $old.evidence 'build-result.json') -Destination (Join-Path $RunRoot 'build\result.json')
        [IO.File]::WriteAllText((Join-Path $RunRoot 'runs\build-latest-v52.json'),(@{run=(Join-Path $RunRoot 'build');mode='verified-compiled-backend-reuse';originalEvidence=$old.evidence} | ConvertTo-Json))
        try {
            $env:FUSION_ASSEMBLE_IN_PLACE='1';$env:FUSION_ASSEMBLE_DIRECTORY=$stage;$env:FUSION_REFRESH_FROM=$old.evidence
            Invoke-Checked $Node @('scripts/fusion/assemble.mjs')
        } finally {
            Remove-Item Env:FUSION_ASSEMBLE_IN_PLACE,Env:FUSION_ASSEMBLE_DIRECTORY,Env:FUSION_REFRESH_FROM -ErrorAction SilentlyContinue
        }
        Invoke-Checked $Python @('scripts/fusion/archive.py','archive')
        Invoke-Checked $Node @('scripts/fusion/run-suite.mjs')
        Invoke-Checked $Node @('scripts/fusion/audit.mjs')
        Write-Host 'Desktop package regression complete. Native window/tray acceptance and candidate promotion remain explicit final steps.'
        return
    }
    if ($Action -in @('release','package')) {
        & (Join-Path $PSScriptRoot 'fusion\build-desktop.ps1')
        # The upgrade baseline only exists where an earlier candidate does; a plain package needs none.
        if ($Action -eq 'release') { Invoke-Checked $Python @('scripts/fusion/archive.py','baseline') }
        $cache='C:\loadout-fusion-swc-'+[guid]::NewGuid().ToString()
        New-Item -ItemType Directory -Path $cache | Out-Null
        $acl=New-Object Security.AccessControl.DirectorySecurity
        $acl.SetAccessRuleProtection($true,$false)
        foreach ($sid in @([Security.Principal.WindowsIdentity]::GetCurrent().User,[Security.Principal.SecurityIdentifier]::new('S-1-5-18'),[Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))) {
            $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))
        }
        Set-Acl -LiteralPath $cache -AclObject $acl
        $env:FUSION_SWC_CACHE=$cache
        Invoke-Checked $Node @('scripts/fusion/build.mjs')
        Invoke-Checked $Node @('scripts/fusion/assemble.mjs')
        if ($Action -eq 'package') {
            $receipt = Get-Content (Join-Path $RunRoot 'runs\package-latest-v52.json') -Raw | ConvertFrom-Json
            Write-Host ('Package: ' + $receipt.directory + '  (start it with Start-Loadout.cmd)')
            return
        }
        Invoke-Checked $Python @('scripts/fusion/archive.py','archive')
    }
    if ($Action -eq 'preflight') { Invoke-Checked $Python @('scripts/fusion/archive.py','current') }
    if ($Action -ne 'preflight') {
        Invoke-Checked $Node @('scripts/fusion/run-suite.mjs')
        Invoke-Checked $Node @('scripts/fusion/audit.mjs')
    }
    Invoke-Checked $Node @('scripts/fusion/preflight.mjs',$SourceDb)
    & (Join-Path $PSScriptRoot 'fusion\finish.ps1') -RunRoot $RunRoot -PreflightOnly:($Action -eq 'preflight')
} finally {
    $env:FUSION_RELEASE_ROOT=$priorRoot
    $env:FUSION_SWC_CACHE=$priorCache
    Set-Location -LiteralPath $oldLocation
    $lock.Dispose()
}
