# Is the GPU-bound scroll frame a real problem, or an artefact of the test page?
#
#   powershell -ExecutionPolicy Bypass -File scripts\exp\run_gpu_bound_probe.ps1
#
# Established by run_scroll_phases.ps1: a scrolling frame on
# graphic_tests/1000000-final.html costs 46.6 ms, of which 36 ms is `acquire`
# — the CPU blocking on `get_current_texture()` while the GPU finishes. Present
# mode is Immediate, so this is not a vsync wait. The frame is GPU-bound.
#
# Two questions, one run each, before any optimization is proposed:
#
#   A) Does frame time scale with viewport AREA?
#      1024x720 -> 512x360 is a 4x area reduction.
#        ~4x faster  => fillrate-bound: the fix is to draw less area
#                       (bbox-sized offscreen layers, damage rects, tile cache).
#        ~unchanged  => fixed per-pass GPU cost: the fix is fewer passes
#                       (RenderTaskGraph, opacity collapse, SDF shadows).
#
#   B) Is the stress page representative?
#      1000000-final.html deliberately stacks every effect the engine has:
#      30 filter passes, 5 backdrop-filters, 869 draw ops. A normal page has
#      none of that. If samples/page.html scrolls in single-digit ms, then the
#      "scroll is slow" finding applies only to a page nobody will ever open,
#      and the 100-1000x goal needs an honest workload before it needs code.
#
# Run C is the same normal page at the small viewport, to separate "the page is
# simpler" from "the window is smaller".

param(
    [int]$Frames = 300,
    [int]$Warmup = 30
)

$ErrorActionPreference = "Continue"
$PSNativeCommandUseErrorActionPreference = $false

$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $Root

$OutDir = Join-Path $Root ".tmp\exp-gpubound"
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$Exe = Join-Path $Root "target\dev-release\lumen.exe"

Write-Host "=== build shell ===" -ForegroundColor Cyan
cargo build --profile dev-release -p lumen-shell *>&1 |
    Tee-Object -FilePath (Join-Path $OutDir "00-build.log") | Out-Host
if ($LASTEXITCODE -ne 0) { Write-Host "build failed" -ForegroundColor Red; exit 1 }

$env:LUMEN_PRESENT = "immediate"
$env:LUMEN_FRAME_LOG = "0"
$env:LUMEN_BENCH = "scroll:${Frames}:${Warmup}"

# name, page, window
$cases = @(
    @("A1-stress-1024x720", "graphic_tests\1000000-final.html", "1024x720"),
    @("A2-stress-512x360",  "graphic_tests\1000000-final.html", "512x360"),
    @("B-normal-1024x720",  "samples\page.html",                "1024x720"),
    @("C-normal-512x360",   "samples\page.html",                "512x360")
)

foreach ($c in $cases) {
    $name, $page, $win = $c
    Write-Host "=== $name ($page @ $win) ===" -ForegroundColor Cyan
    $env:LUMEN_WINDOW = $win
    & $Exe $page *>&1 | Tee-Object -FilePath (Join-Path $OutDir "$name.log") | Out-Null
    $line = Select-String -Path (Join-Path $OutDir "$name.log") -Pattern "\[bench\] scroll"
    if ($line) { Write-Host "  $($line.Line.Trim())" } else { Write-Host "  no [bench] line - check the log" -ForegroundColor Yellow }
    $geo = Select-String -Path (Join-Path $OutDir "$name.log") -Pattern "\[bench\] geometry"
    if ($geo) { Write-Host "  $($geo[0].Line.Trim())" -ForegroundColor DarkGray }
}

Remove-Item Env:\LUMEN_BENCH -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "=== summary ===" -ForegroundColor Green
Select-String -Path (Join-Path $OutDir "*.log") -Pattern "\[bench\] scroll" |
    ForEach-Object { "{0,-24} {1}" -f $_.Filename, $_.Line.Trim() }
Write-Host ""
Write-Host "A1/A2 ratio ~4x  => fillrate-bound (draw less area)"
Write-Host "A1/A2 ratio ~1x  => per-pass bound (draw fewer passes)"
Write-Host "B fast           => the stress page is not representative"
