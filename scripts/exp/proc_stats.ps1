# Per-process resource monitor for lumen.exe (p1-exp-wgpu-only tooling).
#
#   powershell -ExecutionPolicy Bypass -File scripts\exp\proc_stats.ps1 `
#       [-Page <html>] [-Seconds 15] [-IntervalMs 500]
#
# Launches lumen with the given page and samples, until the process exits or
# -Seconds elapses:
#   cpu  — process CPU % normalized to all cores (Task-Manager style),
#   gpu  — sum of the process's "GPU Engine \ Utilization Percentage"
#          counters (same source Task Manager's GPU column reads),
#   ws / priv — working set / private bytes, MB.
# Prints one line per sample and a summary over the steady-state window
# (t > 5 s, startup excluded). Combine with LUMEN_BENCH / LUMEN_PRESENT env
# vars to profile a scroll/hover workload instead of idle.
param(
    [string]$Page = "graphic_tests\1000000-final.html",
    [int]$Seconds = 15,
    [int]$IntervalMs = 500,
    [string]$Exe = "target\dev-release\lumen.exe"
)

$ErrorActionPreference = "Continue"

$p = Start-Process -FilePath $Exe -ArgumentList $Page -PassThru
$cores = [Environment]::ProcessorCount
$samples = New-Object System.Collections.Generic.List[object]
$prevCpuMs = $null
$prevT = 0.0
$sw = [Diagnostics.Stopwatch]::StartNew()

while (-not $p.HasExited -and $sw.Elapsed.TotalSeconds -lt $Seconds) {
    Start-Sleep -Milliseconds $IntervalMs
    try { $proc = Get-Process -Id $p.Id -ErrorAction Stop } catch { break }
    $t = $sw.Elapsed.TotalSeconds
    $cpuMs = $proc.TotalProcessorTime.TotalMilliseconds
    $cpuPct = 0.0
    if ($null -ne $prevCpuMs -and $t -gt $prevT) {
        $cpuPct = ($cpuMs - $prevCpuMs) / (($t - $prevT) * 1000.0 * $cores) * 100.0
    }
    $prevCpuMs = $cpuMs
    $prevT = $t
    $gpu = 0.0
    try {
        $c = Get-Counter "\GPU Engine(pid_$($p.Id)_*)\Utilization Percentage" -ErrorAction Stop
        $gpu = ($c.CounterSamples | Measure-Object -Property CookedValue -Sum).Sum
    } catch {}
    $row = [pscustomobject]@{
        t    = [math]::Round($t, 1)
        cpu  = [math]::Round($cpuPct, 1)
        gpu  = [math]::Round($gpu, 1)
        ws   = [math]::Round($proc.WorkingSet64 / 1MB)
        priv = [math]::Round($proc.PrivateMemorySize64 / 1MB)
    }
    $samples.Add($row)
    Write-Host ("t={0,5:f1}s cpu={1,5:f1}% gpu={2,5:f1}% ws={3,5}MB priv={4,5}MB" -f `
        $row.t, $row.cpu, $row.gpu, $row.ws, $row.priv)
}

$exited = $p.HasExited
if (-not $exited) { Stop-Process -Id $p.Id -Force }

$steady = @($samples | Where-Object { $_.t -gt 5.0 })
if ($steady.Count -gt 0) {
    $cpuAvg = ($steady | Measure-Object -Property cpu -Average).Average
    $gpuAvg = ($steady | Measure-Object -Property gpu -Average).Average
    $gpuMax = ($steady | Measure-Object -Property gpu -Maximum).Maximum
    $privMax = ($steady | Measure-Object -Property priv -Maximum).Maximum
    Write-Host ("[proc_stats] steady (t>5s, n={0}): cpu avg {1:f1}% | gpu avg {2:f1}% max {3:f1}% | priv max {4} MB | exited={5}" -f `
        $steady.Count, $cpuAvg, $gpuAvg, $gpuMax, $privMax, $exited)
} else {
    Write-Host "[proc_stats] no steady-state samples (process exited early? t<5s)"
}
