param([string]$Page = "graphic_tests\1000000-final.html", [string]$Backend = "vulkan", [string]$Out = ".tmp\pw_capture.png")
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class PW {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@ -ReferencedAssemblies System.Drawing
Add-Type -AssemblyName System.Drawing
if ($Backend -ne "auto") { $env:WGPU_BACKEND = $Backend } # auto = ярус-0 проба выбирает сама
$p = Start-Process -FilePath "target\dev-release\lumen.exe" -ArgumentList $Page -PassThru
Start-Sleep -Seconds 8
$h = (Get-Process -Id $p.Id).MainWindowHandle
$r = New-Object PW+RECT
[PW]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
[PW]::PrintWindow($h, $hdc, 3) | Out-Null   # 3 = PW_RENDERFULLCONTENT | PW_CLIENTONLY
$g.ReleaseHdc($hdc); $g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
Stop-Process -Id $p.Id -Force
Write-Output "saved $Out ($w x $ht)"
