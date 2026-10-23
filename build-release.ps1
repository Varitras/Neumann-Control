<#
.SYNOPSIS
    Builds a release binary and drops it into <OutputRoot>\neumann-control-<version>\. The
    version is the one in the workspace Cargo.toml; OutputRoot defaults to release\ in the
    repository. A release folder that already holds files is refused unless -Force is given.
#>
[CmdletBinding()]
param(
    [string] $OutputRoot = 'release',
    [switch] $Force
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

# The one list of what a release folder holds; anything else in it fails the build.
$releaseFiles = @('neumann-control.exe', 'neumann-control-manual.html', 'LICENSE', 'THIRD-PARTY-LICENSES.html', 'SHA256SUMS')
# Only the version line of [workspace.package]: a plain match would also hit a dependency
# pinned with `version = "..."`.
$workspaceVersionLine = [regex]'(\[workspace\.package\][^\[]*?\nversion = ")([^"]+)(")'

function Invoke-Step {
    param([string] $Name, [scriptblock] $Command)
    Write-Host ""
    Write-Host "=== $Name" -ForegroundColor Cyan
    $global:LASTEXITCODE = 0
    & $Command
    if ($LASTEXITCODE -ne 0) {
        Write-Host ""
        Write-Host "FAILED: $Name" -ForegroundColor Red
        exit 1
    }
}

function Assert-Tool {
    param([string] $Name, [string] $InstallHint)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        Write-Host "FAILED: $Name is not installed. $InstallHint" -ForegroundColor Red
        exit 1
    }
}

function Assert-CleanTree {
    $status = git status --porcelain
    # A git that cannot run prints nothing, which would otherwise read as a clean tree.
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: git status did not run; cannot prove the release maps to a commit" -ForegroundColor Red
        exit 1
    }
    if ($status) {
        Write-Host "FAILED: working tree is dirty; a release must map to a commit" -ForegroundColor Red
        exit 1
    }
}

function Get-WorkspaceVersion {
    $match = $workspaceVersionLine.Match([System.IO.File]::ReadAllText((Resolve-Path 'Cargo.toml')))
    if (-not $match.Success) { throw 'could not find the [workspace.package] version in Cargo.toml' }
    $match.Groups[2].Value
}

Assert-Tool 'cargo-about' 'Install with: cargo install cargo-about --locked'
Assert-CleanTree

$version = Get-WorkspaceVersion

# Checked before the gate: a rerun must not silently replace an exe already built.
# Absolute: .NET file calls resolve relative paths against the process directory, not Set-Location.
$root = if ([System.IO.Path]::IsPathRooted($OutputRoot)) { $OutputRoot } else { Join-Path $PSScriptRoot $OutputRoot }
# Only the default root (ignored by git) is created; a given root must exist, so a typo does
# not invent a folder somewhere.
if ($OutputRoot -eq 'release') { New-Item -ItemType Directory -Force -Path $root | Out-Null }
if (-not (Test-Path -LiteralPath $root -PathType Container)) {
    Write-Host "FAILED: output root $root does not exist" -ForegroundColor Red
    exit 1
}
$releaseDirectory = Join-Path (Resolve-Path -LiteralPath $root).Path "neumann-control-$version"
$holdsEarlierBuild = (Test-Path -LiteralPath $releaseDirectory) -and @(Get-ChildItem -LiteralPath $releaseDirectory -Force).Count -gt 0
if ($holdsEarlierBuild -and -not $Force) {
    Write-Host "FAILED: $releaseDirectory already holds files; pass -Force to overwrite them" -ForegroundColor Red
    exit 1
}

Invoke-Step 'Quality gates' { & (Join-Path $PSScriptRoot 'check.ps1') }

Invoke-Step 'Build release binary' { cargo build --release --locked -p neumann-control-app }

New-Item -ItemType Directory -Force -Path $releaseDirectory | Out-Null
Copy-Item 'target\release\neumann-control.exe' $releaseDirectory
Copy-Item 'LICENSE' $releaseDirectory

# The app opens this file from its own folder (help.rs names it the same).
$manualPath = Join-Path $releaseDirectory 'neumann-control-manual.html'
Invoke-Step 'Generate the manual' {
    cargo run --release --locked -q -p neumann-control-manual -- README.md $manualPath
}

$licensesPath = Join-Path $releaseDirectory 'THIRD-PARTY-LICENSES.html'
Invoke-Step 'Generate third-party licenses' {
    # The app's graph only: the manual generator is not part of what ships.
    cargo about generate -m crates\app\Cargo.toml --locked --fail -c about.toml about.hbs -o $licensesPath
}

$sumsPath = Join-Path $releaseDirectory 'SHA256SUMS'
$sumLines = $releaseFiles |
    Where-Object { $_ -ne 'SHA256SUMS' } |
    ForEach-Object { "$((Get-FileHash (Join-Path $releaseDirectory $_) -Algorithm SHA256).Hash.ToLower())  $_" }
# Plain LF and no byte order mark: `sha256sum -c` reads the list on any platform,
# while Set-Content would write CRLF and -Encoding utf8 a BOM on PowerShell 5.1.
[System.IO.File]::WriteAllText($sumsPath, (($sumLines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding $false))

# With -Force the folder is reused, so a stray file from earlier would ship unnoticed.
$present = @(Get-ChildItem -LiteralPath $releaseDirectory -Force | ForEach-Object { $_.Name })
$unexpected = @($present | Where-Object { $releaseFiles -notcontains $_ })
$missing = @($releaseFiles | Where-Object { $present -notcontains $_ })
if ($unexpected -or $missing) {
    $unexpected | ForEach-Object { Write-Host "  unexpected: $_" }
    $missing | ForEach-Object { Write-Host "  missing: $_" }
    Write-Host "FAILED: $releaseDirectory does not hold exactly $($releaseFiles -join ', ')" -ForegroundColor Red
    exit 1
}
Assert-CleanTree

Write-Host ""
Write-Host "Release written to $releaseDirectory" -ForegroundColor Green
