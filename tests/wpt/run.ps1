<#
.SYNOPSIS
    One-file entry point to run WPT tests against Lumen on Windows/PowerShell.

.DESCRIPTION
    Wraps the manual steps from tests/wpt/README.md into a single script:
      1. Creates tests/wpt/.venv (once) and installs tests/wpt/requirements.txt
         if the venv's wptrunner import chain isn't there yet.
      2. Builds target/dev-release/lumen.exe if missing (or if -Build is passed).
      3. Runs one of:
           (default)     run_suite.py   — the curated-subset pass/fail gate (S7)
           -Report       run_report.py  — HTML report for the curated subset
           -All          run_report.py --all [--root X] [-Recursive]
                         — survey every vendored test under -Root (default dom/nodes)
           -AllVendored  run_report.py --all against EVERY vendored category
                         (currently dom/nodes + FileAPI — see docs/wpt-status.md for
                         which categories are vendored at all; the other ~275 upstream
                         categories have no files under tests/wpt/ and can't be run),
                         one HTML report per category

    Run from anywhere; paths are resolved relative to this script's location
    (tests/wpt/), not the caller's working directory.

    Run with NO flags at all (just `tests\wpt\run.ps1`) to get an interactive
    menu instead of the default gate — pass any flag (even just -Build) to
    skip the menu and run non-interactively (scripting/CI).

.PARAMETER Build
    Force a rebuild of lumen-shell (dev-release profile) before running.

.PARAMETER Report
    Run run_report.py (HTML report) instead of the run_suite.py gate.

.PARAMETER All
    Survey every vendored test under -Root instead of just the curated subset.
    Implies -Report (there is no whole-corpus pass/fail gate, only a report).

.PARAMETER AllVendored
    Survey every vendored category (currently dom/nodes + FileAPI), one report
    each. Takes precedence over -All/-Root/-Recursive.

.PARAMETER Root
    Category subdir under tests/wpt/ to scan with -All. Default: dom/nodes.

.PARAMETER Recursive
    With -All, walk -Root recursively and expand .any.js/.window.js templates
    (needed for categories organized into subdirectories, e.g. FileAPI).

.PARAMETER Out
    HTML report path (only used with -Report/-All). Default: .tmp/wpt-report.html.

.EXAMPLE
    tests\wpt\run.ps1
    Runs the curated-subset gate (~20 dom/nodes tests), fails if any result
    is unexpected vs the committed .ini expectations.

.EXAMPLE
    tests\wpt\run.ps1 -Report
    Same tests, but writes .tmp/wpt-report.html instead of gating.

.EXAMPLE
    tests\wpt\run.ps1 -All -Root FileAPI -Recursive
    Surveys the whole vendored FileAPI category, writes .tmp/wpt-report.html.

.EXAMPLE
    tests\wpt\run.ps1 -AllVendored
    Surveys every vendored category (dom/nodes, FileAPI) — the closest thing to
    "run absolutely everything" this repo has, since only these two categories
    are vendored at all. Writes .tmp/wpt-report-dom-nodes.html and
    .tmp/wpt-report-FileAPI.html.
#>

[CmdletBinding()]
param(
    [switch]$Build,
    [switch]$Report,
    [switch]$All,
    [switch]$AllVendored,
    [string]$Root = "dom/nodes",
    [switch]$Recursive,
    [string]$Out = ""
)

# Categories with actual .html files under tests/wpt/ — see docs/wpt-status.md.
# (category subdir, recursive?) — dom/nodes is intentionally flat (its own
# subdirectories are separate never-vetted sub-suites, see run_report.py).
$VendoredCategories = @(
    @{ Root = "dom/nodes"; Recursive = $false },
    @{ Root = "FileAPI";   Recursive = $true }
)

$ErrorActionPreference = "Stop"

$WptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $WptDir "..\..")
$VenvDir = Join-Path $WptDir ".venv"
$VenvPy  = Join-Path $VenvDir "Scripts\python.exe"
$Profile = if ($env:LUMEN_PROFILE) { $env:LUMEN_PROFILE } else { "dev-release" }
$Binary  = Join-Path $RepoRoot "target\$Profile\lumen.exe"

# Interactive menu only when invoked with zero flags (double-click / bare
# `run.ps1`) — any explicit flag skips straight to non-interactive execution
# so scripts/CI invocations behave exactly as before this menu was added.
if ($PSBoundParameters.Count -eq 0) {
    Write-Host ""
    Write-Host "=== Lumen WPT - выбор режима ==="
    Write-Host "  1) Gate: curated-набор (~20 тестов dom/nodes), pass/fail       [по умолчанию]"
    Write-Host "  2) Отчёт (HTML): тот же curated-набор"
    Write-Host "  3) Отчёт (HTML): все вендоренные категории (dom/nodes + FileAPI)"
    Write-Host "  4) Отчёт (HTML): своя категория (--root), с опцией --recursive"
    Write-Host ""
    $choice = Read-Host "Выбор [1]"
    if ([string]::IsNullOrWhiteSpace($choice)) { $choice = "1" }
    switch ($choice) {
        "2" { $Report = $true }
        "3" { $AllVendored = $true }
        "4" {
            $All = $true
            $r = Read-Host "Категория --root [dom/nodes]"
            if (-not [string]::IsNullOrWhiteSpace($r)) { $Root = $r }
            $rec = Read-Host "Рекурсивно? (y/N)"
            if ($rec -match '^(y|yes|д|да)$') { $Recursive = $true }
        }
        default { }
    }
    if (-not (Test-Path $Binary)) {
        Write-Host "lumen.exe ($Profile) не найден - будет собран автоматически."
    } else {
        $rebuild = Read-Host "Пересобрать lumen-shell перед запуском? (y/N)"
        if ($rebuild -match '^(y|yes|д|да)$') { $Build = $true }
    }
    Write-Host ""
}

Push-Location $RepoRoot
try {
    # 1. venv + deps
    if (-not (Test-Path $VenvPy)) {
        Write-Host "== creating venv at $VenvDir =="
        python -m venv $VenvDir
    }
    $importCheckScript = @"
import sys, os
root = r'$RepoRoot'
here = os.path.join(root, 'tools')
sys.path[:0] = [root, here, os.path.join(here, 'wptserve'),
                os.path.join(here, 'webdriver'), os.path.join(here, 'wptrunner')]
import localpaths
import manifest.manifest
from tools.serve import serve
import wptrunner.wptrunner
import wptrunner.wptcommandline
from webdriver.bidi.client import BidiSession
"@
    & $VenvPy -c $importCheckScript *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "== installing tests/wpt/requirements.txt =="
        & $VenvPy -m pip install -r (Join-Path $WptDir "requirements.txt")
        if ($LASTEXITCODE -ne 0) { throw "pip install failed" }
    }

    # 2. lumen-shell build
    if ($Build -or -not (Test-Path $Binary)) {
        Write-Host "== building lumen-shell ($Profile) =="
        cargo build -p lumen-shell --profile $Profile
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    }
    if (-not (Test-Path $Binary)) { throw "lumen binary still not found: $Binary" }

    # 3. run
    $env:LUMEN_PROFILE = $Profile
    if ($AllVendored) {
        $overallRv = 0
        $reportPaths = @()
        foreach ($cat in $VendoredCategories) {
            $slug = $cat.Root -replace '/', '-'
            $outPath = Join-Path $RepoRoot ".tmp\wpt-report-$slug.html"
            $reportArgs = @("--binary", $Binary, "--out", $outPath, "--all", "--root", $cat.Root)
            if ($cat.Recursive) { $reportArgs += "--recursive" }
            Write-Host "== $VenvPy tests/wpt/run_report.py $($reportArgs -join ' ') =="
            & $VenvPy (Join-Path $WptDir "run_report.py") @reportArgs
            if ($LASTEXITCODE -ne 0) { $overallRv = $LASTEXITCODE }
            $reportPaths += $outPath
        }
        Write-Host ""
        Write-Host "Open these files in a browser to see the reports:"
        foreach ($p in $reportPaths) { Write-Host "  $p" }
        exit $overallRv
    } elseif ($All -or $Report) {
        $outPath = if ($Out) { $Out } else { Join-Path $RepoRoot ".tmp\wpt-report.html" }
        $reportArgs = @("--binary", $Binary, "--out", $outPath)
        if ($All) {
            $reportArgs += @("--all", "--root", $Root)
            if ($Recursive) { $reportArgs += "--recursive" }
        }
        Write-Host "== $VenvPy tests/wpt/run_report.py $($reportArgs -join ' ') =="
        & $VenvPy (Join-Path $WptDir "run_report.py") @reportArgs
        $rv = $LASTEXITCODE
        Write-Host ""
        Write-Host "Open this file in a browser to see the report: $outPath"
        exit $rv
    } else {
        Write-Host "== $VenvPy tests/wpt/run_suite.py --binary $Binary =="
        & $VenvPy (Join-Path $WptDir "run_suite.py") --binary $Binary
        $rv = $LASTEXITCODE
        Write-Host ""
        Write-Host "This mode (curated gate) doesn't write an HTML report - results are printed above."
        Write-Host "Re-run with -Report (or -AllVendored) to get an HTML file to open."
        exit $rv
    }
} finally {
    Pop-Location
}
