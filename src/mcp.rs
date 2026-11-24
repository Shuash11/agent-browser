//! MCP stdio server exposing cursor-level browser control to OpenCode.

use rmcp::{
    handler::server::wrapper::Parameters,
    schemars::JsonSchema,
    tool, tool_router,
    transport::stdio,
    ErrorData as McpError, ServiceExt,
};
use serde::Deserialize;

use crate::ipc;
use crate::protocol::{Command, MouseButton, Response};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PathParams {
    /// Absolute path where the PNG screenshot should be saved.
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PointParams {
    /// Screen X coordinate (must be inside the page viewport).
    pub x: i32,
    /// Screen Y coordinate (must be inside the page viewport).
    pub y: i32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClickParams {
    pub x: i32,
    pub y: i32,
    /// left, right, or middle
    pub button: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScrollParams {
    pub x: i32,
    pub y: i32,
    pub dx: i32,
    pub dy: i32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TextParams {
    pub text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct KeyParams {
    /// Enter, Tab, Escape, Backspace, or Ctrl+L
    pub key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IndexParams {
    pub index: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DockParams {
    /// left, bottom, or toggle
    pub mode: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LayoutParams {
    /// Force a fresh DOM scan instead of using the cached snapshot.
    pub refresh: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IdParams {
    /// Element ID from browser_layout, e.g. e7.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TypeIdParams {
    /// Element ID from browser_layout, e.g. e7.
    pub id: String,
    /// Text to type.
    pub text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScrollDomParams {
    /// Horizontal pixels; positive scrolls right.
    pub dx: i32,
    /// Vertical pixels; positive scrolls down.
    pub dy: i32,
}

fn pack(r: Result<Response, String>) -> Result<String, McpError> {
    match r {
        Ok(Response::Text { text }) => Ok(text),
        Ok(resp) => serde_json::to_string(&resp)
            .map_err(|e| McpError::internal_error(e.to_string(), None)),
        Err(msg) => Err(McpError::internal_error(msg, None)),
    }
}

#[derive(Clone)]
pub struct BrowserService;

#[tool_router(server_handler)]
impl BrowserService {
    #[tool(description = "Capture the visible page viewport only (not the tab panel). Returns a PNG path.")]
    async fn browser_screenshot(
        &self,
        Parameters(p): Parameters<PathParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Screenshot { path: p.path }).await)
    }

    #[tool(description = "Move the mouse cursor to a screen coordinate inside the page viewport.")]
    async fn browser_move(
        &self,
        Parameters(p): Parameters<PointParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Move { x: p.x, y: p.y }).await)
    }

    #[tool(description = "Click inside the page viewport. Password fields are refused.")]
    async fn browser_click(
        &self,
        Parameters(p): Parameters<ClickParams>,
    ) -> Result<String, McpError> {
        let button = match p.button.to_lowercase().as_str() {
            "left" => MouseButton::Left,
            "right" => MouseButton::Right,
            "middle" => MouseButton::Middle,
            other => {
                return Err(McpError::invalid_params(
                    format!("button must be left, right, or middle; got {other}"),
                    None,
                ))
            }
        };
        pack(ipc::call(Command::Click { x: p.x, y: p.y, button }).await)
    }

    #[tool(description = "Scroll at a point inside the page viewport. dy>0 scrolls down.")]
    async fn browser_scroll(
        &self,
        Parameters(p): Parameters<ScrollParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Scroll { x: p.x, y: p.y, dx: p.dx, dy: p.dy }).await)
    }

    #[tool(description = "Type text into the focused page field. Password fields are refused.")]
    async fn browser_type(
        &self,
        Parameters(p): Parameters<TextParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Type { text: p.text }).await)
    }

    #[tool(description = "Press a named key: Enter, Tab, Escape, Backspace, or Ctrl+L.")]
    async fn browser_key(
        &self,
        Parameters(p): Parameters<KeyParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Key { key: p.key }).await)
    }

    #[tool(description = "Get the page viewport rectangle in physical screen pixels.")]
    async fn browser_get_viewport(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::GetViewport).await)
    }

    #[tool(description = "List the tabs currently open in the user's panel.")]
    async fn browser_list_tabs(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::ListTabs).await)
    }

    #[tool(description = "Close a tab by index. Cannot close the last open tab.")]
    async fn browser_close_tab(
        &self,
        Parameters(p): Parameters<IndexParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::CloseTab { index: p.index }).await)
    }

    #[tool(description = "Focus an existing tab by index. Does not create tabs.")]
    async fn browser_focus_tab(
        &self,
        Parameters(p): Parameters<IndexParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::FocusTab { index: p.index }).await)
    }

    #[tool(description = "Read the harness-cached layout: all visible elements with stable IDs and screen coordinates. Prefer this over page_elements.")]
    async fn browser_layout(
        &self,
        Parameters(p): Parameters<LayoutParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Layout { refresh: p.refresh.unwrap_or(false) }).await)
    }

    #[tool(description = "Click an element by its cached layout ID (from browser_layout).")]
    async fn browser_click_id(
        &self,
        Parameters(p): Parameters<IdParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::ClickId { id: p.id }).await)
    }

    #[tool(description = "Type into an element by its cached layout ID (from browser_layout).")]
    async fn browser_type_id(
        &self,
        Parameters(p): Parameters<TypeIdParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::TypeId { id: p.id, text: p.text }).await)
    }

    #[tool(description = "Read the active page as plain text (title, URL, visible text). Cheaper than a screenshot.")]
    async fn browser_page_text(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::PageText).await)
    }

    #[tool(description = "List visible interactive elements with labels and screen-space click coordinates.")]
    async fn browser_page_elements(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::PageElements).await)
    }

    #[tool(description = "Scroll the page by pixels using JS. Positive dy scrolls down.")]
    async fn browser_scroll_dom(
        &self,
        Parameters(p): Parameters<ScrollDomParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::ScrollDom { dx: p.dx, dy: p.dy }).await)
    }

    #[tool(description = "Scroll an element containing the given text into the centre of view.")]
    async fn browser_scroll_into_view(
        &self,
        Parameters(p): Parameters<TextParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::ScrollIntoView { text: p.text }).await)
    }

    #[tool(description = "Dock the terminal panel left or bottom (or toggle).")]
    async fn browser_dock(
        &self,
        Parameters(p): Parameters<DockParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Dock { mode: p.mode }).await)
    }
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let service = BrowserService.serve(stdio()).await?;
    // A stdio client disconnect is normal when OpenCode reloads tools.
    // Do not turn it into a non-zero process exit (reported as code 255).
    let _ = service.waiting().await;
    Ok(())
}
