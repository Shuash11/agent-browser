param([string]$Version = "0.1.2")

$root = Split-Path -Parent $PSScriptRoot
$stage = Join-Path $PSScriptRoot "staging"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

$files = @(
  "target/release/agent-browser.exe",
  "opencode.jsonc",
  ".opencode/opencode.jsonc",
  "AGENTS.md",
  "README.md",
  "DESIGN.md",
  "LICENSE",
  "start-agent-browser.ps1",
  "apply-update.ps1",
  "tests/fixture.html",
  "assets/terminal/index.html",
  "assets/terminal/toolbar.html",
  "assets/terminal/divider.html",
  "assets/terminal/newtab.html",
  "assets/terminal/tabs.html",
  "assets/terminal/theme.css",
  "assets/terminal/xterm.min.js",
  "assets/terminal/xterm.min.css",
  "assets/terminal/addon-fit.min.js",
  "assets/terminal/LICENSE",
  "assets/terminal/LICENSE-xterm",
  "assets/terminal/icons.css",
  "assets/icons/LICENSE",
  "assets/icons/arrow-left.svg",
  "assets/icons/arrow-right.svg",
  "assets/icons/rotate-cw.svg",
  "assets/icons/house.svg",
  "assets/icons/x.svg",
  "assets/icons/plus.svg",
  "assets/icons/globe.svg",
  "assets/icons/circle-play.svg",
  "assets/icons/app-window.svg",
  "assets/icons/monitor.svg",
  "assets/icons/download.svg",
  "assets/icons/external-link.svg",
  "assets/icons/panel-bottom.svg",
  "assets/icons/panel-left.svg",
  "assets/icons/search.svg",
  "assets/app.ico"
)
foreach ($f in $files) {
  $src = Join-Path $root $f
  if (-not (Test-Path $src)) { throw "staging file missing: $f" }
  $dest = Join-Path $stage $f
  $dir = Split-Path -Parent $dest
  if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
  Copy-Item $src $dest -Force
}
"staged $($files.Count) files into $stage"
