param(
  [Parameter(Mandatory=$true)][string]$InstallerPath,
  [Parameter(Mandatory=$true)][string]$InstallDir,
  [Parameter(Mandatory=$true)][string]$LauncherPath
)

Start-Sleep -Milliseconds 700

# Close the running GUI but leave `agent-browser.exe mcp` bridges alone.
Get-CimInstance Win32_Process -Filter "name='agent-browser.exe'" |
  Where-Object { $_.CommandLine -notmatch '\bmcp\b' } |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

Start-Sleep -Milliseconds 500
& $InstallerPath /S "/D=$InstallDir"
Start-Sleep -Seconds 2

if (Test-Path $LauncherPath) {
  Start-Process -FilePath 'powershell.exe' -ArgumentList @(
    '-NoProfile', '-WindowStyle', 'Hidden', '-ExecutionPolicy', 'Bypass',
    '-File', $LauncherPath
  )
}
