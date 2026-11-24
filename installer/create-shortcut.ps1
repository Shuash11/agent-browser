$root = Split-Path -Parent $PSScriptRoot
$script = Join-Path $root 'start-agent-browser.ps1'
$exe = Join-Path $root 'target\release\agent-browser.exe'
$link = Join-Path $env:USERPROFILE 'Desktop\Agent Browser.lnk'

$shell = New-Object -ComObject WScript.Shell
$lnk = $shell.CreateShortcut($link)
$lnk.TargetPath = 'powershell.exe'
$lnk.Arguments = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $script + '"'
$lnk.WorkingDirectory = $root
$lnk.IconLocation = "$exe,0"
$lnk.Description = 'Agent Browser — embedded OpenCode'
$lnk.Save()
Write-Output "shortcut created: $link"
