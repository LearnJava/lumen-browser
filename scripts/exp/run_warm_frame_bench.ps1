# Warm-frame benchmark (p1-exp-wgpu-only).
#
#   powershell -ExecutionPolicy Bypass -File scripts\exp\run_warm_frame_bench.ps1
#
# Measures what `measure_idle.ps1` cannot: the cost of a repaint under input.
# EXPERIMENT.md §12 established that the idle window contains four frames and no
# skips, so `t=5s cpu` is startup, not steady state.
#
# Two modes, three repeats each:
#   hover  — redraw with no state change; every frame takes the
#            skip-identical-frame path, so the sample IS the frame hash.
#   scroll — one CSS px of page scroll per frame; a real repaint
#            (hash -> collect -> encode -> submit), display list untouched.
#
# LUMEN_PRESENT=immediate is mandatory: under the default Fifo present mode the
# swapchain acquire blocks on vsync and every sample flatlines at ~16.7 ms.

param(
    [string]$Page = "graphic_tests\1000000-final.html",
    [int]$Frames = 600,
    [int]$Warmup = 30,
    [int]$Runs = 3
)

$ErrorActionPreference = "Continue"
$PSNativeCommandUseErrorActionPreference = $false

$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $Root

$OutDir = Join-Path $Root ".tmp\exp-warmframe"
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$Exe = Join-Path $Root "target\dev-release\lumen.exe"

Write-Host "=== clippy lumen-shell ===" -ForegroundColor Cyan
cargo clippy -p lumen-shell --all-targets -- -D warnings *>&1 |
    Tee-Object -FilePath (Join-Path $OutDir "01-clippy.log") | Out-Host
if ($LASTEXITCODE -ne 0) {
    Write-Host "clippy failed - see .tmp\exp-warmframe\01-clippy.log" -ForegroundColor Red
    exit 1
}

Write-Host "=== build shell ===" -ForegroundColor Cyan
cargo build --profile dev-release -p lumen-shell *>&1 |
    Tee-Object -FilePath (Join-Path $OutDir "02-build.log") | Out-Host
if ($LASTEXITCODE -ne 0) {
    Write-Host "build failed - see .tmp\exp-warmframe\02-build.log" -ForegroundColor Red
    exit 1
}

$env:LUMEN_PRESENT = "immediate"
$env:LUMEN_FRAME_LOG = "0"   # per-frame eprintln would dominate the sample
# Fixed viewport: the window must be smaller than the page, otherwise
# max_scroll == 0 and scroll mode degenerates into hover mode (the harness
# now says so out loud, but a deterministic size makes runs comparable too).
$env:LUMEN_WINDOW = "1024x720"

foreach ($mode in @("hover", "scroll")) {
    for ($i = 1; $i -le $Runs; $i++) {
        Write-Host "=== $mode run $i / $Runs ===" -ForegroundColor Cyan
        $env:LUMEN_BENCH = "${mode}:${Frames}:${Warmup}"
        $log = Join-Path $OutDir ("{0}-{1:d2}.log" -f $mode, $i)
        & $Exe $Page *>&1 | Tee-Object -FilePath $log | Out-Host
    }
}

Remove-Item Env:\LUMEN_BENCH -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "=== [bench] lines ===" -ForegroundColor Green
Select-String -Path (Join-Path $OutDir "*.log") -Pattern "\[bench\]" |
    ForEach-Object { "{0}: {1}" -f $_.Filename, $_.Line.Trim() }

Write-Host ""
Write-Host "Artefacts: $OutDir" -ForegroundColor Green
