param(
  [Parameter(Mandatory=$true)][string]$InstallerPath,
  [Parameter(Mandatory=$true)][string]$InstallDir,
  [Parameter(Mandatory=$true)][string]$LauncherPath
)

Start-Sleep -Milliseconds 500

# The installer must overwrite agent-browser.exe, so every instance using it
# has to exit first — including MCP bridges.
$procs = Get-CimInstance Win32_Process -Filter "name='agent-browser.exe'"
foreach ($proc in $procs) {
  Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
}
foreach ($i in 1..30) {
  if (-not (Get-CimInstance Win32_Process -Filter "name='agent-browser.exe'")) { break }
  Start-Sleep -Milliseconds 200
}

if (-not (Test-Path $InstallerPath)) {
  Write-Error "Installer not found: $InstallerPath"
  exit 1
}

$install = Start-Process -FilePath $InstallerPath -ArgumentList @('/S', "/D=$InstallDir") -Wait -PassThru
if ($install.ExitCode -ne 0) {
  Write-Error "Installer exited with $($install.ExitCode)"
  exit $install.ExitCode
}

Start-Sleep -Seconds 2
if (Test-Path $LauncherPath) {
  Start-Process -FilePath 'powershell.exe' -ArgumentList @(
    '-NoProfile', '-WindowStyle', 'Hidden', '-ExecutionPolicy', 'Bypass',
    '-File', $LauncherPath
  )
}
