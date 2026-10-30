<#
.SYNOPSIS
    Writes the CHANGELOG section of one version (`## [<version>] - <date>`, up to the next
    `## [`) to OutFile, for the GitHub release. Fails when the CHANGELOG has no such section or
    it is empty: a release must say what changed.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $OutFile,
    [string] $Changelog = 'CHANGELOG.md'
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$lines = [System.IO.File]::ReadAllLines((Resolve-Path $Changelog))
$heading = "## [$Version]"
$start = [Array]::FindIndex($lines, [Predicate[string]] { param($line) $line.StartsWith($heading) })
if ($start -lt 0) {
    Write-Host "FAILED: CHANGELOG.md has no section $heading" -ForegroundColor Red
    exit 1
}
$end = [Array]::FindIndex($lines, $start + 1, [Predicate[string]] { param($line) $line.StartsWith('## [') })
if ($end -lt 0) { $end = $lines.Length }
# PowerShell counts a range down when its end lies before its start: without lines between, the
# headings themselves would become the section.
$body = if ($end -gt $start + 1) { $lines[($start + 1)..($end - 1)] } else { @() }
$section = ($body -join "`n").Trim()
if (-not $section) {
    Write-Host "FAILED: the CHANGELOG section $heading is empty" -ForegroundColor Red
    exit 1
}
[System.IO.File]::WriteAllText($OutFile, $section + "`n", (New-Object System.Text.UTF8Encoding $false))
Write-Host "Release notes for $Version written to $OutFile"
