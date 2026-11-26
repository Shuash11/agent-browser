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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UrlParams {
    /// Absolute URL (any scheme), or a bare host such as example.com.
    pub url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NewTabParams {
    /// Optional URL (or bare host) to load in the new tab.
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PolicyParams {
    /// Re-read agent-policy.json before returning it.
    pub reload: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ModelPathParams {
    /// Absolute path to a .gguf model file.
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ModelProviderParams {
    /// cloud or local
    pub provider: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Search text. URL-encoded automatically.
    pub query: String,
    /// google (default), bing, or ddg
    pub engine: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HelpParams {
    /// Tool name to look up; omit to list all tools.
    pub tool: Option<String>,
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Compact, copyable usage reference for weak tool-calling models.
fn help_json(filter: Option<&str>) -> String {
    let entries: &[(&str, &str, &str)] = &[
        ("browser_layout", "List interactive elements with IDs.", "{\"refresh\":false}"),
        ("browser_click_id", "Click an element by ID from browser_layout.", "{\"id\":\"e3\"}"),
        ("browser_type_id", "Type text into an element by ID.", "{\"id\":\"e5\",\"text\":\"hello\"}"),
        ("browser_page_text", "Read page title, URL and text.", "{}"),
        ("browser_page_elements", "Raw interactive element list.", "{}"),
        ("browser_hover", "Hover an element by ID and refresh layout.", "{\"id\":\"e4\"}"),
        ("browser_search", "Search the web; query is encoded for you.", "{\"query\":\"local model guide\"}"),
        ("browser_navigate", "Open a URL in the active tab.", "{\"url\":\"https://example.com\"}"),
        ("browser_new_tab", "Open a new tab, optionally at a URL.", "{\"url\":\"https://example.com\"}"),
        ("browser_back", "Go back in history.", "{}"),
        ("browser_forward", "Go forward in history.", "{}"),
        ("browser_reload", "Reload the active tab.", "{}"),
        ("browser_list_tabs", "List open tabs.", "{}"),
        ("browser_focus_tab", "Focus a tab by index.", "{\"index\":0}"),
        ("browser_close_tab", "Close a tab by index.", "{\"index\":2}"),
        ("browser_scroll_dom", "Scroll by pixels; dy>0 scrolls down.", "{\"dx\":0,\"dy\":300}"),
        ("browser_scroll_into_view", "Scroll to text into centre view.", "{\"text\":\"Submit\"}"),
        ("browser_move", "Move the real mouse to a screen point.", "{\"x\":800,\"y\":400}"),
        ("browser_click", "Click a screen point.", "{\"x\":800,\"y\":400,\"button\":\"left\"}"),
        ("browser_type", "Type into the focused field.", "{\"text\":\"hello\"}"),
        ("browser_key", "Press a key or chord.", "{\"key\":\"Enter\"}"),
        ("browser_get_viewport", "Get the page rectangle.", "{}"),
        ("browser_screenshot", "Save a PNG of the page.", "{\"path\":\"C:\\\\temp\\\\page.png\"}"),
        ("browser_dock", "Dock terminal left or bottom.", "{\"mode\":\"left\"}"),
        ("browser_focus", "Bring the harness window forward.", "{}"),
        ("browser_policy", "Read policy; reload=true re-reads file.", "{\"reload\":false}"),
        ("browser_model_status", "Read local model status.", "{}"),
        ("browser_model_load", "Load a .gguf and switch to local.", "{\"path\":\"C:\\\\models\\\\m.gguf\"}"),
        ("browser_model_use", "Switch cloud or local provider.", "{\"provider\":\"cloud\"}"),
        ("browser_model_stop", "Stop the local model server.", "{}"),
        ("browser_help", "Show this reference; filter by tool name.", "{\"tool\":\"browser_click_id\"}"),
        ("browser_scroll", "Scroll at a point (dy>0 down).", "{\"x\":800,\"y\":400,\"dx\":0,\"dy\":300}"),
    ];
    let selected: Vec<serde_json::Value> = entries
        .iter()
        .filter(|(name, _, _)| filter.map(|f| f.eq_ignore_ascii_case(name)).unwrap_or(true))
        .map(|(name, usage, example)| serde_json::json!({
            "tool": name, "use": usage, "example": example,
        }))
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "recipe": [
            "search -> browser_search {query}",
            "read -> browser_page_text, then browser_layout",
            "click -> browser_click_id {id}; if id rejected, browser_layout {refresh:true}",
            "hover -> browser_hover {id}, then browser_click_id on the revealed menu",
            "fill -> browser_type_id {id,text}, then browser_key {key:\"Enter\"}"
        ],
        "tools": selected,
    }))
    .unwrap_or_else(|_| "{\"tools\":[]}".to_string())
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
    #[tool(description = "Save page PNG. {\"path\":\"C:\\\\temp\\\\p.png\"}")]
    async fn browser_screenshot(
        &self,
        Parameters(p): Parameters<PathParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Screenshot { path: p.path }).await)
    }

    #[tool(description = "Move real mouse. {\"x\":800,\"y\":400}")]
    async fn browser_move(
        &self,
        Parameters(p): Parameters<PointParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Move { x: p.x, y: p.y }).await)
    }

    #[tool(description = "Click point. {\"x\":800,\"y\":400,\"button\":\"left\"}")]
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

    #[tool(description = "Scroll at point (dy>0 down). {\"x\":800,\"y\":400,\"dx\":0,\"dy\":300}")]
    async fn browser_scroll(
        &self,
        Parameters(p): Parameters<ScrollParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Scroll { x: p.x, y: p.y, dx: p.dx, dy: p.dy }).await)
    }

    #[tool(description = "Type into focused field. {\"text\":\"hello\"}")]
    async fn browser_type(
        &self,
        Parameters(p): Parameters<TextParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Type { text: p.text }).await)
    }

    #[tool(description = "Press key or chord. {\"key\":\"Ctrl+C\"}")]
    async fn browser_key(
        &self,
        Parameters(p): Parameters<KeyParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Key { key: p.key }).await)
    }

    #[tool(description = "Get page rectangle. {}")]
    async fn browser_get_viewport(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::GetViewport).await)
    }

    #[tool(description = "List open tabs. {}")]
    async fn browser_list_tabs(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::ListTabs).await)
    }

    #[tool(description = "Close a tab. {\"index\":2}")]
    async fn browser_close_tab(
        &self,
        Parameters(p): Parameters<IndexParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::CloseTab { index: p.index }).await)
    }

    #[tool(description = "Focus a tab. {\"index\":0}")]
    async fn browser_focus_tab(
        &self,
        Parameters(p): Parameters<IndexParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::FocusTab { index: p.index }).await)
    }

    #[tool(description = "List elements with IDs. {\"refresh\":false}")]
    async fn browser_layout(
        &self,
        Parameters(p): Parameters<LayoutParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Layout { refresh: p.refresh.unwrap_or(false) }).await)
    }

    #[tool(description = "Click by layout ID. {\"id\":\"e3\"}")]
    async fn browser_click_id(
        &self,
        Parameters(p): Parameters<IdParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::ClickId { id: p.id }).await)
    }

    #[tool(description = "Type into layout ID. {\"id\":\"e5\",\"text\":\"hello\"}")]
    async fn browser_type_id(
        &self,
        Parameters(p): Parameters<TypeIdParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::TypeId { id: p.id, text: p.text }).await)
    }

    #[tool(description = "Read page title, URL and text. {}")]
    async fn browser_page_text(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::PageText).await)
    }

    #[tool(description = "Raw interactive elements. {}")]
    async fn browser_page_elements(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::PageElements).await)
    }

    #[tool(description = "JS scroll by pixels. {\"dx\":0,\"dy\":300}")]
    async fn browser_scroll_dom(
        &self,
        Parameters(p): Parameters<ScrollDomParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::ScrollDom { dx: p.dx, dy: p.dy }).await)
    }

    #[tool(description = "Scroll to matching text. {\"text\":\"Submit\"}")]
    async fn browser_scroll_into_view(
        &self,
        Parameters(p): Parameters<TextParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::ScrollIntoView { text: p.text }).await)
    }

    #[tool(description = "Dock terminal. {\"mode\":\"left\"}")]
    async fn browser_dock(
        &self,
        Parameters(p): Parameters<DockParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Dock { mode: p.mode }).await)
    }

    #[tool(description = "Open a URL in active tab. {\"url\":\"https://example.com\"}")]
    async fn browser_navigate(
        &self,
        Parameters(p): Parameters<UrlParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::Navigate { url: p.url }).await)
    }

    #[tool(description = "Open a new tab. {\"url\":\"https://example.com\"}")]
    async fn browser_new_tab(
        &self,
        Parameters(p): Parameters<NewTabParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::NewTab { url: p.url }).await)
    }

    #[tool(description = "Go back in history. {}")]
    async fn browser_back(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::Back).await)
    }

    #[tool(description = "Go forward in history. {}")]
    async fn browser_forward(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::Forward).await)
    }

    #[tool(description = "Reload the active tab. {}")]
    async fn browser_reload(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::Reload).await)
    }

    #[tool(description = "Read policy. {\"reload\":false}")]
    async fn browser_policy(
        &self,
        Parameters(p): Parameters<PolicyParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::GetPolicy { reload: p.reload.unwrap_or(false) }).await)
    }

    #[tool(description = "Bring harness window forward. {}")]
    async fn browser_focus(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::Focus).await)
    }

    #[tool(description = "Read local model status. {}")]
    async fn browser_model_status(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::ModelStatus).await)
    }

    #[tool(description = "Load a GGUF file. {\"path\":\"C:\\\\models\\\\m.gguf\"}")]
    async fn browser_model_load(
        &self,
        Parameters(p): Parameters<ModelPathParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::ModelLoad { path: p.path }).await)
    }

    #[tool(description = "Switch provider. {\"provider\":\"cloud\"}")]
    async fn browser_model_use(
        &self,
        Parameters(p): Parameters<ModelProviderParams>,
    ) -> Result<String, McpError> {
        pack(ipc::call(Command::ModelUse { provider: p.provider }).await)
    }

    #[tool(description = "Stop local model server. {}")]
    async fn browser_model_stop(&self) -> Result<String, McpError> {
        pack(ipc::call(Command::ModelStop).await)
    }

    #[tool(description = "Search the web. Example: {\"query\":\"local model guide\"}")]
    async fn browser_search(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<String, McpError> {
        let base = match p.engine.as_deref().unwrap_or("google").to_ascii_lowercase().as_str() {
            "bing" => "https://www.bing.com/search?q=",
            "ddg" | "duckduckgo" => "https://duckduckgo.com/?q=",
            _ => "https://www.google.com/search?q=",
        };
        pack(ipc::call(Command::Navigate { url: format!("{base}{}", url_encode(&p.query)) }).await)
    }

    #[tool(description = "Hover an element by ID and refresh layout. Example: {\"id\":\"e4\"}")]
    async fn browser_hover(
        &self,
        Parameters(p): Parameters<IdParams>,
    ) -> Result<String, McpError> {
        let layout = pack(ipc::call(Command::Layout { refresh: false }).await)?;
        let value: serde_json::Value = serde_json::from_str(&layout)
            .map_err(|e| McpError::internal_error(format!("bad layout: {e}"), None))?;
        let element = value
            .get("elements")
            .and_then(|e| e.as_array())
            .and_then(|items| {
                items.iter().find(|item| {
                    item.get("id").and_then(|v| v.as_str()) == Some(p.id.as_str())
                })
            })
            .ok_or_else(|| {
                McpError::invalid_params(
                    format!("unknown layout id {}; call browser_layout first", p.id),
                    None,
                )
            })?;
        let x = element.get("sx").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let y = element.get("sy").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let _ = pack(ipc::call(Command::Move { x, y }).await)?;
        pack(ipc::call(Command::Layout { refresh: true }).await)
    }

    #[tool(description = "Show usage recipes and JSON examples. Example: {\"tool\":\"browser_click_id\"}")]
    async fn browser_help(
        &self,
        Parameters(p): Parameters<HelpParams>,
    ) -> Result<String, McpError> {
        Ok(help_json(p.tool.as_deref()))
    }
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let service = BrowserService.serve(stdio()).await?;
    // A stdio client disconnect is normal when OpenCode reloads tools.
    // Do not turn it into a non-zero process exit (reported as code 255).
    let _ = service.waiting().await;
    Ok(())
}
