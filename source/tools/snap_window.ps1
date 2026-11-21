param([string]$Out = "$env:TEMP\tombox\snap.png")
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class W { [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
public struct RECT { public int L, T, R, B; } }
"@
[W]::SetProcessDPIAware() | Out-Null
$p = Get-Process FortniteClient* -EA 0 | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { "no game window"; exit 1 }
$r = New-Object W+RECT
[W]::GetWindowRect($p.MainWindowHandle, [ref]$r) | Out-Null
$w = $r.R - $r.L; $h = $r.B - $r.T
$bmp = New-Object Drawing.Bitmap $w, $h
$g = [Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
$s = New-Object Drawing.Bitmap $bmp, ([int]($w / 2)), ([int]($h / 2))
$s.Save($Out, [Drawing.Imaging.ImageFormat]::Png)
"$Out $w x $h at $($r.L),$($r.T)"
