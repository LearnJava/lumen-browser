# Runner for the "structural display-list hash" experiment (p1-exp-wgpu-only).
#
# Collects every artefact of one full BEFORE/AFTER cycle into .tmp\exp-hash\,
# one file per step, nothing overwritten inside a run. The measure_idle.ps1
# stderr sink (.tmp\idle_stderr.log) is truncated on every launch, so this
# script copies it out immediately after each run.
#
# Usage (from anywhere):
#   powershell -ExecutionPolicy Bypass -File scripts\exp\run_hash_experiment.ps1
#
# Steps 1-2 are gates: if clippy or the unit tests fail, the script stops
# BEFORE touching git stash, so the working tree is never left in a half state.

param(
    # Skip the BEFORE half (steps 4-6). Use when the baseline is already
    # recorded and only the AFTER numbers need a re-run.
    [switch]$SkipBefore,
    # Page under test. Must match between BEFORE and AFTER.
    [string]$Page = "graphic_tests\1000000-final.html"
)

# NOT "Stop": cargo and git write progress to stderr, `*>&1` turns those lines
# into ErrorRecords, and under "Stop" the first `Checking lumen-paint...` line
# would abort the script. Success/failure is decided by $LASTEXITCODE below,
# which is what actually matters for a native command.
$ErrorActionPreference = "Continue"
# PS 7.3+: keep native stderr out of the error-action machinery entirely.
$PSNativeCommandUseErrorActionPreference = $false

# Repo root = two levels up from scripts\exp\. All relative paths below (and
# inside measure_idle.ps1) resolve against it.
$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
if (-not $Root) { throw "cannot resolve repo root from $PSScriptRoot" }
Set-Location $Root

$OutDir = Join-Path $Root ".tmp\exp-hash"
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Root ".tmp") | Out-Null

$FrameSink = Join-Path $Root ".tmp\idle_stderr.log"
$Summary = Join-Path $OutDir "00-summary.md"
$Results = [ordered]@{}

function Write-Header($text) {
    Write-Host ""
    Write-Host "=== $text ===" -ForegroundColor Cyan
}

# Runs a command, tees stdout+stderr into $LogName, records the exit code.
# Tee output goes to the host, NOT to the pipeline, so the only value this
# function emits is the exit code.
function Invoke-Step($Name, $LogName, [scriptblock]$Cmd) {
    Write-Header $Name
    $log = Join-Path $OutDir $LogName
    & $Cmd *>&1 | Tee-Object -FilePath $log | Out-Host
    $code = $LASTEXITCODE
    $Results[$Name] = [pscustomobject]@{ Log = $LogName; ExitCode = $code }
    return $code
}

# measure_idle.ps1 prints the CPU/WS/private table to stdout and redirects the
# browser's stderr (the [frame] phase log) into $FrameSink, truncating it each
# launch. Capture both, under distinct names.
function Invoke-Measure($Phase) {
    Write-Header "measure idle ($Phase)"
    $stdoutLog = Join-Path $OutDir "$Phase-idle.log"
    $frameLog = Join-Path $OutDir "$Phase-frame.log"

    & powershell -ExecutionPolicy Bypass -File (Join-Path $Root "scripts\exp\measure_idle.ps1") `
        -Page $Page -FrameLog 2 *>&1 | Tee-Object -FilePath $stdoutLog | Out-Host
    $code = $LASTEXITCODE

    if (Test-Path $FrameSink) {
        Copy-Item $FrameSink $frameLog -Force
    } else {
        "measure_idle.ps1 produced no stderr sink at $FrameSink" | Set-Content $frameLog
    }
    $Results["measure idle ($Phase)"] = [pscustomobject]@{ Log = "$Phase-idle.log + $Phase-frame.log"; ExitCode = $code }
    return $code
}

# ── Gates: compile + unit tests. Never stash a tree that does not build. ──

if ((Invoke-Step "clippy lumen-paint" "01-clippy.log" {
    cargo clippy -p lumen-paint --all-targets -- -D warnings
}) -ne 0) {
    Write-Host "clippy failed - see .tmp\exp-hash\01-clippy.log. Nothing stashed." -ForegroundColor Red
    exit 1
}

# --lib: the hash tests live in the crate's unit-test module. Building the
# integration test binaries here buys nothing and drags in a full wgpu link.
if ((Invoke-Step "unit tests (hash_*)" "02-test-hash.log" {
    cargo test -p lumen-paint --lib hash_
}) -ne 0) {
    Write-Host "tests failed - see .tmp\exp-hash\02-test-hash.log. Nothing stashed." -ForegroundColor Red
    exit 1
}

# ── Isolated BEFORE/AFTER of the hash fold itself (both in one process). ──

Invoke-Step "hash bench (isolated)" "03-bench-hash.log" {
    cargo test -p lumen-paint --release --lib hash_display_list_bench -- --ignored --nocapture
} | Out-Null

# ── BEFORE: stash the patch, build, measure, restore. ──

if (-not $SkipBefore) {
    Write-Header "git stash push"

    # Guard: never run `git stash pop` unless this script created the entry.
    # An empty tree would otherwise pop a stash that belongs to someone else.
    $dirty = git status --porcelain -- crates subsystems
    if (-not $dirty) {
        Write-Host "No tracked modifications under crates/ or subsystems/." -ForegroundColor Yellow
        Write-Host "The patch is not in the tree, so there is no BEFORE to measure." -ForegroundColor Red
        Write-Host "Check 'git status', or rerun with -SkipBefore." -ForegroundColor Red
        exit 1
    }

    $stashBefore = (git stash list | Measure-Object -Line).Lines
    git stash push -m "exp-hash-baseline" -- crates subsystems 2>&1 |
        Tee-Object -FilePath (Join-Path $OutDir "04-git-stash.log") | Out-Host
    $stashAfter = (git stash list | Measure-Object -Line).Lines

    if ($stashAfter -le $stashBefore) {
        Write-Host "git stash push created no entry - aborting before the build." -ForegroundColor Red
        exit 1
    }

    try {
        Invoke-Step "build shell (before)" "05-build-before.log" {
            cargo build --profile dev-release -p lumen-shell
        } | Out-Null
        Invoke-Measure "06-before" | Out-Null
    } finally {
        # Always restore the patch, even if the build or the measurement blew up.
        Write-Header "git stash pop"
        git stash pop 2>&1 | Tee-Object -FilePath (Join-Path $OutDir "07-git-stash-pop.log")
    }
}

# ── AFTER: rebuild with the patch, measure again. ──

Invoke-Step "build shell (after)" "08-build-after.log" {
    cargo build --profile dev-release -p lumen-shell
} | Out-Null
Invoke-Measure "09-after" | Out-Null

# ── Manifest: what was run, where it landed, whether it passed. ──

$lines = @()
$lines += "# Structural display-list hash - experiment run"
$lines += ""
$lines += "- Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
$lines += "- Branch: $(git rev-parse --abbrev-ref HEAD)"
$lines += "- HEAD: $(git rev-parse --short HEAD)"
$lines += "- Page: $Page"
$lines += "- SkipBefore: $SkipBefore"
$lines += ""
$lines += "| Step | Log | Exit |"
$lines += "|---|---|---|"
foreach ($k in $Results.Keys) {
    $r = $Results[$k]
    $lines += "| $k | ``$($r.Log)`` | $($r.ExitCode) |"
}
$lines += ""
$lines += "## Where to look"
$lines += ""
$lines += "- ``03-bench-hash.log`` - line ``[hash bench]``: debug-fmt vs structural ms/frame, isolated."
$lines += "- ``06-before-idle.log`` / ``09-after-idle.log`` - ``t=5s cpu=``, ``t=15s cpu=``, idle-10s delta."
$lines += "- ``06-before-frame.log`` / ``09-after-frame.log`` - ``[frame]`` phase lines. Compare the"
$lines += "  ``faces`` phase on WARM frames only (skip the first two: cold imgpre + face load)."
$lines += "- ``01-clippy.log`` / ``02-test-hash.log`` - gates; both must be exit 0."
$lines += ""
$lines += "Baseline (before this patch), from EXPERIMENT.md items 8-9:"
$lines += "warm ``faces`` = 1.2-2.5 ms/frame (only ``hash_display_list`` left in it);"
$lines += "CPU at t=5s = 2078.1 ms."
$lines -join "`r`n" | Set-Content -Path $Summary -Encoding UTF8

Write-Header "Done"
Write-Host "Artefacts: $OutDir"
Get-ChildItem $OutDir | Select-Object Name, Length | Format-Table -AutoSize
Write-Host "Manifest: .tmp\exp-hash\00-summary.md"
