<#
.SYNOPSIS
    Checks release-notes.ps1 against small CHANGELOGs: a section with text is written, an empty
    one (followed by the next heading, by only blank lines or by the end of the file) fails.
    check.ps1 runs it.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'release-notes.ps1'
# The same PowerShell as this one, so Windows PowerShell and pwsh both test themselves.
$shell = (Get-Process -Id $PID).Path
$folder = Join-Path ([System.IO.Path]::GetTempPath()) "neumann-control-release-notes-$PID"
New-Item -ItemType Directory -Force -Path $folder | Out-Null

$cases = @(
    @{ Name = 'a section with text'; Passes = $true
       Text = "# Changelog`n`n## [0.3.0] - 2026-11-01`n`n- A change.`n`n## [0.2.0] - 2026-10-27`n`n- Older.`n" }
    @{ Name = 'a section followed by the next heading'; Passes = $false
       Text = "# Changelog`n`n## [0.3.0] - 2026-11-01`n## [0.2.0] - 2026-10-27`n`n- Older.`n" }
    @{ Name = 'a section of blank lines'; Passes = $false
       Text = "# Changelog`n`n## [0.3.0] - 2026-11-01`n`n   `n`n## [0.2.0] - 2026-10-27`n`n- Older.`n" }
    @{ Name = 'a section at the end of the file'; Passes = $false
       Text = "# Changelog`n`n## [0.2.0] - 2026-10-27`n`n- Older.`n`n## [0.3.0] - 2026-11-01" }
    @{ Name = 'no section'; Passes = $false
       Text = "# Changelog`n`n## [0.2.0] - 2026-10-27`n`n- Older.`n" }
)

$failed = 0
foreach ($case in $cases) {
    $changelog = Join-Path $folder 'CHANGELOG.md'
    $notes = Join-Path $folder 'notes.md'
    [System.IO.File]::WriteAllText($changelog, $case.Text)
    Remove-Item $notes -ErrorAction SilentlyContinue
    & $shell -NoProfile -File $script -Version '0.3.0' -OutFile $notes -Changelog $changelog *> $null
    $passed = $LASTEXITCODE -eq 0
    $text = if (Test-Path $notes) { [System.IO.File]::ReadAllText($notes) } else { '' }
    $right = if ($case.Passes) { $passed -and $text -eq "- A change.`n" } else { -not $passed }
    if (-not $right) {
        Write-Host "FAILED: $($case.Name) (exit $LASTEXITCODE, notes '$text')" -ForegroundColor Red
        $failed++
    }
}
Remove-Item $folder -Recurse -Force
if ($failed) { exit 1 }
Write-Host "release-notes.ps1: $($cases.Count) cases right"
