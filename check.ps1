<#
.SYNOPSIS
    Definition of done, as one command. CI runs this same script.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

function Invoke-Gate {
    param([string] $Name, [scriptblock] $Command)
    Write-Host ""
    Write-Host "=== $Name" -ForegroundColor Cyan
    # A block without a native command would leave the exit code of an earlier step in place.
    $global:LASTEXITCODE = 0
    & $Command
    if ($LASTEXITCODE -ne 0) {
        Write-Host ""
        Write-Host "FAILED: $Name" -ForegroundColor Red
        exit 1
    }
}

# Fails when the search prints anything; git grep exits 1 for "no match", which is the pass.
function Invoke-Forbid {
    param([string] $Name, [scriptblock] $Search)
    Write-Host ""
    Write-Host "=== $Name" -ForegroundColor Cyan
    $global:LASTEXITCODE = 0
    $found = & $Search
    if ($LASTEXITCODE -gt 1) {
        Write-Host "FAILED: $Name could not search" -ForegroundColor Red
        exit 1
    }
    if ($found) {
        $found | Out-Host
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

# Splits Rust source into comments, raw strings, character literals and normal string literals,
# leftmost first, so a quote or brace inside one never starts another. Only normal string literals
# are format strings, so the width guard looks at those alone.
$rustToken = [regex]('(?<comment>//[^\n]*|/\*[\s\S]*?\*/)' +
    '|(?<![\w])(?<raw>b?r(?<hashes>#*)"[\s\S]*?"\k<hashes>)' +
    '|(?<character>''(?:\\u\{[0-9a-fA-F]+\}|\\.|[^\\\n''])'')' +
    '|(?<string>"(?:\\[\s\S]|[^"\\])*")')

# Each normal string literal in tracked and new Rust files that matches $Pattern, with its line.
function Find-InRustStrings {
    param([regex] $Pattern)
    foreach ($path in git ls-files --cached --others --exclude-standard -- '*.rs') {
        $content = Get-Content -Raw -LiteralPath $path -Encoding utf8
        if (-not $content) { continue }
        foreach ($token in $rustToken.Matches($content)) {
            $literal = $token.Groups['string']
            if (-not $literal.Success -or -not $Pattern.IsMatch($literal.Value)) { continue }
            $line = ($content.Substring(0, $literal.Index) -split "`n").Count
            "${path}:${line}: $($literal.Value)"
        }
    }
}

Assert-Tool 'cargo-audit' 'Install with: cargo install cargo-audit --locked'
Assert-Tool 'cargo-about' 'Install with: cargo install cargo-about --locked'

# A width or precision taken from a variable panics above u16::MAX (Rust 1.87+); pad by building.
# The argument name before `:` excludes escaped JSON such as "{\"a\":\"b$\"}".
Invoke-Forbid 'runtime format widths' {
    Find-InRustStrings ('\{[A-Za-z0-9_]*:[^{}"\\]*?(?:[a-z_][a-z0-9_]*|[0-9]+)\$[^{}]*\}' +
        '|\{[A-Za-z0-9_]*:[^{}"\\]*\.\*[^{}]*\}')
}
# The index is what gets committed; -text paths (fixtures) skip LF normalisation, so a CR there
# would reach a blob unchanged.
Invoke-Forbid 'carriage returns in staged text files' {
    git grep --cached -nIP '\r'
}
# Catches a stray CR in new and unstaged files before they are added. A CR at a line end is a
# CRLF ending, which git normalises to LF on add outside the -text paths.
Invoke-Forbid 'carriage returns inside lines of working files' {
    git grep -nIP --untracked '\r(?!$)'
}

Invoke-Gate 'cargo fmt (check)' { cargo fmt --all -- --check }
# A dependency with an unaccepted licence (e.g. a GPL crate) fails generation, so this doubles
# as the licence gate; build-release.ps1 generates the shipped file from the same config.
Invoke-Gate 'licence gate (cargo about)' {
    $aboutCheck = Join-Path ([System.IO.Path]::GetTempPath()) 'neumann-control-about-check.html'
    cargo about generate --workspace --locked --fail -c about.toml about.hbs -o $aboutCheck
}
Invoke-Gate 'cargo audit'  { cargo audit }
Invoke-Gate 'cargo clippy' { cargo clippy --workspace --all-targets --all-features --locked -- -D warnings }
Invoke-Gate 'cargo test'   { cargo test --workspace --all-features --locked }

Write-Host ""
Write-Host "All gates green." -ForegroundColor Green
