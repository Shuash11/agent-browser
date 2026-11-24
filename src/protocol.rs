//! IPC protocol between the MCP bridge process and the GUI process.
//!
//! All coordinates are in *screen* pixels. The GUI validates them against the
//! content WebView viewport before executing any input action.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Viewport {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Capture the current content viewport to a PNG file; returns its path.
    Screenshot { path: String },
    Move { x: i32, y: i32 },
    Click { x: i32, y: i32, button: MouseButton },
    Scroll { x: i32, y: i32, dx: i32, dy: i32 },
    /// Type text at the current focus. Password fields are rejected by policy.
    Type { text: String },
    /// Press a named key (e.g. "Enter", "Ctrl+L", "Escape").
    Key { key: String },
    ListTabs,
    FocusTab { index: usize },
    GetViewport,
    /// Dock the terminal panel left or bottom (or toggle).
    Dock { mode: String },
    /// Visible page text as plain text (cheaper than a screenshot).
    PageText,
    /// Visible interactive elements with screen-space centres.
    PageElements,
    /// Scroll the document with JS pixels instead of OS wheel ticks.
    ScrollDom { dx: i32, dy: i32 },
    /// Scroll an element containing the given text into view.
    ScrollIntoView { text: String },
    /// Return the harness-cached layout snapshot (optionally refreshed).
    Layout { refresh: bool },
    /// Click an element by its cached layout ID.
    ClickId { id: String },
    /// Type into an element by its cached layout ID.
    TypeId { id: String, text: String },
    /// Close a tab by its stable index.
    CloseTab { index: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok { message: String },
    Screenshot { path: String, width: u32, height: u32 },
    Tabs { tabs: Vec<TabInfo> },
    Viewport { viewport: Viewport },
    /// Text result from a page query (JSON string for structured results).
    Text { text: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TabInfo {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub active: bool,
}

/// Actions that carry coordinates and must be checked against the viewport.
pub fn action_point(cmd: &Command) -> Option<(i32, i32)> {
    match cmd {
        Command::Move { x, y }
        | Command::Click { x, y, .. }
        | Command::Scroll { x, y, .. } => Some((*x, *y)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_contains_strict_bounds() {
        let v = Viewport { x: 100, y: 200, width: 800, height: 600 };
        assert!(v.contains(100, 200));
        assert!(v.contains(899, 799));
        assert!(!v.contains(99, 200));
        assert!(!v.contains(900, 799));
        assert!(!v.contains(500, 800));
    }

    #[test]
    fn coordinate_actions_are_identified() {
        let click = Command::Click { x: 5, y: 9, button: MouseButton::Left };
        assert_eq!(action_point(&click), Some((5, 9)));
        assert_eq!(action_point(&Command::Type { text: "hi".into() }), None);
    }

    #[test]
    fn command_round_trip() {
        let cmd = Command::Click { x: 12, y: 34, button: MouseButton::Right };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn response_round_trip() {
        let r = Response::Error { message: "outside viewport".into() };
        let json = serde_json::to_string(&r).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
