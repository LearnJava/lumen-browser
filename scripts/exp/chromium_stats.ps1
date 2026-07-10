# Process-tree resource monitor for a Chromium browser (p1-exp-wgpu-only tooling).
#
#   powershell -ExecutionPolicy Bypass -File scripts\exp\chromium_stats.ps1 `
#       -RootPid <pid> [-Seconds 15] [-IntervalMs 500]
#
# Counterpart of proc_stats.ps1 for multi-process browsers: each sample sums
# CPU / GPU / working set / private bytes over the WHOLE process tree rooted at
# -RootPid (browser + renderers + GPU process + utilities). Same counters as
# proc_stats.ps1, so the two reports are directly comparable:
#   cpu  — sum of per-process CPU deltas, normalized to all cores,
#   gpu  — sum of "GPU Engine \ Utilization Percentage" over the tree's pids,
#   ws / priv — summed working set / private bytes, MB.
# Prints one line per sample and a steady-state summary (t > 5 s).
param(
    [Parameter(Mandatory = $true)][int]$RootPid,
    [int]$Seconds = 15,
    [int]$IntervalMs = 500
)

$ErrorActionPreference = "Continue"
$cores = [Environment]::ProcessorCount

function Get-Tree([int]$root) {
    # Children discovered via Win32_Process ParentProcessId, transitively.
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
    $byParent = @{}
    foreach ($p in $all) {
        if (-not $byParent.ContainsKey([int]$p.ParentProcessId)) {
            $byParent[[int]$p.ParentProcessId] = New-Object System.Collections.Generic.List[int]
        }
        $byParent[[int]$p.ParentProcessId].Add([int]$p.ProcessId)
    }
    $tree = New-Object System.Collections.Generic.List[int]
    $queue = New-Object System.Collections.Generic.Queue[int]
    $queue.Enqueue($root)
    while ($queue.Count -gt 0) {
        $cur = $queue.Dequeue()
        $tree.Add($cur)
        if ($byParent.ContainsKey($cur)) {
            foreach ($c in $byParent[$cur]) { $queue.Enqueue($c) }
        }
    }
    return $tree
}

$samples = New-Object System.Collections.Generic.List[object]
$prevCpu = @{}   # pid -> TotalProcessorTime ms
$prevT = 0.0
$sw = [Diagnostics.Stopwatch]::StartNew()

while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
    Start-Sleep -Milliseconds $IntervalMs
    $t = $sw.Elapsed.TotalSeconds
    $pids = Get-Tree $RootPid
    if ($pids.Count -eq 0) { break }

    $cpuDeltaMs = 0.0
    $ws = 0L; $priv = 0L; $nproc = 0
    $curCpu = @{}
    foreach ($procId in $pids) {
        try { $proc = Get-Process -Id $procId -ErrorAction Stop } catch { continue }
        $nproc++
        $ms = $proc.TotalProcessorTime.TotalMilliseconds
        $curCpu[$procId] = $ms
        if ($prevCpu.ContainsKey($procId)) { $cpuDeltaMs += $ms - $prevCpu[$procId] }
        $ws += $proc.WorkingSet64
        $priv += $proc.PrivateMemorySize64
    }
    if ($nproc -eq 0) { break }
    $prevCpuKnown = $prevCpu.Count -gt 0
    $prevCpu = $curCpu

    $cpuPct = 0.0
    if ($prevCpuKnown -and $t -gt $prevT) {
        $cpuPct = $cpuDeltaMs / (($t - $prevT) * 1000.0 * $cores) * 100.0
    }
    $prevT = $t

    $gpu = 0.0
    try {
        $c = Get-Counter "\GPU Engine(*)\Utilization Percentage" -ErrorAction Stop
        foreach ($s in $c.CounterSamples) {
            foreach ($procId in $pids) {
                if ($s.InstanceName -like "pid_${procId}_*") { $gpu += $s.CookedValue; break }
            }
        }
    } catch {}

    $row = [pscustomobject]@{
        t = [math]::Round($t, 1); cpu = [math]::Round($cpuPct, 1)
        gpu = [math]::Round($gpu, 1); ws = [math]::Round($ws / 1MB)
        priv = [math]::Round($priv / 1MB); n = $nproc
    }
    $samples.Add($row)
    Write-Host ("t={0,5:f1}s cpu={1,5:f1}% gpu={2,5:f1}% ws={3,5}MB priv={4,5}MB procs={5}" -f `
        $row.t, $row.cpu, $row.gpu, $row.ws, $row.priv, $row.n)
}

$steady = @($samples | Where-Object { $_.t -gt 5.0 })
if ($steady.Count -gt 0) {
    $cpuAvg = ($steady | Measure-Object -Property cpu -Average).Average
    $gpuAvg = ($steady | Measure-Object -Property gpu -Average).Average
    $gpuMax = ($steady | Measure-Object -Property gpu -Maximum).Maximum
    $privMax = ($steady | Measure-Object -Property priv -Maximum).Maximum
    Write-Host ("[chromium_stats] steady (t>5s, n={0}): cpu avg {1:f1}% | gpu avg {2:f1}% max {3:f1}% | priv max {4} MB" -f `
        $steady.Count, $cpuAvg, $gpuAvg, $gpuMax, $privMax)
} else {
    Write-Host "[chromium_stats] no steady-state samples"
}
