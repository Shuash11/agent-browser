Add-Type @"
using System; using System.Runtime.InteropServices;
public class NV {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint dx,uint dy,uint d,IntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void SwitchToThisWindow(IntPtr h, bool alt);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetActiveWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
}
"@
Add-Type -AssemblyName System.Windows.Forms
$proc = Get-Process agent-browser | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
$h = $proc.MainWindowHandle
[NV]::BringWindowToTop($h) | Out-Null
[NV]::SwitchToThisWindow($h, $true)
Start-Sleep -Milliseconds 300
[NV]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 300
"foreground match: $([NV]::GetForegroundWindow() -eq $h)"
$r = New-Object NV+RECT
[NV]::GetWindowRect($h, [ref]$r) | Out-Null
[NV]::SetCursorPos($r.Left + 900, $r.Top + 55) | Out-Null
[NV]::mouse_event(2,0,0,0,[IntPtr]::Zero)
Start-Sleep -Milliseconds 100
[NV]::mouse_event(4,0,0,0,[IntPtr]::Zero)
Start-Sleep -Milliseconds 300
[System.Windows.Forms.SendKeys]::SendWait("^a")
[System.Windows.Forms.SendKeys]::SendWait("https://example.com{ENTER}")
"done"
