# SPDX-License-Identifier: Apache-2.0
# Only the completed 059 temporary workspace; never the retained migration instance.
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$expected=Join-Path $repo 'development\fusion\runs\059-real-binding'
$target=(Resolve-Path -LiteralPath $expected).Path
$evidence=Join-Path $repo 'development\fusion\migrations\059'
$audit=Get-Content -LiteralPath (Join-Path $evidence 'final-audit.json') -Raw | ConvertFrom-Json
$report=Get-Content -LiteralPath (Join-Path $evidence 'result.json') -Raw | ConvertFrom-Json
if ($target -ne $expected -or $audit.temporaryRoot -ne $target -or
    $report.status -ne 'PARTIAL_SCOPE_MIGRATED_AND_RESTORE_VERIFIED' -or
    $audit.retained.status -ne 'stopped' -or $audit.restored.status -ne 'stopped' -or
    $audit.restored.provider -ne 0 -or $audit.restored.bootstrap -ne 0 -or $audit.packageProcesses -ne 0) {throw 'CLEANUP_NOT_READY'}
if (((Get-Item -LiteralPath $target).Attributes -band [IO.FileAttributes]::ReparsePoint) -or
    @(Get-ChildItem -LiteralPath $target -Recurse -Force -Attributes ReparsePoint).Count) {throw 'CLEANUP_LINK'}
$package=Join-Path $target 'extracted'
$active=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -and $_.ExecutablePath.StartsWith($package+'\',[StringComparison]::OrdinalIgnoreCase)})
if ($active.Count) {throw 'PACKAGE_STILL_RUNNING'}
Copy-Item -LiteralPath (Join-Path $target 'runs\release-latest.json') -Destination (Join-Path $evidence 'release-tested.json')
$bytes=(Get-ChildItem -LiteralPath $target -Recurse -Force -File | Measure-Object Length -Sum).Sum
Remove-Item -LiteralPath $target -Recurse -Force
$receipt=@{temporaryRemoved=!(Test-Path -LiteralPath $target);logicalBytesRemoved=$bytes;retainedInstance=$audit.retainedInstance;restoredCredentials=0;completed=[DateTime]::UtcNow.ToString('o')}
[IO.File]::WriteAllText((Join-Path $evidence 'cleanup.json'),($receipt | ConvertTo-Json),[Text.UTF8Encoding]::new($false))
Write-Output '059 temporary workspace removed; stopped current migration state retained.'
