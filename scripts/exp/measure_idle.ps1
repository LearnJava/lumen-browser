# BUG-274 idle-CPU measurement: launch lumen.exe on the demo page,
# sample TotalProcessorTime/WorkingSet at t=5s and t=15s (10s idle window), kill.
param(
    [string]$Exe = "target\dev-release\lumen.exe",
    [string]$Page = "graphic_tests\1000000-final.html",
    [string]$FrameLog = "0"
)
$env:LUMEN_FRAME_LOG = $FrameLog
$p = Start-Process -FilePath $Exe -ArgumentList $Page -PassThru -RedirectStandardError ".tmp\idle_stderr.log"
Start-Sleep -Seconds 5
$p.Refresh()
$cpu5 = $p.TotalProcessorTime.TotalMilliseconds
$ws5 = [math]::Round($p.WorkingSet64 / 1MB, 1)
$pb5 = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
Start-Sleep -Seconds 10
$p.Refresh()
$cpu15 = $p.TotalProcessorTime.TotalMilliseconds
$ws15 = [math]::Round($p.WorkingSet64 / 1MB, 1)
$pb15 = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
Stop-Process -Id $p.Id -Force
Write-Output ("t=5s   cpu={0}ms ws={1}MB priv={2}MB" -f [math]::Round($cpu5,1), $ws5, $pb5)
Write-Output ("t=15s  cpu={0}ms ws={1}MB priv={2}MB" -f [math]::Round($cpu15,1), $ws15, $pb15)
Write-Output ("idle-10s CPU delta = {0}ms  ws delta = {1}MB  priv delta = {2}MB" -f [math]::Round($cpu15-$cpu5,1), [math]::Round($ws15-$ws5,1), [math]::Round($pb15-$pb5,1))
