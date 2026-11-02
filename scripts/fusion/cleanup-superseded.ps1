# SPDX-License-Identifier: Apache-2.0
# Optional, run by hand: removes candidates and work folders that the current package replaced.
# Keeps the current package, all source, evidence, isolated data, credentials and the installed app.
[CmdletBinding()]
param([switch]$WhatIf)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
if ($repo -ne 'D:\Loadout') { throw 'WORKSPACE_SCOPE' }
$current = Get-Content -LiteralPath (Join-Path $repo 'development\fusion\current.json') -Raw | ConvertFrom-Json
$keep = [IO.Path]::GetFullPath($current.runtime)
$runs = Join-Path $repo 'development\fusion\runs'
$targets = @(
    (Join-Path $repo 'development\fusion\runtime\current'),
    (Join-Path $repo '.build\fusion-desktop')
) + @(Get-ChildItem -LiteralPath (Join-Path $repo 'dist\fusion') -File -Filter '*.zip' | Where-Object { $_.FullName -ne $current.archive } | ForEach-Object { $_.FullName }
) + @(Get-ChildItem -LiteralPath $runs -Directory | Where-Object { $_.Name -ne '067-product' } | ForEach-Object { $_.FullName })
$product = Join-Path $runs '067-product'
if (Test-Path -LiteralPath $product) {
    $targets += @(Get-ChildItem -LiteralPath $product -Directory | Where-Object { $_.Name -in @('baseline', 'packages', 'build') } | ForEach-Object { $_.FullName })
}
$running = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($runs + '\', [StringComparison]::OrdinalIgnoreCase) })
if ($running.Count) { throw "Still running from the work folders: $($running.ProcessId -join ', '). Stop them first." }
$total = 0L
foreach ($target in $targets) {
    if (!(Test-Path -LiteralPath $target)) { continue }
    $full = (Resolve-Path -LiteralPath $target).Path
    if ($full -ne $target -or !$full.StartsWith($repo + '\') -or $keep.StartsWith($full + '\') -or $full -eq $keep -or $full -eq $current.archive) { throw "CLEANUP_SCOPE: $full" }
    $item = Get-Item -LiteralPath $full -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "CLEANUP_LINK: $full" }
    $bytes = if ($item.PSIsContainer) { (Get-ChildItem -LiteralPath $full -Force -Recurse -File | Measure-Object Length -Sum).Sum } else { $item.Length }
    $total += [long]$bytes
    if ($WhatIf) { Write-Host ('would remove {0,8:N2} GiB  {1}' -f ($bytes / 1GB), $full) }
    else { Remove-Item -LiteralPath $full -Recurse -Force; Write-Host "removed $full" }
}
Write-Host ('{0} {1:N2} GiB of files.' -f $(if ($WhatIf) { 'Would free' } else { 'Freed' }), ($total / 1GB))
