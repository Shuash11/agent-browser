Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WE {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
}
"@
$p = Get-Process agent-browser | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
$cb = [WE+EnumProc] {
  param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [WE]::GetClassName($h, $sb, 256) | Out-Null
  $r = New-Object WE+RECT
  [WE]::GetWindowRect($h, [ref]$r) | Out-Null
  $name = $sb.ToString()
  "HWND=$h CLASS=$name RECT=$($r.Left),$($r.Top),$($r.Right),$($r.Bottom)"
  return $true
}
[WE]::EnumChildWindows($p.MainWindowHandle, $cb, [IntPtr]::Zero) | Out-Null
