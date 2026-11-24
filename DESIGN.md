# Agent Browser — design specification (v0.2)

## Direction
Dark graphite with a single cyan accent. Restrained translucency on chrome surfaces only (toolbar, terminal header), never on content pages. Purpose: a professional operator tool where the page stays the visual focus and the chrome recedes.

## Product context
Two roles in one window:
- **User**: signs in, drives the address bar, docks/resizes panels.
- **Agent (OpenCode)**: sees only the page viewport through screenshots and clicks there. It never sees or edits the toolbar/terminal panel.

Screens: terminal panel (agent session), browser toolbar (user navigation), page WebView (shared content), divider (user resize).

## Tokens (`assets/terminal/theme.css`)
| Token | Value | Role |
|---|---|---|
| `--bg` | `#0b1120` | app background |
| `--surface` | `rgba(17,28,48,0.86)` | chrome surface |
| `--surface-solid` | `#111c30` | fallback / high-contrast |
| `--line` | `#22304a` | borders, dividers |
| `--line-strong` | `#324561` | hover / focus border |
| `--text` | `#e8edf6` | primary text |
| `--muted` | `#9aabc4` | secondary text (≥4.5:1 on surface) |
| `--accent` | `#4dd8ff` | focus, selection, active state |
| `--accent-soft` | `rgba(77,216,255,0.14)` | selected surface |
| `--danger` | `#ff7a7a` | error text |
| `--radius-sm/md` | `6px / 10px` | controls / panels |
| `--space-1..4` | `4/8/12/16px` | spacing scale |
| `--shadow-panel` | `0 8px 30px rgba(0,0,0,0.35)` | panel elevation |

Typography: system stack `Segoe UI Variable, Segoe UI, Inter, system-ui, sans-serif`; terminal `Cascadia Mono, JetBrains Mono, Consolas, monospace`. Sizes: 11px labels, 12px toolbar, 13px terminal/body, 15px panel title. Weight 600 for titles, 400 body. Line-height 1.35. Letter-spacing 0.02em on uppercase labels only.

## Layout
- Window 1600×900 default, **resizable** with min 1100×680.
- Four page tabs share one WebContext. A 140 px tab strip sits on the far right; the agent viewport excludes it.
- Tab 1 defaults to an app-styled New Tab page: five popular site shortcuts (TikTok, YouTube, Facebook, Instagram, GitHub) and a clearly labeled official YouTube Premium download button. No search input; URL entry belongs in the browser toolbar.
- Layout cache: one harness-side DOM scan per page state; elements receive stable IDs (`eN`) and screen-space centres. Cache invalidates on navigation, scroll, resize, dock change, tab switch, and DOM mutation.
- Left dock: terminal `term_size` (default 540, clamped 260…w−400), divider 8px, toolbar 44px, page fills remainder.
- Bottom dock: page top, toolbar 44px, divider 8px, terminal bottom (default h/2, clamped 180…h−300).
- Toolbar internals: Back 32px, Forward 32px, Reload 32px, Home 32px, gaps 8px, URL flex `min-width:0`, status 16px. Padding 12px.
- Terminal header: 36px, title 12px uppercase, subtitle 11px muted, dock button right-aligned, min 24×24 target.

## Component states
- Buttons: default / hover (stronger border + accent text) / focus-visible (2px accent ring) / pressed (accent-soft background) / disabled (40% opacity, `aria-disabled`).
- URL input: default / focus (accent border) / invalid (danger border + inline status message).
- Dock button label reflects current dock; toolbar divider cursor changes with orientation.
- Terminal loading: PTY output buffered until xterm ready; no spinner needed, page shows blank briefly.
- Error: navigation rejection shows short status text; agent errors return readable messages.

## Responsive
- On window resize, tao emits `Resized`; layout recomputes from current dimensions and updates the agent viewport.
- Narrow windows (<1100px): left terminal capped at 38% of width; toolbar buttons shrink to 28px but keep ≥24px targets.
- Short windows (<720px): bottom terminal capped at 42% of height.
- URL input truncates with ellipsis, never pushes controls off-screen; no horizontal scroll.

## Motion
Only hover/focus color transitions (120ms ease). No entrance animations, no blurred backgrounds behind content. The virtual cursor is a static cyan ring (no easing); it is removed from the page 900 ms after the last agent action. `prefers-reduced-motion` removes transitions; state still changes via color/border.

## Accessibility
Visible focus rings, semantic buttons with `aria-label`, live region for navigation status, ≥24px targets, contrast-checked muted text, keyboard operation of all toolbar controls, reduced-motion support. Drag has a non-drag alternative (`browser_dock` / dock button).

## Update dialog
A native Windows dialog with three choices: **Yes = Update now**, **No = Remind me later**, **Cancel = Skip this version**. Triggered only by the user's "Check for updates" button in the terminal header. Message shows the new version and truncated release notes. "Skip" persists the tag in `data/update-state.json`. Download is SHA-256 verified against the GitHub asset digest before any installer runs.

## Acceptance
- Rendered window matches tokens and spacing at 1600×900, 1100×700, and bottom dock.
- Resizing updates both WebViews and the agent viewport without overlap.
- Toolbar controls work with mouse and keyboard; focus visible.
- Reduced-motion setting removes transitions without losing feedback.
