# Click inside the TomBox host window. usage: click.ps1 <fx> <fy>   (fractions of the client area, 0..1)
param([double]$Fx = 0.5, [double]$Fy = 0.68)
Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class TbClick {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    public static IntPtr Find() {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => { var sb = new StringBuilder(256); GetClassName(h, sb, 256);
            var tt = new StringBuilder(256); GetWindowText(h, tt, 256);
            if (sb.ToString() == "TomBoxHostWindow" || (tt.ToString().Trim() == "Fortnite" && IsWindowVisible(h))) { found = h; return false; } return true; }, IntPtr.Zero);
        return found;
    }
}
'@
$h = [TbClick]::Find()
if ($h -eq [IntPtr]::Zero) { Write-Output "no TomBoxHostWindow"; exit 1 }
[void][TbClick]::ShowWindow($h, 9); [void][TbClick]::SetForegroundWindow($h)
Start-Sleep -Milliseconds 500
$r = New-Object TbClick+RECT; [void][TbClick]::GetClientRect($h, [ref]$r)
$p = New-Object TbClick+POINT; [void][TbClick]::ClientToScreen($h, [ref]$p)
$x = $p.X + [int](($r.R - $r.L) * $Fx); $y = $p.Y + [int](($r.B - $r.T) * $Fy)
[void][TbClick]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 200
[TbClick]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 80
[TbClick]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
Write-Output "clicked at $x,$y (client $($r.R - $r.L)x$($r.B - $r.T))"
