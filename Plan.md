# Plan — Agent Browser (Rust, MCP-controlled)

## Goal and acceptance criteria
A Windows desktop app with a **left side panel of tabs** and a **main WebView page**, where the user signs in manually. OpenCode controls it through a **local MCP stdio server** using screenshot + mouse + scroll + keyboard tools. The agent may act **only inside the currently visible page panel**; it cannot create tabs/windows or navigate on its own.

Acceptance:
1. `cargo build --release` succeeds on Rust 1.99 / Windows 11.
2. Launching `agent-browser` opens a window with a tab strip side panel and a working WebView.
3. OpenCode can call MCP tools to screenshot the page viewport, move/click, scroll, and type text/URLs; every action is reflected in the GUI.
4. Actions outside the content viewport are rejected; password-field typing is rejected.
5. Every action is appended to a local JSONL log.
6. End-to-end test on a local HTML fixture proves click → visible state change → screenshot evidence.

## Confirmed findings / assumptions
- Confirmed: Rust 1.99.1 and Cargo 1.99.1 installed; Windows 11 target.
- Confirmed: repo already uses `opencode.jsonc` with local MCP servers started via command arrays (no network port required).
- Assumed: WebView2 runtime is present on Windows 11 (to be confirmed by research/build).
- Pending: researcher report on `wry`/`tao`/`enigo`/screenshot/MCP crate APIs and versions.

## Verified crate versions (research + crates.io)
wry 0.57.3, tao 0.37.3, enigo 0.6.1, screenshots 0.8.7, rmcp 3.11.0, tokio 1.54.1, windows-sys 0.61, url 2, serde/serde_json 1. `build_as_child` + `with_bounds` verified for two WebViews in one tao window; `evaluate_script_with_callback` runs in cross-origin top-level pages; Tokio named pipes used for local IPC.

## Chosen approach
Single Rust binary with two subcommands:
- `agent-browser` — GUI process. Owns the tao window(s), wry WebViews, tab manager, and an IPC listener.
- `agent-browser mcp` — MCP stdio server launched by OpenCode. Exposes browser tools; forwards commands to the GUI over a **local named pipe** (no TCP exposure).

Modules (idiomatic Rust modules with clear ownership, no forced OOP hierarchy):
- `app` — window/event loop, WebView lifecycle, IPC thread.
- `tabs` — tab list, selection, per-tab URL/title; side-panel state.
- `control` — viewport bounds, action validation, enigo input, screenshot capture.
- `policy` — viewport scoping, credential-field block, action log.
- `ipc` — serde message enum + named pipe transport.
- `mcp_server` — JSON-RPC tool definitions over stdio.

## Ordered work packages
1. Research crate/API details (background specialist) → fill in versions and skeletons here.
2. Project scaffold + protocol/IPC types + tests for message round-trip.
3. GUI shell: root window, tab-panel child window, content child window, tab add/select/close.
4. Control layer: screenshot region, click/move/scroll/type via enigo, viewport validation, password check, JSONL logging.
5. MCP stdio server: tools with JSON schemas, forwarding over named pipe, error responses.
6. Integration: build, run local fixture page, drive it via MCP tools, inspect screenshots.
7. Register MCP server in `opencode.jsonc` (only after passing tests) and document usage.

## Delegation rationale
- `researcher` verifies external crate APIs/versions (fresh context, authoritative docs) — lead does not duplicate.
- Remaining implementation done by lead with incremental checks due to tight coupling between window, WebView, IPC, and input layers.

## Risks and verification
- **Credential safety (must mitigate):** agent never receives cookies/passwords; typing into `input[type=password]` rejected via in-page JS check before keystrokes. No generic JS-eval tool exposed.
- **Scope violation:** all coordinates checked against content viewport; no tab/window creation command; no navigate command.
- **Windows focus/input races:** bring content window to foreground before input; verify with screenshot after each action.
- **WebView2 dependency:** verify runtime present; if missing, app reports clearly.
- Verification: cargo test (unit), cargo build --release, manual fixture-page run with screenshot evidence.

## Completion conditions
All acceptance criteria met with observed evidence; no unverified claims; limitations stated.

## Status (2026-11-24)
Rebuilt the left panel as an embedded OpenCode terminal: MIT-licensed xterm.js 6.0.0 + addon-fit 0.11.0 assets under `assets/terminal/`, portable-pty 0.9.0 ConPTY, custom protocol `abterm` for IPC (file:// does not deliver IPC), output buffering until the page is ready, PTY resize from xterm fit. Added a draggable 8 px splitter and left/bottom docking with live viewport updates. Verified: OpenCode TUI renders in the panel, MCP click/type still work, `browser_dock bottom` changed the page viewport to 1600×442 and rendered the page above the terminal.

## Earlier status (2026-11-23)
Implemented and verified: cargo test 4/4; cargo build --release; MCP handshake with 9 tools; end-to-end click changed fixture status to BUTTON A CLICKED; typing produced HELLO in the text field; password-field click refused; viewport-only screenshots; JSONL action log. Registered in `opencode.jsonc` as `agent-browser` (restart OpenCode to load). Inline-image tool results were attempted and rejected by rmcp's `#[tool]` return-type constraints; the working String-returning tools were restored. Known limitations: no true live video stream — the agent sees discrete screenshots on demand; no navigate/create-tab tool by design.
