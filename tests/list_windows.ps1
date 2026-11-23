Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WEL {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
}
"@
$p = Get-Process agent-browser | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
$cb = [WEL+EnumProc] {
  param($h, $l)
  $pid = 0
  [WEL]::GetWindowThreadProcessId($h, [ref]$pid) | Out-Null
  if ($pid -eq $p.Id) {
    $sb = New-Object System.Text.StringBuilder 256
    [WEL]::GetClassName($h, $sb, 256) | Out-Null
    $r = New-Object WEL+RECT
    [WEL]::GetWindowRect($h, [ref]$r) | Out-Null
    $owner = [WEL]::GetWindow($h, 4)
    "HWND=$h CLASS=$($sb.ToString()) OWNER=$owner RECT=$($r.Left),$($r.Top),$($r.Right),$($r.Bottom)"
  }
  return $true
}
[WEL]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
