# Capture the TomBox host window's client area to a PNG.
# usage: powershell -File capture.ps1 <out.png>
param([string]$Out)
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class TbCap {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    public static IntPtr Find() {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            var sb = new StringBuilder(256); GetClassName(h, sb, 256);
            var tt = new StringBuilder(256); GetWindowText(h, tt, 256);
            if (sb.ToString() == "TomBoxHostWindow" || (tt.ToString().Trim() == "Fortnite" && IsWindowVisible(h))) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
Add-Type -AssemblyName System.Drawing
$h = [TbCap]::Find()
if ($h -eq [IntPtr]::Zero) { Write-Output "no TomBoxHostWindow"; exit 1 }
[void][TbCap]::ShowWindow($h, 9)
[void][TbCap]::SetWindowPos($h, [IntPtr](-1), 40, 40, 0, 0, 0x0001 -bor 0x0040)
Start-Sleep -Milliseconds 400
$r = New-Object TbCap+RECT; [void][TbCap]::GetClientRect($h, [ref]$r)
$p = New-Object TbCap+POINT; [void][TbCap]::ClientToScreen($h, [ref]$p)
$w = $r.R - $r.L; $hh = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap $w, $hh
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($p.X, $p.Y, 0, 0, (New-Object System.Drawing.Size $w, $hh))
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "captured ${w}x${hh} -> $Out"
