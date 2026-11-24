# Resilient launcher: clears stale headless instances, then starts Agent Browser.
Add-Type -AssemblyName System.Windows.Forms
$installed = Join-Path $env:LOCALAPPDATA "Programs\Agent Browser\target\release\agent-browser.exe"
$dev = Join-Path $PSScriptRoot "target\release\agent-browser.exe"
$exe = if (Test-Path $installed) { $installed } else { $dev }

if (-not (Test-Path $exe)) {
  [System.Windows.Forms.MessageBox]::Show("Agent Browser executable not found: $exe", "Agent Browser")
  exit 1
}

Get-Process agent-browser -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400
Start-Process -FilePath $exe
