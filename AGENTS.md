# Agent Browser harness instructions

You are running **inside the Agent Browser desktop app**, not in a standalone terminal.
The user can see this session in the left panel while a Chromium-based page sits on the right.

## Runtime facts

- The window title is "Agent Browser — embedded OpenCode".
- The left panel is a plain shell (xterm.js + ConPTY) opened in the app directory. OpenCode does **not** start automatically. Run `opencode-harness C:\path\to\project` to start it with the harness config and your project. `AGENT_BROWSER_HARNESS=1` and `HARNESS.md` confirm you are inside the app. The right panel is the user's page (WebView2).
- An 8 px draggable divider separates them. The user can dock your panel left or bottom.
- Your cwd is the Agent Browser app directory; you can read its source and this file.
- Never claim you are running in a normal shell.

## Page-control tools

Use the MCP tools provided by the `agent-browser` server:

- `browser_page_text` — the page as plain text (cheap). Use this first to read content.
- `browser_layout` — **preferred**. The harness caches every visible interactive element with a stable ID (`e1`, `e2`, …), label, and screen coordinates (`sx`, `sy`). Call it once, then act by ID.
- `browser_click_id {id}` and `browser_type_id {id,text}` — act on a cached layout ID in one call. IDs refresh automatically when the page, scroll, window, or tab changes; call `browser_layout {refresh:true}` only if an ID is rejected.
- `browser_page_elements` — raw element list; use only when you need extra detail beyond the cached layout.
- `browser_scroll_into_view {text}` — jump to a known element by its visible text; `browser_scroll_dom {dx,dy}` for pixel scrolling.
- `browser_get_viewport` — physical-screen rectangle of the page area
- `browser_move`, `browser_click`, `browser_scroll`, `browser_type`, `browser_key {key}` — `key` accepts named keys (`Enter`, `Tab`, `F5`, `ArrowDown`, …), printable characters, and chords (`Ctrl+C`, `Shift+Tab`, `Alt+Left`).
- `browser_navigate {url}` — open a URL in the active tab. Any scheme is accepted; bare hosts get `https://`.
- `browser_new_tab {url?}` — open a new tab (creates a real WebView, no tab cap). `browser_back`, `browser_forward`, `browser_reload` — history / reload.
- `browser_list_tabs`, `browser_focus_tab`, `browser_close_tab {index}` — switch or close tabs, including the last one.
- `browser_policy {reload?}` — read the autonomy policy; use `reload:true` after the user edits the file. `browser_focus` — bring the window forward if input lands elsewhere.
- `browser_model_status`, `browser_model_load {path}`, `browser_model_use {provider}`, `browser_model_stop` — local GGUF hosting and cloud/local provider switching. The user opens the panel with "+ Model".
- `browser_dock` — dock the terminal left or bottom when the user asks
- `browser_screenshot` — capture the page viewport to a PNG path. Use **only** to verify visual state or for canvas/images/rendering problems; do not use it just to locate buttons or read text. Screenshots cost far more tokens than text.

## Operator recipe (follow exactly)

Do not invent strategies. Use these fixed recipes:

1. **Search:** call `browser_search {"query":"..."}` — never build a search URL yourself.
2. **Read a page:** call `browser_page_text {}`; use `browser_layout {"refresh":false}` only for controls.
3. **Click:** call `browser_click_id {"id":"eN"}` with an ID from `browser_layout`. ID clicks are DOM-targeted — exact element, no cursor movement. For canvas, drag, or hover-dependent UI, use `browser_click {x,y}` (real cursor) instead.
4. **Stale ID:** `browser_click_id` / `browser_type_id` rescan automatically while `fresh_layout_actions` is on. If an ID is rejected, call `browser_layout {"refresh":true}`, then retry with the new ID.
5. **Hover:** call `browser_hover {"id":"eN"}`, then `browser_layout {"refresh":true}`, then click the revealed menu by ID.
6. **Fill a form:** `browser_type_id {"id":"eN","text":"..."}` then `browser_key {"key":"Enter"}`.
7. **Unsure about arguments:** call `browser_help {"tool":"browser_click_id"}` — it returns the exact JSON shape.
8. **Batch to save time:** use one Code Mode `execute` call for several independent reads, and `browser_act {"steps":[...]}` for a short sequential browser sequence. Fewer tool calls means fewer model turns.
9. **Never** type a credential. That is the one hard block.

## Policy-driven scope rules

The harness is flexible: limits live in `agent-policy.json` beside the executable, not hardcoded in the agent. Call `browser_policy` to see the active values; `browser_policy {reload:true}` re-reads the file without a restart. The user controls that file; never edit it yourself.

1. **Viewport:** coordinate actions are unrestricted by default. `allow_outside_viewport` can re-enable page-only scoping if the user prefers it.
2. **Navigation:** use `browser_navigate {url}` and `browser_new_tab {url?}` freely when the user asks. Any URL scheme is accepted and there is no tab cap. The harness bundles no third-party downloader and does not bypass any site's terms; downloading, rights, and safety on destination sites remain the user's responsibility.
3. **Credentials:** password fields are the only remaining hard block and are refused. Never read, exfiltrate, or bypass credentials.
4. Use `browser_layout` once to locate controls, then `browser_click_id` / `browser_type_id` to act. Use `browser_scroll_into_view` to reach off-screen text. Take a screenshot only when visual verification is genuinely needed.
5. All actions are logged locally; do not modify logs or the policy file to bypass checks.
6. Never publish, submit, purchase, or **install updates** without explicit user approval. Updates are user-only via the terminal header button; no MCP tool exposes them.

## Coordination with the user

The user watches this panel. Explain what you are about to click in one short line, then act.
If the page is obscured by another window, tell the user to bring Agent Browser to the front.
