# Relative mouse input for the TomBox host window (the title reads raw mouse deltas).
# usage: mouse.ps1 <dx> <dy> [click]     moves in steps of <=10 counts, then optionally left-clicks
param([int]$Dx = 0, [int]$Dy = 0, [string]$Click = "")
Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class TbMouse {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
    public static IntPtr Find() {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => { var sb = new StringBuilder(256); GetClassName(h, sb, 256);
            var tt = new StringBuilder(256); GetWindowText(h, tt, 256);
            if (sb.ToString() == "TomBoxHostWindow" || (tt.ToString().Trim() == "Fortnite" && IsWindowVisible(h))) { found = h; return false; } return true; }, IntPtr.Zero);
        return found;
    }
}
'@
$h = [TbMouse]::Find()
if ($h -eq [IntPtr]::Zero) { Write-Output "no TomBoxHostWindow"; exit 1 }
[void][TbMouse]::SetForegroundWindow($h); Start-Sleep -Milliseconds 300
if ([TbMouse]::GetForegroundWindow() -ne $h) {
    # Windows refuses to hand over the foreground to a background process unless it has
    # just received input: an injected Alt press/release counts.
    [TbMouse]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [TbMouse]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    [void][TbMouse]::SetForegroundWindow($h); Start-Sleep -Milliseconds 300
    if ([TbMouse]::GetForegroundWindow() -ne $h) { Write-Output "WARNING: title window is not in the foreground" }
}
$rx = $Dx; $ry = $Dy
while ($rx -ne 0 -or $ry -ne 0) {
    $sx = [Math]::Max(-10, [Math]::Min(10, $rx)); $sy = [Math]::Max(-10, [Math]::Min(10, $ry))
    # Injected relative moves only produce input while the OS cursor can move: keep it mid-screen.
    [void][TbMouse]::SetCursorPos(900, 600)
    [TbMouse]::mouse_event(0x0001, $sx, $sy, 0, [UIntPtr]::Zero)
    $rx -= $sx; $ry -= $sy; Start-Sleep -Milliseconds 12
}
if ($Click -eq "click") {
    Start-Sleep -Milliseconds 250
    [TbMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 90
    [TbMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
}
Write-Output "moved $Dx,$Dy $Click"
