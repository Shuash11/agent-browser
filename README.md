# Agent Browser

A local Windows desktop app with an **embedded OpenCode terminal** (xterm.js + ConPTY) and a page WebView. OpenCode controls the page through MCP over stdio. You sign in yourself; the agent acts on web pages and can reach OS coordinates when `allow_outside_viewport` is enabled (the default), but terminal chrome is excluded.

## Layout

- Terminal panel docks **left** (default, 540 px) or **bottom** (half height).
- Drag the 8 px divider between panels to resize; the page viewport and agent coordinates update live.
- Click **Dock: Left / Bottom** in the terminal toolbar, or call `browser_dock {mode:"left"|"bottom"|"toggle"}`.
- The terminal starts in a plain shell in the app directory so the app-local `opencode.jsonc` and MCP server are found. To work on another project, run `opencode-harness C:\path\to\project` — that wrapper keeps cwd at the app directory and passes your project as OpenCode's directory argument. Plain `cd` + `opencode` will not load the harness MCP config.
- The page panel has a **toolbar**: address bar, Back, Forward, Reload, Home. The agent can also navigate via `browser_navigate` / `browser_search` and open unlimited tabs via `browser_new_tab`.
- Tabs are created on demand. The **right-edge tab strip** shows every open tab; click a tab (user) or call `browser_focus_tab` (agent) to switch. Each row has a × to close it; closed rows show "reopen". Closing the last tab automatically opens a fresh New Tab.
- The first tab opens an app-styled **New Tab** page with no search box: seven shortcuts — TikTok, YouTube, Facebook, Instagram, GitHub, **Edge (MSN.com)**, and **Chrome (Google.com)** — all open inside the harness page panel. A **custom source** field stores your own authorized download URL in this browser profile; no third-party downloader is bundled. URL entry stays in the toolbar address bar.
- **Manual updates:** click "Check for updates" in the terminal header. The app queries the GitHub `v*` release, shows a dialog with **Update now / Remind me later / Skip this version**, verifies the installer's GitHub SHA-256 digest, then silently reinstalls into the current folder and relaunches. Skipped tags are remembered in `data/update-state.json`. Updates are user-only — no agent tool can check or install them.
- The agent uses an **in-app virtual cursor**: a cyan ring drawn on the page at the agent's logical position. The page's native pointer is hidden briefly during agent actions.
- **Terminal:** opens a plain `cmd.exe` shell in the app directory. OpenCode is not launched automatically. Run `opencode-harness C:\path\to\project` to start it with the harness config and your target project. The wrapper sets `AGENT_BROWSER_HARNESS=1` and the config injects `HARNESS.md` so the agent knows it is inside the app. Plain `opencode` in another project folder loads neither.
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
   - `browser_type {text}`, `browser_key {key}`, `browser_list_tabs`, `browser_focus_tab {index}`, `browser_close_tab {index}`, `browser_dock {mode}`
   - `browser_navigate {url}` — open a URL in the active tab. Any scheme is accepted; bare hosts get `https://`.
   - `browser_new_tab {url?}`, `browser_back`, `browser_forward`, `browser_reload` — open a tab, navigate history, or reload. New tabs create a real WebView, so there is no tab cap.
   - `browser_policy {reload?}` — read the autonomy policy; set `reload:true` after editing the file to apply changes immediately.
   - `browser_focus` — bring the harness window forward. Focus is best-effort and never blocks input tools.
   - `browser_model_status`, `browser_model_load {path}`, `browser_model_use {provider}`, `browser_model_stop` — host a local GGUF and switch OpenCode between local and cloud.
   - `browser_search {query,engine?}` — URL-encoded web search (one call). `browser_hover {id}` — hover an element and refresh layout. `browser_help {tool?}` — usage recipes and JSON examples.
   - `browser_act {steps}` — run a short validated browser sequence in one MCP call (fewer model turns). Code Mode `execute` can batch independent reads.

## Operator recipe

`AGENTS.md` contains a fixed recipe for search, read, click, stale IDs, hover, and form filling — follow it instead of inventing a strategy. ID-based clicks are DOM-targeted (they do not move your cursor); coordinate clicks still use the real cursor for canvas/drag cases. Call `browser_help {"tool":"..."}` any time to get the exact JSON shape for a tool. All 33 tools remain available; there is no reduced-tool default.

## Local model (confidential work)

Click **+ Model** in the terminal header to open the draggable model panel, then drop a `.gguf` onto it (or click to browse). The harness extracts its bundled, hash-verified llama.cpp CPU runtime and serves the model at `http://127.0.0.1:8080` — loopback only, nothing leaves the machine. Use **Use local** / **Use cloud** to switch OpenCode providers; the harness writes the matching config and reloads OpenCode. You can still type in the shell manually at any time.

A small 2B model may not reliably emit tool calls, so local mode is best for confidential drafting rather than full agent automation.

## Autonomy policy

Edit `agent-policy.json` next to `agent-browser.exe` to control what the agent may do. Missing or invalid values fall back to safe defaults:

| Key | Default | Meaning |
|---|---|---|
| `allow_password_entry` | `false` | Let the agent type into password fields — the only remaining guard |
| `allow_outside_viewport` | `true` | Pointer actions may target coordinates outside the page |
| `allow_close_last_tab` | `true` | Let the agent close the last remaining tab |
| `allow_new_tabs` | `true` | Let the agent open new tabs |
| `allow_url_navigation` | `true` | Let the agent navigate to any parseable URL |
| `fresh_layout_actions` | `true` | Rescan the DOM immediately before acting on a layout ID |

Changes take effect with `browser_policy {reload:true}` — no restart needed. Missing or invalid values fall back to the defaults above. With `fresh_layout_actions` on, `browser_click_id` and `browser_type_id` rescan the DOM before acting, so scrolling never leaves them using stale coordinates.

## Behavior and limits

- The agent can navigate, open unlimited tabs, and use history/back/forward/reload. It cannot create OS windows. Navigating to third-party sites is done at the user's request; the harness bundles no downloader and does not bypass a site's terms — rights and safety on destination sites remain the user's responsibility.
- There are no viewport or tab-count caps, and page text / element lists are no longer truncated.
- Window focus is best-effort and never hard-blocks input. Call `browser_focus` if a click lands elsewhere.
- Password inputs remain refused via an in-page check. This is the only hard block retained. It is a credential-safety guard, not a click approval step.
- Every action is logged to `logs/actions.jsonl`; downloads to `downloads/` with `logs/downloads.jsonl`.
- GUI↔MCP communication uses a local Windows named pipe (`\\.\pipe\agent-browser-<user>`), no TCP port.
- Screenshots capture physical screen pixels while the app window is foregrounded. If another window covers it, capture shows that window.
- WebView profile lives in `data/webview` (user sign-ins persist locally).

## Verified

`cargo test` (10 unit tests), `cargo build --release`, MCP handshake with 32 tools listed,
end-to-end navigation, layout ID clicking, text entry, local model load/health, and
password-field refusal verified at runtime.

## Files

- `src/protocol.rs` — IPC command/response types, viewport validation
- `src/ipc.rs` — named-pipe GUI↔MCP transport
- `src/control.rs` — enigo input, screenshots, policy, logging
- `src/gui.rs` — tao window with terminal, model panel, toolbar, tab strip, and page WebViews
- `src/mcp.rs` — rmcp stdio server with 32 tools
- `opencode-harness.cmd` — start OpenCode with the harness config and your project directory
- `tests/fixture.html`, `tests/mcp_smoke.py` — end-to-end fixtures
