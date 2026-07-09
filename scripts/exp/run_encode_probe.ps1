# BUG-274 encode diagnostics: build the shell and capture one LUMEN_FRAME_LOG=3
# run. Pure measurement — no stash, no BEFORE/AFTER, nothing to compare yet.
#
#   powershell -ExecutionPolicy Bypass -File scripts\exp\run_encode_probe.ps1
#
# Artefacts land in .tmp\exp-encode\. The question this run must answer:
#   is the ~260 ms cold-frame `encode` spread evenly over ~260 passes,
#   or concentrated in the handful of passes that touch a fresh texture?
# The histogram and the top-12 list decide it; the `alloc:` line says how much
# of it is allocation rather than use.

param(
    [string]$Page = "graphic_tests\1000000-final.html",
    # How many repeats. Frame timings are noisy; one run proves nothing (see
    # EXPERIMENT.md "Грабли": t=5s spread was 281 ms across identical binaries).
    [int]$Runs = 3
)

$ErrorActionPreference = "Continue"
$PSNativeCommandUseErrorActionPreference = $false

$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $Root

$OutDir = Join-Path $Root ".tmp\exp-encode"
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Root ".tmp") | Out-Null

$FrameSink = Join-Path $Root ".tmp\idle_stderr.log"

Write-Host "=== clippy lumen-paint ===" -ForegroundColor Cyan
cargo clippy -p lumen-paint --all-targets -- -D warnings *>&1 |
    Tee-Object -FilePath (Join-Path $OutDir "01-clippy.log") | Out-Host
if ($LASTEXITCODE -ne 0) {
    Write-Host "clippy failed - see .tmp\exp-encode\01-clippy.log" -ForegroundColor Red
    exit 1
}

Write-Host "=== build shell ===" -ForegroundColor Cyan
cargo build --profile dev-release -p lumen-shell *>&1 |
    Tee-Object -FilePath (Join-Path $OutDir "02-build.log") | Out-Host
if ($LASTEXITCODE -ne 0) {
    Write-Host "build failed - see .tmp\exp-encode\02-build.log" -ForegroundColor Red
    exit 1
}

for ($i = 1; $i -le $Runs; $i++) {
    Write-Host "=== run $i / $Runs (LUMEN_FRAME_LOG=3) ===" -ForegroundColor Cyan
    & powershell -ExecutionPolicy Bypass -File (Join-Path $Root "scripts\exp\measure_idle.ps1") `
        -Page $Page -FrameLog 3 *>&1 |
        Tee-Object -FilePath (Join-Path $OutDir ("{0:d2}-idle.log" -f $i)) | Out-Host

    $frameLog = Join-Path $OutDir ("{0:d2}-frame.log" -f $i)
    if (Test-Path $FrameSink) {
        Copy-Item $FrameSink $frameLog -Force
    } else {
        "no stderr sink at $FrameSink" | Set-Content $frameLog
    }
}

Write-Host ""
Write-Host "Artefacts: $OutDir" -ForegroundColor Green
Get-ChildItem $OutDir | Select-Object Name, Length | Format-Table -AutoSize
