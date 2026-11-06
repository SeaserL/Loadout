# SPDX-License-Identifier: Apache-2.0
param([Parameter(Mandatory=$true)][string]$RunRoot,[switch]$PreflightOnly,[switch]$DesktopCandidate,[switch]$RetainCleanupPending)
$ErrorActionPreference='Stop'
if($RetainCleanupPending -and !$DesktopCandidate){throw 'PENDING_CLEANUP_ONLY_FOR_DESKTOP_CANDIDATE'}
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$area=Join-Path $repo 'development\fusion'
$RunRoot=(Resolve-Path -LiteralPath $RunRoot).Path
$script:removedBytes=0L
$spaceBefore=@{};foreach ($drive in @('C','D')) {$spaceBefore[$drive]=([IO.DriveInfo]::new($drive)).AvailableFreeSpace}
if (!$RunRoot.StartsWith((Join-Path $area 'runs')+'\',[StringComparison]::OrdinalIgnoreCase)) {throw 'RUN_SCOPE'}
function Read-Json([string]$p) {Get-Content -LiteralPath $p -Raw -Encoding UTF8 | ConvertFrom-Json}
function Write-Json([string]$p,$v) {[IO.File]::WriteAllText($p,($v | ConvertTo-Json -Depth 40),[Text.UTF8Encoding]::new($false))}
function Remove-Owned([string]$p) {
    if (!(Test-Path -LiteralPath $p)) {return}
    $resolved=(Resolve-Path -LiteralPath $p).Path
    $cache=Join-Path $repo 'development\upstreams\omniroute\.claude\worktrees\loadout-phase11-v52\.build\loadout-fusion'
    $allowed=$resolved -eq $RunRoot -or $resolved -eq $cache -or
        ($script:buildCache -and $resolved -eq $script:buildCache -and $resolved -match '^C:\\loadout-fusion-swc-[a-f0-9-]+$') -or
        ($resolved.EndsWith('.zip') -and ($resolved.StartsWith((Join-Path $repo 'dist\fusion')+'\') -or $resolved.StartsWith((Join-Path $repo 'development\experiments\omniroute-phase17\packages')+'\')))
    $allowed=$allowed -or ($script:oldUnpacked -and $resolved -eq $script:oldUnpacked -and $resolved.StartsWith((Join-Path $area 'runs')+'\') -and (Split-Path -Leaf $resolved) -eq 'extracted') -or ($resolved -eq (Join-Path $repo '.build\fusion-console')) -or ($DesktopCandidate -and $resolved -eq (Join-Path $repo '.build\fusion-desktop-target'))
    if (!$allowed) {throw "CLEANUP_SCOPE: $resolved"}
    $item=Get-Item -LiteralPath $resolved -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {throw 'CLEANUP_LINK'}
    if ($item.PSIsContainer) {
        $links=@(Get-ChildItem -LiteralPath $resolved -Force -Recurse -Attributes ReparsePoint)
        if ($links.Count) {throw 'CLEANUP_CHILD_LINK'}
    }
    $bytes=if($item.PSIsContainer){(Get-ChildItem -LiteralPath $resolved -Force -Recurse -File | Measure-Object Length -Sum).Sum}else{$item.Length}
    Remove-Item -LiteralPath $resolved -Recurse -Force
    $script:removedBytes+=[long]$bytes
}
$release=Read-Json (Join-Path $RunRoot 'runs\release-latest.json')
if (!$DesktopCandidate) {
    $report=Read-Json (Join-Path $RunRoot 'evidence\actual-preflight.json')
    if (!$report.metadataStableDuringRead) {throw 'SOURCE_CHANGED_REPEAT_PREFLIGHT'}
}
if (!$PreflightOnly) {
    $regression=Read-Json (Join-Path $RunRoot 'evidence\regression.json')
    $audit=Read-Json (Join-Path $RunRoot 'evidence\final-audit.json')
    if (!$regression.complete -or $regression.passed -ne 292 -or $regression.releaseId -ne $release.releaseId -or
        $audit.releaseId -ne $release.releaseId -or $audit.remaining -ne 0 -or $audit.active -ne 0 -or @($audit.sourceMismatch).Count -ne 0) {throw 'ACCEPTANCE_FAILED'}
    if ((Get-FileHash -LiteralPath $release.archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $release.archiveSha256) {throw 'ARCHIVE_HASH'}
    $uiReviewPath=Join-Path $RunRoot $(if($DesktopCandidate){'evidence\desktop-review.json'}else{'evidence\ui-review.json'})
    if (!(Test-Path -LiteralPath $uiReviewPath)) {throw 'UI_INTERACTION_REVIEW_REQUIRED'}
    $uiReview=Read-Json $uiReviewPath
    if ($uiReview.releaseId -ne $release.releaseId) {throw 'UI_REVIEW_RELEASE_MISMATCH'}
    if ($DesktopCandidate) {
        if ($uiReview.status -notin @('passed','blocked') -or !$uiReview.manualStepsDocument) {throw 'DESKTOP_REVIEW_REQUIRED'}
    } elseif (!$uiReview.complete) {throw 'UI_INTERACTION_REVIEW_INCOMPLETE'}
}
$evidence=Join-Path $area ('evidence\'+(Split-Path -Leaf $RunRoot))
New-Item -ItemType Directory -Force -Path $evidence | Out-Null
Copy-Item -LiteralPath (Join-Path $RunRoot 'evidence') -Destination $evidence -Recurse
Copy-Item -LiteralPath (Join-Path $RunRoot 'runs\release-latest.json') -Destination (Join-Path $evidence 'release-tested.json')
if (!$PreflightOnly) {
    $build=Read-Json (Join-Path $RunRoot 'runs\build-latest-v52.json')
    $script:buildCache=$build.cache
    foreach ($name in @('result.json','build.log')) {if(Test-Path -LiteralPath (Join-Path $build.run $name)){Copy-Item -LiteralPath (Join-Path $build.run $name) -Destination (Join-Path $evidence ('build-'+$name))}}
    Copy-Item -LiteralPath (Join-Path $RunRoot 'runs\build-latest-v52.json') -Destination (Join-Path $evidence 'build-provenance.json')
    $old=Read-Json (Join-Path $RunRoot 'runs\old-baseline.json')
    if (!$DesktopCandidate -and !$old.directory.StartsWith($RunRoot+'\')) {$script:oldUnpacked=(Resolve-Path -LiteralPath $old.directory).Path}
    Copy-Item -LiteralPath (Join-Path $RunRoot 'runs\old-baseline.json') -Destination (Join-Path $evidence 'upgrade-baseline.json')
    $dist=Join-Path $repo 'dist\fusion';New-Item -ItemType Directory -Force -Path $dist | Out-Null
    $target=Join-Path $dist ($release.releaseId+'.zip')
    if (Test-Path -LiteralPath $target) {throw 'CANDIDATE_EXISTS'}
    $archive=(Resolve-Path -LiteralPath $release.archive).Path
    if (!$archive.StartsWith($RunRoot+'\') -or !$target.StartsWith($dist+'\')) {throw 'PROMOTION_SCOPE'}
    Move-Item -LiteralPath $archive -Destination $target
    # 1.1 (ExecPlan 071): the lite app and the AI pack travel with the full archive.
    $liteTarget=Join-Path $dist ($release.releaseId+'-lite.zip')
    $packTarget=Join-Path $dist ($release.releaseId+'-ai-'+$release.packVersion+'.zip')
    foreach ($pair in @(@($release.liteArchive,$liteTarget,$release.liteSha256),@($release.packArchive,$packTarget,$release.packSha256))) {
        $from=(Resolve-Path -LiteralPath $pair[0]).Path
        if (!$from.StartsWith($RunRoot+'\') -or (Test-Path -LiteralPath $pair[1])) {throw 'PROMOTION_SCOPE'}
        if ((Get-FileHash -LiteralPath $from -Algorithm SHA256).Hash.ToLowerInvariant() -ne $pair[2]) {throw 'ARCHIVE_HASH'}
        Move-Item -LiteralPath $from -Destination $pair[1]
    }
    $current=[ordered]@{releaseId=$release.releaseId;treeHash=$release.treeHash;archive=$target;archiveSha256=$release.archiveSha256;bytes=$release.bytes;files=$release.files;evidence=$evidence;profile='067.1';activationReady=$false;liteArchive=$liteTarget;liteSha256=$release.liteSha256;liteBytes=$release.liteBytes;packArchive=$packTarget;packSha256=$release.packSha256;packBytes=$release.packBytes;packVersion=$release.packVersion}
    if($DesktopCandidate){$current.desktopAcceptance=$uiReview.status;$current.runtime=$release.extracted;$current.actualMigrationRepeated=$false}
    if($RetainCleanupPending){$current.cleanupPending=$true;$current.cleanupScript=(Join-Path $repo 'scripts\fusion\cleanup-superseded.ps1')}
    Write-Json (Join-Path $area 'current.json') $current
    if(!$RetainCleanupPending){
    Remove-Owned $old.archive
    foreach ($p in @($old.liteArchive,$old.packArchive)) {if ($p) {Remove-Owned $p}}
    if($build.mode -ne 'verified-compiled-backend-reuse') {
        Remove-Owned (Join-Path $build.work '.build\loadout-fusion')
        Remove-Owned $script:buildCache
    }
    if ($script:oldUnpacked) {Remove-Owned $script:oldUnpacked}
    Remove-Owned (Join-Path $repo '.build\fusion-console')
    if($DesktopCandidate){Remove-Owned (Join-Path $repo '.build\fusion-desktop-target')}
    }
}
if(!$DesktopCandidate){Remove-Owned $RunRoot}
$spaceAfter=@{};$spaceDelta=@{};foreach ($drive in @('C','D')) {$spaceAfter[$drive]=([IO.DriveInfo]::new($drive)).AvailableFreeSpace;$spaceDelta[$drive]=$spaceAfter[$drive]-$spaceBefore[$drive]}
Write-Json (Join-Path $evidence 'cleanup.json') @{runRemoved=!(Test-Path -LiteralPath $RunRoot);preflightOnly=[bool]$PreflightOnly;recorded=[DateTime]::UtcNow.ToString('o');cleanupPending=[bool]$RetainCleanupPending;candidate=(Join-Path $area 'current.json');removedFileBytes=$script:removedBytes;volumeFreeDeltaBytes=$spaceDelta;volumeDeltaIncludesOtherSystemActivity=$true}
Write-Host "Evidence: $evidence"
