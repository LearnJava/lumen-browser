# Неинвазивный наблюдатель за живым lumen.exe: метрики процесса, отзывчивость окна,
# потоки, GUI-ресурсы, системная память. Ничего не шлёт в MCP и не трогает чужой прогон.
#
# Перенесён из .tmp/observe/ (THREAD-0, 2026-09-04) в трекаемое место (THREAD-1 срез 2) —
# рабочая копия в .tmp/ не версионируется и терялась между сессиями. Пути больше не
# зашиты на корневой чекаут: и OutDir, и каталог live.stderr-логов perf_audit.py
# выводятся из расположения самого скрипта, поэтому один и тот же файл работает
# что из корня, что из любого worktree в .claude/worktrees/.
param(
  [int]$IntervalMs = 1000,
  [string]$OutDir  = "",
  [int]$DurationMin = 0
)

$ErrorActionPreference = 'Continue'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot ".tmp\observe" }
$PerfAuditDir = Join-Path $RepoRoot ".tmp\perf-audit"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$samplesPath = Join-Path $OutDir 'samples.jsonl'
$eventsPath  = Join-Path $OutDir 'events.jsonl'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class NativeWin {
  [DllImport("user32.dll")] public static extern bool IsHungAppWindow(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);
  [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr hProcess, uint flags);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
}
'@ -ErrorAction SilentlyContinue

function Write-Line($path, $obj) {
  ($obj | ConvertTo-Json -Compress -Depth 6) | Out-File -FilePath $path -Append -Encoding utf8
}

function Now() { (Get-Date).ToString('o') }

function Get-LumenProc() {
  Get-Process -Name lumen -ErrorAction SilentlyContinue | Sort-Object StartTime | Select-Object -Last 1
}

$cores = [Environment]::ProcessorCount
$prev  = @{}          # pid -> @{ cpu; ts; threads = @{ tid -> cpuMs } }
$curPid = 0
$hangStart = $null
$hangPeak  = 0.0
$sampleNo  = 0
$deadline  = if ($DurationMin -gt 0) { (Get-Date).AddMinutes($DurationMin) } else { $null }

Write-Line $eventsPath @{ ts = (Now); kind = 'observer_start'; interval_ms = $IntervalMs; cores = $cores }

while ($true) {
  if ($deadline -and (Get-Date) -gt $deadline) { break }
  $loopStart = Get-Date
  $p = Get-LumenProc

  if (-not $p) {
    if ($curPid -ne 0) {
      $ev = [ordered]@{ ts = (Now); kind = 'process_gone'; pid = $curPid }
      try { if ($lastProc) { $ev.exit_code = $lastProc.ExitCode; $ev.exit_time = $lastProc.ExitTime.ToString('o') } } catch { }
      $ev.last_title = $lastTitle
      $ev.last_ws_mb = $lastWs
      # хвост stderr самого свежего лога прогона + запись из журнала событий, если это падение
      try {
        $log = Get-ChildItem -Path $PerfAuditDir -Recurse -Filter 'live.stderr.*.log' -ErrorAction Stop |
               Sort-Object LastWriteTime | Select-Object -Last 1
        if ($log) {
          $ev.log = $log.FullName
          $ev.log_tail = (Get-Content $log.FullName -Tail 25 -ErrorAction SilentlyContinue) -join "`n"
        }
      } catch { }
      try {
        $since = (Get-Date).AddMinutes(-3)
        $we = Get-WinEvent -FilterHashtable @{ LogName='Application'; StartTime=$since } -MaxEvents 40 -ErrorAction Stop |
              Where-Object { $_.Message -match 'lumen' } | Select-Object -First 3
        if ($we) { $ev.winevents = @($we | ForEach-Object { @{ id = $_.Id; provider = $_.ProviderName; msg = ($_.Message -replace "`r`n", ' | ') } }) }
      } catch { }
      Write-Line $eventsPath $ev
      $curPid = 0; $hangStart = $null; $lastProc = $null
    }
    Start-Sleep -Milliseconds $IntervalMs
    continue
  }

  if ($p.Id -ne $curPid) {
    Write-Line $eventsPath @{ ts = (Now); kind = 'process_new'; pid = $p.Id; prev_pid = $curPid; start = $p.StartTime.ToString('o') }
    $curPid = $p.Id; $prev = @{}; $hangStart = $null
  }
  $lastProc = $p

  $sampleNo++
  $rec = [ordered]@{ ts = (Now); n = $sampleNo; pid = $p.Id }

  try {
    $p.Refresh()
    $nowTs   = Get-Date
    $cpuTot  = $p.TotalProcessorTime.TotalSeconds
    $rec.uptime_s = [math]::Round(($nowTs - $p.StartTime).TotalSeconds, 1)
    $rec.cpu_total_s  = [math]::Round($cpuTot, 3)
    $rec.cpu_user_s   = [math]::Round($p.UserProcessorTime.TotalSeconds, 3)
    $rec.cpu_kernel_s = [math]::Round($p.PrivilegedProcessorTime.TotalSeconds, 3)

    if ($prev.ContainsKey($p.Id)) {
      $dt = ($nowTs - $prev[$p.Id].ts).TotalSeconds
      if ($dt -gt 0) {
        $rec.cpu_pct = [math]::Round((($cpuTot - $prev[$p.Id].cpu) / $dt) * 100.0, 1)
        $rec.cpu_pct_of_all = [math]::Round($rec.cpu_pct / $cores, 1)
      }
    }

    $rec.ws_mb       = [math]::Round($p.WorkingSet64 / 1MB, 1)
    $rec.priv_mb     = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
    $rec.virt_mb     = [math]::Round($p.VirtualMemorySize64 / 1MB, 1)
    $rec.peak_ws_mb  = [math]::Round($p.PeakWorkingSet64 / 1MB, 1)
    $rec.peak_pag_mb = [math]::Round($p.PeakPagedMemorySize64 / 1MB, 1)
    $rec.handles     = $p.HandleCount
    $rec.threads     = $p.Threads.Count

    try {
      $rec.gdi  = [NativeWin]::GetGuiResources($p.Handle, 0)
      $rec.user = [NativeWin]::GetGuiResources($p.Handle, 1)
    } catch { }

    # --- отзывчивость окна ---
    $hwnd = $p.MainWindowHandle
    $rec.hwnd = [int64]$hwnd
    if ($hwnd -ne [IntPtr]::Zero) {
      $rec.hung = [NativeWin]::IsHungAppWindow($hwnd)
      $sb = New-Object System.Text.StringBuilder 512
      [void][NativeWin]::GetWindowTextW($hwnd, $sb, 512)
      $rec.title = $sb.ToString()
      $sw = [System.Diagnostics.Stopwatch]::StartNew()
      $res = [IntPtr]::Zero
      # WM_NULL, SMTO_ABORTIFHUNG|SMTO_NORMAL, 3000 мс
      $r = [NativeWin]::SendMessageTimeout($hwnd, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$res)
      $sw.Stop()
      $rec.pump_ms = [math]::Round($sw.Elapsed.TotalMilliseconds, 1)
      $rec.pump_ok = ($r -ne [IntPtr]::Zero)
    } else {
      $rec.hung = $null; $rec.pump_ok = $null; $rec.title = ''
    }
    $rec.responding = $p.Responding
    $lastTitle = $rec.title
    $lastWs = $rec.ws_mb

    # --- потоки: топ по приросту CPU + распределение состояний ---
    $thrPrev = if ($prev.ContainsKey($p.Id)) { $prev[$p.Id].threads } else { @{} }
    $thrNow  = @{}
    $tops = @()
    $waitHist = @{}
    foreach ($t in $p.Threads) {
      # поток может исчезнуть между перечислением и чтением — любое свойство станет null
      try {
        if ($null -eq $t) { continue }
        $ms = $t.TotalProcessorTime.TotalMilliseconds
        $tid = $t.Id
        $thrNow[$tid] = $ms
        $d = if ($thrPrev.ContainsKey($tid)) { $ms - $thrPrev[$tid] } else { $null }
        $state = "$($t.ThreadState)"
        $wr = ''
        if ($state -eq 'Wait') { $wr = "$($t.WaitReason)" }
        $key = if ($wr) { "$state/$wr" } else { "$state" }
        if ($waitHist.ContainsKey($key)) { $waitHist[$key]++ } else { $waitHist[$key] = 1 }
        $tstart = ''
        try { $tstart = $t.StartTime.ToString('HH:mm:ss') } catch { }
        if ($null -ne $d) {
          $tops += [pscustomobject]@{ tid = $tid; d_ms = [math]::Round($d, 1); st = $key; start = $tstart }
        }
      } catch { continue }
    }
    $rec.thread_states = $waitHist
    $rec.top_threads = @($tops | Sort-Object -Property d_ms -Descending | Select-Object -First 5)

    $prev[$p.Id] = @{ cpu = $cpuTot; ts = $nowTs; threads = $thrNow }

    # --- активность движка: прирост stderr-лога прогона за сэмпл ---
    # молчащий лог при зависании = встал весь процесс, растущий = встал только UI/движок
    if (($sampleNo % 30 -eq 1) -or (-not $logPath)) {
      try {
        $lf = Get-ChildItem -Path $PerfAuditDir -Recurse -Filter 'live.stderr.*.log' -ErrorAction Stop |
              Sort-Object LastWriteTime | Select-Object -Last 1
        if ($lf) { $logPath = $lf.FullName }
      } catch { }
    }
    if ($logPath) {
      try {
        $len = (New-Object System.IO.FileInfo $logPath).Length
        if ($null -ne $lastLogLen) { $rec.log_bytes = $len - $lastLogLen }
        $lastLogLen = $len
        $rec.log_file = Split-Path $logPath -Leaf
      } catch { }
    }

    # --- сеть процесса (раз в 5 сэмплов) ---
    if ($sampleNo % 5 -eq 1) {
      try {
        $tcp = Get-NetTCPConnection -OwningProcess $p.Id -ErrorAction Stop
        $g = @{}
        foreach ($c in $tcp) { $s = "$($c.State)"; if ($g.ContainsKey($s)) { $g[$s]++ } else { $g[$s] = 1 } }
        $rec.tcp = $g
        $rec.tcp_total = @($tcp).Count
      } catch { }
    }

    # --- системный фон (раз в 5 сэмплов) ---
    if ($sampleNo % 5 -eq 1) {
      try {
        $os = Get-CimInstance Win32_OperatingSystem
        $rec.sys_free_mb   = [math]::Round($os.FreePhysicalMemory / 1KB, 0)
        $rec.sys_commit_mb = [math]::Round(($os.TotalVirtualMemorySize - $os.FreeVirtualMemory) / 1KB, 0)
      } catch { }
      try {
        $perf = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($p.Id)" -ErrorAction Stop
        if ($perf) {
          $rec.io_read_mb   = [math]::Round($perf.IOReadBytesPersec / 1MB, 1)
          $rec.io_write_mb  = [math]::Round($perf.IOWriteBytesPersec / 1MB, 1)
          $rec.io_other_ops = $perf.IOOtherOperationsPersec
          $rec.page_faults  = $perf.PageFaultsPersec
          $rec.priv_bytes_mb = [math]::Round($perf.PrivateBytes / 1MB, 1)
          $rec.ws_private_mb = [math]::Round($perf.WorkingSetPrivate / 1MB, 1)
        }
      } catch { }
    }

    # --- события зависания ---
    $isStuck = ($rec.hung -eq $true) -or ($rec.pump_ok -eq $false) -or ($rec.pump_ms -ge 1000)
    if ($isStuck) {
      if (-not $hangStart) {
        $hangStart = $nowTs
        $hangPeak = 0.0
        Write-Line $eventsPath @{ ts = (Now); kind = 'hang_start'; pid = $p.Id; title = $rec.title;
                                  ws_mb = $rec.ws_mb; handles = $rec.handles; threads = $rec.threads;
                                  cpu_pct = $rec.cpu_pct; top_threads = $rec.top_threads;
                                  thread_states = $rec.thread_states; log_bytes = $rec.log_bytes;
                                  tcp = $rec.tcp; log_file = $rec.log_file }
        $hangCpuSum = 0.0; $hangLogSum = 0; $hangSamples = 0
      }
      if ($rec.cpu_pct -and $rec.cpu_pct -gt $hangPeak) { $hangPeak = $rec.cpu_pct }
      if ($rec.cpu_pct) { $hangCpuSum += $rec.cpu_pct }
      if ($rec.log_bytes) { $hangLogSum += $rec.log_bytes }
      $hangSamples++
      $rec.in_hang = $true
    } elseif ($hangStart) {
      $dur = ((Get-Date) - $hangStart).TotalSeconds
      $avgCpu = if ($hangSamples -gt 0) { [math]::Round($hangCpuSum / $hangSamples, 1) } else { $null }
      # класс зависания: busy = крутится CPU, blocked = процесс простаивает
      $cls = if ($hangPeak -ge 50) { 'busy' } elseif ($hangLogSum -gt 0) { 'blocked_engine_alive' } else { 'blocked_silent' }
      Write-Line $eventsPath @{ ts = (Now); kind = 'hang_end'; pid = $p.Id; dur_s = [math]::Round($dur, 1);
                                peak_cpu_pct = $hangPeak; avg_cpu_pct = $avgCpu; class = $cls;
                                log_bytes_total = $hangLogSum; samples = $hangSamples;
                                title = $rec.title; ws_mb = $rec.ws_mb; tcp = $rec.tcp }
      $hangStart = $null
    }
  } catch {
    $rec.error = "$_"
  }

  Write-Line $samplesPath $rec

  $spent = ((Get-Date) - $loopStart).TotalMilliseconds
  $sleep = [int]($IntervalMs - $spent)
  if ($sleep -gt 0) { Start-Sleep -Milliseconds $sleep }
}

Write-Line $eventsPath @{ ts = (Now); kind = 'observer_stop' }
