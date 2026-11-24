# Agent Browser

A local Windows desktop app with an **embedded OpenCode terminal** (xterm.js + ConPTY) and a page WebView. OpenCode controls the page through MCP over stdio. You sign in yourself; the agent can only act on the visible page area, never the terminal panel.

## Layout

- Terminal panel docks **left** (default, 540 px) or **bottom** (half height).
- Drag the 8 px divider between panels to resize; the page viewport and agent coordinates update live.
- Click **Dock: Left / Bottom** in the terminal toolbar, or call `browser_dock {mode:"left"|"bottom"|"toggle"}`.
- The terminal runs OpenCode with cwd = repository root, so it loads the same `opencode.jsonc` and MCP servers.
- The page panel has a **user-only toolbar**: address bar, Back, Forward, Reload, Home. Type a URL and press Enter, or click a button. Navigation is deliberately not exposed to the agent; the MCP viewport starts below the toolbar.
- Four real tabs are available. The **right-edge tab strip** shows every open tab; click a tab (user) or call `browser_focus_tab` (agent) to switch. Each row has a × to close it; closed rows show "reopen". The agent can also call `browser_close_tab {index}`. The last open tab cannot be closed.
- The first tab opens an app-styled **New Tab** page with no search box: seven shortcuts — TikTok, YouTube, Facebook, Instagram, GitHub, **Edge (MSN.com)**, and **Chrome (Google.com)** — all open inside the harness page panel. A **custom source** field stores your own authorized download URL in this browser profile; no third-party downloader is bundled. URL entry stays in the toolbar address bar.
- **Manual updates:** click "Check for updates" in the terminal header. The app queries the GitHub `v*` release, shows a dialog with **Update now / Remind me later / Skip this version**, verifies the installer's GitHub SHA-256 digest, then silently reinstalls into the current folder and relaunches. Skipped tags are remembered in `data/update-state.json`. Updates are user-only — no agent tool can check or install them.
- The agent uses an **in-app virtual cursor**: a cyan ring drawn on the page at the agent's logical position. The page's native pointer is hidden briefly during agent actions.
- **Design:** macOS dark-mode-inspired graphite surfaces, system-blue accents, rounded controls, and functional traffic-light window buttons (close / minimize / maximize) in the terminal header.
- **Harness-side layout cache:** `browser_layout` returns every visible interactive element with a stable ID and screen coordinates. The agent then uses `browser_click_id {id}` / `browser_type_id {id,text}` — one or two calls instead of repeated pixel analysis. The cache invalidates on navigation, scroll, resize, tab switch, and DOM mutation; `browser_layout {refresh:true}` forces a rescan.
- **Text-first perception:** `browser_page_text` and `browser_page_elements` return page content and visible controls with screen coordinates; `browser_scroll_into_view {text}` jumps to named elements. Use `browser_screenshot` only for visual verification (canvas, images, layout bugs), not for locating buttons or reading text — a 1080×1920 screenshot costs roughly 1–3k image tokens each time.
- The toolbar address bar stays in sync with page loads.
- Test hook: `AGENT_BROWSER_TEST_NAV=https://example.com agent-browser.exe` navigates on startup (used to verify the IPC path).

## Build

```powershell
cd tools\agent-browser
cargo build --release
```

Verified on Rust 1.99.1 / Windows 11 / WebView2 157.

## Run

1. Start the GUI (leave it running):
   ```powershell
   .\target\release\agent-browser.exe
   ```
2. Restart OpenCode once so it loads the `agent-browser` MCP server from `opencode.jsonc`.
3. In OpenCode, call the tools:
   - `browser_get_viewport` — screen rectangle of the page panel
   - `browser_screenshot {path}` — capture page viewport only
   - `browser_move {x,y}`, `browser_click {x,y,button}`, `browser_scroll {x,y,dx,dy}`
   - `browser_type {text}`, `browser_key {key}`, `browser_list_tabs`, `browser_focus_tab {index}`, `browser_dock {mode}`

## Behavior and limits

- The agent has **no navigate tool** and cannot create tabs or windows. It only clicks/types inside the visible page area.
- All coordinate actions are checked against the content viewport; out-of-viewport points return an error.
- Password inputs are refused using an in-page check before clicking/typing. This is a credential-safety guard, not a click approval step.
- Every action is logged to `logs/actions.jsonl`; downloads to `downloads/` with `logs/downloads.jsonl`.
- GUI↔MCP communication uses a local Windows named pipe (`\\.\pipe\agent-browser-<user>`), no TCP port.
- Screenshots capture physical screen pixels while the app window is foregrounded. If another window covers it, capture shows that window.
- WebView profile lives in `data/webview` (user sign-ins persist locally).

## Verified

`cargo test` (4 protocol tests), `cargo build --release`, MCP handshake + 9 tools listed,
end-to-end click → page state change (`BUTTON A CLICKED`) → screenshot evidence,
typed text (`HELLO`) into the text field, password-field click refused.

## Files

- `src/protocol.rs` — IPC command/response types, viewport validation
- `src/ipc.rs` — named-pipe GUI↔MCP transport
- `src/control.rs` — enigo input, screenshots, policy, logging
- `src/gui.rs` — tao window + two child WebViews (tab panel + page)
- `src/mcp.rs` — rmcp stdio server with 9 tools
- `tests/fixture.html`, `tests/mcp_smoke.py` — end-to-end fixtures
