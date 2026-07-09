# Phase breakdown of a scrolling frame (p1-exp-wgpu-only).
#
#   powershell -ExecutionPolicy Bypass -File scripts\exp\run_scroll_phases.ps1
#
# The warm-frame bench says a scrolling frame costs ~46.6 ms median, while the
# same display list in the idle run costs 8.0 ms (collect 3.1 + encode 3.9).
# Six times more, same page, same window. This run finds out where it goes.
#
# Hypothesis under test: `hash_display_list` folds scroll_x/scroll_y, and the
# same hash keys the backdrop-filter result cache. Scrolling changes the hash
# every frame, so the cache misses every frame and the page's 10 `bdrop` items
# re-run their two-pass Gaussian blur. If true, `plan: ... bdrop 10xNN.Nms`
# will dominate, and the cache key — not the pass count — is the bug.
#
# Few frames: LUMEN_FRAME_LOG=2 prints ~5 lines per frame, and the eprintln
# itself lands in the sample. These timings are for *proportions*, not for
# absolute per-frame cost — take that from run_warm_frame_bench.ps1.

param(
    [string]$Page = "graphic_tests\1000000-final.html",
    [int]$Frames = 60,
    [int]$Warmup = 30
)

$ErrorActionPreference = "Continue"
$PSNativeCommandUseErrorActionPreference = $false

$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $Root

$OutDir = Join-Path $Root ".tmp\exp-scrollphase"
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$Exe = Join-Path $Root "target\dev-release\lumen.exe"

Write-Host "=== build shell ===" -ForegroundColor Cyan
cargo build --profile dev-release -p lumen-shell *>&1 |
    Tee-Object -FilePath (Join-Path $OutDir "01-build.log") | Out-Host
if ($LASTEXITCODE -ne 0) {
    Write-Host "build failed" -ForegroundColor Red
    exit 1
}

$env:LUMEN_PRESENT = "immediate"
$env:LUMEN_WINDOW = "1024x720"
$env:LUMEN_FRAME_LOG = "2"

foreach ($mode in @("scroll", "hover")) {
    $env:LUMEN_BENCH = "${mode}:${Frames}:${Warmup}"
    Write-Host "=== $mode with phase log ===" -ForegroundColor Cyan
    & $Exe $Page *>&1 | Tee-Object -FilePath (Join-Path $OutDir "$mode-frames.log") | Out-Null
    Write-Host "  -> $OutDir\$mode-frames.log"
}

Remove-Item Env:\LUMEN_BENCH -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "=== last 3 scrolling frames ===" -ForegroundColor Green
Get-Content (Join-Path $OutDir "scroll-frames.log") |
    Select-String -Pattern "^\[frame:wgpu\] total|^\[frame:wgpu\]   plan:|^\[bench\]" |
    Select-Object -Last 9 |
    ForEach-Object { $_.Line }

Write-Host ""
Write-Host "=== skip counters ===" -ForegroundColor Green
Select-String -Path (Join-Path $OutDir "*.log") -Pattern "\[bench\] (hover|scroll)" |
    ForEach-Object { "{0}: {1}" -f $_.Filename, $_.Line.Trim() }
