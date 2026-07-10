# Captures the main window of an EXISTING process via PrintWindow
# (PW_RENDERFULLCONTENT|PW_CLIENTONLY — same capture as printwindow.ps1,
# which launches its own process and cannot shoot a window that an MCP
# harness is already driving).
#
#   powershell -ExecutionPolicy Bypass -File scripts\exp\capture_pid.ps1 `
#       -TargetPid <pid> -Out .tmp\shot.png
param(
    [Parameter(Mandatory = $true)][int]$TargetPid,
    [string]$Out = ".tmp\pw_capture.png"
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class PW2 {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@ -ReferencedAssemblies System.Drawing
Add-Type -AssemblyName System.Drawing
$h = (Get-Process -Id $TargetPid).MainWindowHandle
if ($h -eq [IntPtr]::Zero) { Write-Error "process $TargetPid has no main window"; exit 1 }
$r = New-Object PW2+RECT
[PW2]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
[PW2]::PrintWindow($h, $hdc, 3) | Out-Null   # 3 = PW_RENDERFULLCONTENT | PW_CLIENTONLY
$g.ReleaseHdc($hdc); $g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
Write-Output "saved $Out ($w x $ht)"
