//! GUI shell: embedded OpenCode terminal, browser toolbar, multi-tab page area,
//! right-edge tab strip, draggable splitter, and a virtual agent cursor.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tao::dpi::LogicalSize as TaoLogicalSize;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use url::Url;
use wry::dpi::{LogicalPosition, LogicalSize, Position, Size};
use wry::WebContext;
use wry::WebViewBuilder;

fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char)
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> wry::Rect {
    wry::Rect {
        position: Position::Logical(LogicalPosition::new(x as f64, y as f64)),
        size: Size::Logical(LogicalSize::new(w as f64, h as f64)),
    }
}

/// Normalize a user- or agent-supplied destination into a URL. Any scheme is
/// accepted (file, data, mailto, about, custom); bare hosts get `https://`.
/// An empty destination is the only thing rejected. Shared by the toolbar and
/// the MCP navigation tools so there is a single validation path.
fn navigation_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let has_scheme = trimmed.contains("://")
        || ["about:", "data:", "mailto:", "tel:", "file:"]
            .iter()
            .any(|p| trimmed.to_ascii_lowercase().starts_with(p));
    let candidate = if has_scheme {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    Url::parse(&candidate).ok().map(|u| u.to_string())
}

const WINDOW_W: i32 = 1600;
const WINDOW_H: i32 = 900;
const MIN_WINDOW_W: u32 = 1100;
const MIN_WINDOW_H: u32 = 680;
const TERMINAL_W: i32 = 500;
const DIVIDER_W: i32 = 8;
const TAB_W: i32 = 140;
const MIN_TERM: i32 = 240;
const TOOLBAR_H: i32 = 44;

fn project_dir() -> PathBuf {
    // Runtime resources live beside the executable: <root>/target/release/agent-browser.exe
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().and_then(|a| a.parent()).and_then(|b| b.parent()).map(|c| c.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn agent_cursor_js(fx: f64, fy: f64) -> String {
    format!(
        r#"(function(){{
var c=window.__abCursor;
if(!c){{c=document.createElement('div');c.id='__abCursor';
c.style.cssText='position:fixed;z-index:2147483647;width:18px;height:18px;border:2px solid #4dd8ff;border-radius:50%;box-shadow:0 0 0 5px rgba(77,216,255,.14),0 4px 14px rgba(0,0,0,.45);pointer-events:none;transform:translate(-50%,-50%);';
document.documentElement.appendChild(c);window.__abCursor=c;}}
c.style.left=({fx}*window.innerWidth)+'px';c.style.top=({fy}*window.innerHeight)+'px';
var s=document.getElementById('__abHide');
if(!s){{s=document.createElement('style');s.id='__abHide';s.textContent='*{{cursor:none !important}}';document.documentElement.appendChild(s);}}
clearTimeout(window.__abHideTimer);
window.__abHideTimer=setTimeout(function(){{
if(s.parentNode)s.parentNode.removeChild(s);
if(c.parentNode)c.parentNode.removeChild(c);
window.__abCursor=null;
}},900);
}})()"#
    )
}

#[derive(Clone, serde::Serialize)]
struct LayoutEntry {
    id: String,
    tag: String,
    role: String,
    label: String,
    sx: i32,
    sy: i32,
    w: i32,
    h: i32,
}

#[derive(Default)]
struct LayoutCache {
    tab: usize,
    url: String,
    entries: Vec<LayoutEntry>,
    dirty: bool,
}

fn layout_json(cache: &LayoutCache) -> String {
    serde_json::json!({
        "tab": cache.tab,
        "url": cache.url,
        "count": cache.entries.len(),
        "elements": cache.entries,
    })
    .to_string()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message_box(text: &str, caption: &str, flags: u32) -> i32 {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, MessageBoxW};
    unsafe {
        let owner: HWND = FindWindowW(std::ptr::null(), wide(crate::control::WINDOW_TITLE).as_ptr());
        MessageBoxW(
            owner,
            wide(text).as_ptr(),
            wide(caption).as_ptr(),
            flags,
        )
    }
}

fn window_command(action: &str) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsZoomed, ShowWindow, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
    };
    let title = wide(crate::control::WINDOW_TITLE);
    unsafe {
        let hwnd: HWND = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd.is_null() { return; }
        match action {
            "minimize" => { ShowWindow(hwnd, SW_MINIMIZE); }
            "maximize" => {
                if IsZoomed(hwnd) != 0 { ShowWindow(hwnd, SW_RESTORE); }
                else { ShowWindow(hwnd, SW_MAXIMIZE); }
            }
            _ => {}
        }
    }
}

fn serve_abterm(assets: &Path, request: &wry::http::Request<Vec<u8>>) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    let path = request.uri().path();
    let (file, mime): (&str, &str) = if let Some(name) = path.strip_prefix("/icons/") {
        let safe = name.rsplit('/').next().unwrap_or("");
        if safe.contains("..") || !safe.ends_with(".svg") {
            ("index.html", "text/html")
        } else {
            let owned: &'static str = Box::leak(format!("../icons/{safe}").into_boxed_str());
            (owned, "image/svg+xml")
        }
    } else {
        match path {
            "/xterm.min.js" => ("xterm.min.js", "application/javascript"),
            "/xterm.min.css" => ("xterm.min.css", "text/css"),
            "/addon-fit.min.js" => ("addon-fit.min.js", "application/javascript"),
            "/divider.html" => ("divider.html", "text/html"),
            "/toolbar.html" => ("toolbar.html", "text/html"),
            "/newtab.html" => ("newtab.html", "text/html"),
            "/tabs.html" => ("tabs.html", "text/html"),
            "/model.html" => ("model.html", "text/html"),
            "/icons.css" => ("icons.css", "text/css"),
            "/theme.css" => ("theme.css", "text/css"),
            _ => ("index.html", "text/html"),
        }
    };
    let bytes = std::fs::read(assets.join(file)).unwrap_or_default();
    wry::http::Response::builder()
        .header("content-type", mime)
        .header("cache-control", "no-store")
        .body(std::borrow::Cow::Owned(bytes))
        .unwrap()
}

fn push_model_status(panel: &wry::WebView, model: &crate::local_model::LocalModel) {
    if let Ok(js) = serde_json::to_string(&model.status()) {
        let _ = panel.evaluate_script(&format!("window.__setModelStatus({js})"));
    }
}

#[allow(clippy::too_many_arguments)]
fn build_tab(
    context: &mut WebContext,
    window: &tao::window::Window,
    url: &str,
    index: usize,
    browser_proxy: tao::event_loop::EventLoopProxy<crate::ipc::UserEvent>,
    tab_ipc_proxy: tao::event_loop::EventLoopProxy<crate::ipc::UserEvent>,
    term_dir: &PathBuf,
    downloads: &PathBuf,
    logs: &PathBuf,
) -> Result<wry::WebView, Box<dyn std::error::Error>> {
    let tab_assets = term_dir.clone();
    let dl_dir = downloads.clone();
    let lg_dir = logs.clone();
    Ok(WebViewBuilder::new_with_web_context(context)
        .with_initialization_script(SHADOW_TRACK_JS)
        .with_bounds(rect(TERMINAL_W + DIVIDER_W, TOOLBAR_H,
            WINDOW_W - TERMINAL_W - DIVIDER_W - TAB_W, WINDOW_H - TOOLBAR_H))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            serve_abterm(&tab_assets, &request)
        })
        .with_url(url)
        .with_on_page_load_handler(move |_event, loaded| {
            let _ = browser_proxy.send_event(crate::ipc::UserEvent::PageLoaded {
                url: loaded, tab: index,
            });
        })
        .with_download_started_handler(move |_url, path| {
            let p = path;
            *p = dl_dir.join(p.file_name().unwrap_or_default());
            true
        })
        .with_download_completed_handler(move |url, path, ok| {
            let line = format!("{{\"url\":{:?},\"path\":{:?},\"ok\":{}}}\n", url,
                path.as_ref().map(|p| p.to_string_lossy().to_string()), ok);
            let _ = fs::OpenOptions::new().create(true).append(true)
                .open(lg_dir.join("downloads.jsonl"))
                .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
        })
        .with_ipc_handler(move |request| {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(request.body()) {
                let kind = msg.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if kind == "layout_changed" {
                    let _ = tab_ipc_proxy.send_event(crate::ipc::UserEvent::LayoutDirty);
                    return;
                }
                let value = msg.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                let _ = tab_ipc_proxy.send_event(crate::ipc::UserEvent::BrowserAction {
                    kind, value, tab: Some(index),
                });
            }
        })
        .build_as_child(window)?)
}

fn page_text_js() -> String {
    "(function(){return JSON.stringify({title:document.title,url:location.href,\
     text:(document.body&&document.body.innerText||'')});})()".to_string()
}

fn page_elements_js() -> String {
    "(function(){var out=[];var els=document.querySelectorAll(\
     'a,button,input,textarea,select,[role=button],[role=link],[role=textbox],[onclick]');\
     for(var i=0;i<els.length;i++){var e=els[i];var r=e.getBoundingClientRect();\
     if(r.width<2||r.height<2||r.bottom<0||r.top>window.innerHeight)continue;\
     var s=getComputedStyle(e);if(s.visibility==='hidden'||s.display==='none'||s.opacity==='0')continue;\
     var label=(e.innerText||e.value||e.getAttribute('aria-label')||e.placeholder||'');\
     out.push({tag:e.tagName,role:e.getAttribute('role')||'',\
     label:String(label).trim(),\
     x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),\
     w:Math.round(r.width),h:Math.round(r.height)});\
     }\
     return JSON.stringify({innerWidth:window.innerWidth,innerHeight:window.innerHeight,elements:out});})()"
        .to_string()
}

fn scroll_into_view_js(text: &str) -> String {
    let needle = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        "(function(){{var n={needle};var needle=n.toLowerCase().slice(1,-1);\
         var els=document.querySelectorAll('button,a,input,textarea,select,[role=button],[role=link],h1,h2,h3,h4,h5,h6,li,td,th,label,p,span');\
         var best=null,bestArea=Infinity,bestText='';\
         for(var i=0;i<els.length;i++){{var e=els[i];\
         var t=(e.innerText||e.value||e.getAttribute('aria-label')||'').trim();\
         if(!t||t.toLowerCase().indexOf(needle)<0)continue;\
         var r=e.getBoundingClientRect();var area=r.width*r.height;\
         if(area>0&&area<bestArea){{best=e;bestArea=area;bestText=t;}}}}\
         if(best){{best.scrollIntoView({{block:'center',behavior:'instant'}});\
         return JSON.stringify({{found:true,text:bestText}});}}\
         return JSON.stringify({{found:false,text:n}});}})()"
    )
}

#[allow(clippy::too_many_arguments)]
fn layout_bounds(
    dock: &str,
    term_size: i32,
    w: i32,
    h: i32,
    terminal: &wry::WebView,
    divider: &wry::WebView,
    model_panel: &wry::WebView,
    model_divider: &wry::WebView,
    model_size: i32,
    toolbar: &wry::WebView,
    tabs: &[wry::WebView],
    tab_strip: &wry::WebView,
    origin: tao::dpi::PhysicalPosition<i32>,
    scale: f64,
) -> crate::protocol::Viewport {
    tab_strip.set_bounds(rect(w - TAB_W, 0, TAB_W, h)).ok();
    if dock == "left" {
        let max_term = (w * 32 / 100).max(MIN_TERM).min(w - TAB_W - 360);
        let ts = term_size.min(max_term).max(MIN_TERM);
        let area_x = ts + DIVIDER_W;
        let area_w = w - area_x - TAB_W;
        let mh = if model_size <= 0 { 0 } else { model_size.clamp(120, (h - 240).max(120)) };
        let gap = if mh > 0 { DIVIDER_W } else { 0 };
        terminal.set_bounds(rect(0, 0, ts, h - mh - gap)).ok();
        model_panel.set_visible(mh > 0).ok();
        model_divider.set_visible(mh > 0).ok();
        if mh > 0 {
            model_divider.set_bounds(rect(0, h - mh - gap, ts, DIVIDER_W)).ok();
            model_panel.set_bounds(rect(0, h - mh, ts, mh)).ok();
        }
        divider.set_bounds(rect(ts, 0, DIVIDER_W, h)).ok();
        toolbar.set_bounds(rect(area_x, 0, area_w, TOOLBAR_H)).ok();
        for tab in tabs { tab.set_bounds(rect(area_x, TOOLBAR_H, area_w, h - TOOLBAR_H)).ok(); }
        crate::protocol::Viewport {
            x: origin.x + (area_x as f64 * scale) as i32,
            y: origin.y + (TOOLBAR_H as f64 * scale) as i32,
            width: (area_w as f64 * scale) as i32,
            height: ((h - TOOLBAR_H) as f64 * scale) as i32,
        }
    } else {
        let max_term = (h * 40 / 100).max(180).min(h - 300);
        let ts = term_size.min(max_term).max(180);
        let area_h = h - ts - DIVIDER_W - TOOLBAR_H;
        let bottom_w = w - TAB_W;
        let mw = if model_size <= 0 { 0 } else { model_size.clamp(260, (bottom_w - 420).max(260)) };
        let gap = if mw > 0 { DIVIDER_W } else { 0 };
        terminal.set_bounds(rect(0, h - ts, bottom_w - mw - gap, ts)).ok();
        model_panel.set_visible(mw > 0).ok();
        model_divider.set_visible(mw > 0).ok();
        if mw > 0 {
            model_divider.set_bounds(rect(bottom_w - mw - gap, h - ts, DIVIDER_W, ts)).ok();
            model_panel.set_bounds(rect(bottom_w - mw, h - ts, mw, ts)).ok();
        }
        divider.set_bounds(rect(0, h - ts - DIVIDER_W, bottom_w, DIVIDER_W)).ok();
        toolbar.set_bounds(rect(0, 0, bottom_w, TOOLBAR_H)).ok();
        for tab in tabs { tab.set_bounds(rect(0, TOOLBAR_H, w - TAB_W, area_h)).ok(); }
        crate::protocol::Viewport {
            x: origin.x,
            y: origin.y + (TOOLBAR_H as f64 * scale) as i32,
            width: ((w - TAB_W) as f64 * scale) as i32,
            height: (area_h as f64 * scale) as i32,
        }
    }
}

// Runs before page scripts: records every shadow host, including closed ones,
// so the password guard can fail closed without DevTools inspection.
const SHADOW_TRACK_JS: &str = r#"(function(){
window.__abClosedHosts = new WeakSet();
// Capture pristine DOM primitives before page scripts run and expose them
// through a non-configurable global. The credential guard uses these instead
// of page-overridable methods.
try {
  var doc = document;
  var activeGet = Object.getOwnPropertyDescriptor(Document.prototype,'activeElement').get;
  var tagGet = Object.getOwnPropertyDescriptor(Element.prototype,'tagName').get;
  var typeGet = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'type').get;
  var efp = doc.elementFromPoint.bind(doc);
  var closest = Element.prototype.closest;
  var closed = function(e){try{return window.__abClosedHosts.has(e);}catch(x){return true;}};
  Object.defineProperty(window,'__abDom',{value:{
    active:function(){return activeGet.call(doc);},
    tag:function(e){return tagGet.call(e);},
    type:function(e){return typeGet.call(e);},
    efp:function(x,y){return efp(x,y);},
    closest:closest,
    closed:closed
  },writable:false,configurable:false});
} catch (e) {}
var orig = Element.prototype.attachShadow;
Element.prototype.attachShadow = function (init) {
  try { window.__abClosedHosts.add(this); } catch (e) {}
  return orig.call(this, init);
};
try {
  new MutationObserver(function () {
    window.chrome.webview.postMessage(JSON.stringify({ type: 'layout_changed' }));
  }).observe(document.documentElement, {
    childList: true, subtree: true, attributes: true, characterData: true
  });
} catch (e) {}
try { window.addEventListener('scroll', function () {
  window.chrome.webview.postMessage(JSON.stringify({ type: 'layout_changed' }));
}, true); } catch (e) {}
})();"#;

fn terminal_command() -> (String, Vec<String>) {
    // Open a plain shell in the app directory. The user decides when and where
    // to run `opencode` (e.g. after `cd` into the project they want to work on).
    let comspec = std::env::var("COMSPEC")
        .unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string());
    (comspec, Vec::new())
}

fn activate_tab(
    index: usize,
    tabs: &[wry::WebView],
    toolbar: &wry::WebView,
    urls: &[String],
) -> usize {
    let i = index.min(tabs.len().saturating_sub(1));
    for (n, tab) in tabs.iter().enumerate() {
        tab.set_visible(n == i).ok();
    }
    let _ = tabs[i].focus();
    if let Ok(js) = serde_json::to_string(&urls[i]) {
        let _ = toolbar.evaluate_script(&format!("window.__setUrl({js})"));
    }
    i
}

pub fn run_gui() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_dir();
    let data_dir = root.join("data").join("webview");
    let downloads = root.join("downloads");
    let logs = root.join("logs");
    fs::create_dir_all(&data_dir)?;
    fs::create_dir_all(&downloads)?;
    fs::create_dir_all(&logs)?;

    let event_loop = EventLoopBuilder::with_user_event().build();
    let term_proxy = event_loop.create_proxy();
    let ui_proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title(crate::control::WINDOW_TITLE)
        .with_inner_size(TaoLogicalSize::new(WINDOW_W, WINDOW_H))
        .with_min_inner_size(TaoLogicalSize::new(MIN_WINDOW_W, MIN_WINDOW_H))
        .with_resizable(true)
        .build(&event_loop)?;

    let (opencode_prog, opencode_args) = terminal_command();
    let arg_refs: Vec<&str> = opencode_args.iter().map(|s| s.as_str()).collect();
    let session = crate::terminal::TerminalSession::spawn(
        &opencode_prog, &arg_refs, &root, term_proxy,
    )?;
    let writer = session.writer_handle();
    let resize_session = session.clone();
    let term_proxy_ipc = event_loop.create_proxy();
    let term_dir = root.join("assets").join("terminal");
    let term_assets = term_dir.clone();
    let mut context = WebContext::new(Some(data_dir));

    // --- terminal panel ---
    let terminal = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(0, 0, TERMINAL_W, WINDOW_H))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            serve_abterm(&term_assets, &request)
        })
        .with_url("http://abterm.localhost/index.html")
        .with_ipc_handler(move |request| {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(request.body()) {
                match msg.get("type").and_then(|v| v.as_str()) {
                    Some("input") => {
                        if let Some(data) = msg.get("data").and_then(|v| v.as_str()) {
                            if let Ok(mut w) = writer.lock() {
                                let _ = w.write_all(data.as_bytes());
                                let _ = w.flush();
                            }
                        }
                    }
                    Some("resize") => {
                        let cols = msg.get("cols").and_then(|v| v.as_u64()).unwrap_or(100);
                        let rows = msg.get("rows").and_then(|v| v.as_u64()).unwrap_or(24);
                        resize_session.resize(cols as u16, rows as u16);
                    }
                    Some("ready") => {
                        let _ = term_proxy_ipc.send_event(crate::ipc::UserEvent::TermReady);
                    }
                    Some("dock") => {
                        let _ = term_proxy_ipc.send_event(crate::ipc::UserEvent::SetDock {
                            mode: "toggle".into(),
                        });
                    }
                    Some("model_toggle") => {
                        let _ = term_proxy_ipc.send_event(crate::ipc::UserEvent::ModelAction {
                            kind: "model_toggle".into(), value: None,
                        });
                    }
                    Some("update_check") => {
                        let _ = term_proxy_ipc.send_event(crate::ipc::UserEvent::UpdateCheck);
                    }
                    Some("window_action") => {
                        if let Some(action) = msg.get("action").and_then(|v| v.as_str()) {
                            let _ = term_proxy_ipc.send_event(crate::ipc::UserEvent::WindowAction {
                                action: action.to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        })
        .build_as_child(&window)?;

    // --- divider ---
    let divider_proxy = event_loop.create_proxy();
    let divider_assets = term_dir.clone();
    let divider = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(TERMINAL_W, 0, DIVIDER_W, WINDOW_H))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            serve_abterm(&divider_assets, &request)
        })
        .with_url("http://abterm.localhost/divider.html")
        .with_ipc_handler(move |request| {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(request.body()) {
                if msg.get("type").and_then(|v| v.as_str()) == Some("split") {
                    let delta = msg.get("delta").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let _ = divider_proxy.send_event(crate::ipc::UserEvent::SplitDelta { delta });
                }
            }
        })
        .build_as_child(&window)?;

    // --- tabs (real WebViews, shared WebContext) ---
    let newtab_url = "http://abterm.localhost/newtab.html";
    let fixture = root.join("tests").join("fixture.html");
    let fixture_url = Url::from_file_path(&fixture)
        .map_err(|_| format!("invalid fixture path: {}", fixture.display()))?
        .to_string();
    let _fixture_url = fixture_url; // kept for local tests, never opened in production
    let tab_urls = vec![newtab_url.to_string(), newtab_url.to_string(),
                        newtab_url.to_string(), newtab_url.to_string()];
    let mut tabs: Vec<wry::WebView> = Vec::new();
    for (i, url) in tab_urls.iter().enumerate() {
        tabs.push(build_tab(
            &mut context, &window, url, i,
            event_loop.create_proxy(), event_loop.create_proxy(),
            &term_dir, &downloads, &logs,
        )?);
    }

    // --- toolbar ---
    let browser_proxy = event_loop.create_proxy();
    let toolbar_proxy = browser_proxy.clone();
    let toolbar_assets = term_dir.clone();
    let toolbar_url = match std::env::var("AGENT_BROWSER_TEST_NAV") {
        Ok(u) => format!("http://abterm.localhost/toolbar.html?nav={}", urlencoding(&u)),
        Err(_) => "http://abterm.localhost/toolbar.html".to_string(),
    };
    let toolbar = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(TERMINAL_W + DIVIDER_W, 0,
            WINDOW_W - TERMINAL_W - DIVIDER_W - TAB_W, TOOLBAR_H))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            serve_abterm(&toolbar_assets, &request)
        })
        .with_url(&toolbar_url)
        .with_ipc_handler(move |request| {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(request.body()) {
                let kind = msg.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let value = msg.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                let _ = toolbar_proxy.send_event(crate::ipc::UserEvent::BrowserAction {
                    kind, value, tab: None,
                });
            }
        })
        .build_as_child(&window)?;

    // --- tab strip ---
    let tabs_proxy = event_loop.create_proxy();
    let tabs_ready_proxy = event_loop.create_proxy();
    let tabs_assets = term_dir.clone();
    let tab_strip = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(WINDOW_W - TAB_W, 0, TAB_W, WINDOW_H))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            serve_abterm(&tabs_assets, &request)
        })
        .with_url("http://abterm.localhost/tabs.html")
        .with_on_page_load_handler(move |_event, _url| {
            let _ = tabs_ready_proxy.send_event(crate::ipc::UserEvent::TabsReady);
        })
        .with_ipc_handler(move |request| {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(request.body()) {
                let kind = msg.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let index = msg.get("index").and_then(|v| v.as_u64()).map(|v| v as usize);
                let _ = tabs_proxy.send_event(crate::ipc::UserEvent::TabAction { kind, index });
            }
        })
        .build_as_child(&window)?;

    // --- local model panel with its own draggable divider ---
    let model_proxy = event_loop.create_proxy();
    let model_ready_proxy = event_loop.create_proxy();
    let model_assets = term_dir.clone();
    let model_panel = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(0, WINDOW_H - 220, TERMINAL_W, 220))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            serve_abterm(&model_assets, &request)
        })
        .with_url("http://abterm.localhost/model.html")
        .with_on_page_load_handler(move |_event, _url| {
            let _ = model_ready_proxy.send_event(crate::ipc::UserEvent::ModelAction {
                kind: "model_ready".into(), value: None,
            });
        })
        .with_ipc_handler(move |request| {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(request.body()) {
                let kind = msg.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let value = msg.get("value").and_then(|v| v.as_str()).map(|s| s.to_string());
                let _ = model_proxy.send_event(crate::ipc::UserEvent::ModelAction { kind, value });
            }
        })
        .build_as_child(&window)?;

    let model_div_proxy = event_loop.create_proxy();
    let model_div_assets = term_dir.clone();
    let model_divider = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(0, WINDOW_H - 228, TERMINAL_W, DIVIDER_W))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            serve_abterm(&model_div_assets, &request)
        })
        .with_url("http://abterm.localhost/divider.html")
        .with_ipc_handler(move |request| {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(request.body()) {
                if msg.get("type").and_then(|v| v.as_str()) == Some("split") {
                    let delta = msg.get("delta").and_then(|v| v.as_i64()).unwrap_or(0);
                    let _ = model_div_proxy.send_event(crate::ipc::UserEvent::ModelAction {
                        kind: "model_split".into(), value: Some(delta.to_string()),
                    });
                }
            }
        })
        .build_as_child(&window)?;

    let local_model = Arc::new(crate::local_model::LocalModel::new(root.clone()));

    let mut scale = window.scale_factor();
    let mut origin = window.inner_position()?;
    let mut model_size = 0; // hidden until the user clicks "+ Model"
    let viewport = layout_bounds("left", TERMINAL_W, WINDOW_W, WINDOW_H,
        &terminal, &divider, &model_panel, &model_divider, model_size, &toolbar, &tabs, &tab_strip, origin, scale);
    let mut controller = crate::control::Controller::new(viewport.clone(), logs.join("actions.jsonl"))?;
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt, Err(e) => { eprintln!("IPC runtime error: {e}"); return; }
        };
        rt.block_on(crate::ipc::serve(proxy));
    });

    let mut active = 0usize;
    let mut titles = vec!["New Tab".to_string(), "New Tab".to_string(),
                          "New Tab".to_string(), "New Tab".to_string()];
    let mut urls = tab_urls.clone();
    let mut closed = vec![false; tabs.len()];
    let refresh_tabs = |tabs_w: &wry::WebView, titles: &Vec<String>, urls: &Vec<String>,
                        active: usize, closed: &Vec<bool>| {
        if let (Ok(t), Ok(u), Ok(c)) = (
            serde_json::to_string(titles),
            serde_json::to_string(urls),
            serde_json::to_string(closed),
        ) {
            let _ = tabs_w.evaluate_script(&format!("window.__setTabs({t},{u},{active},{c})"));
        }
    };
    for (i, tab) in tabs.iter().enumerate() { tab.set_visible(i == 0).ok(); }
    refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
    println!("GUI ready: root={}; {} tabs, page viewport {:?}", root.display(), tabs.len(), viewport);

    // Cloned inside the event loop when a brand-new tab WebView is needed.
    let tab_proxy_template = event_loop.create_proxy();
    let tab_ipc_template = event_loop.create_proxy();

    let mut term_ready = false;
    let mut pending_output: Vec<String> = Vec::new();
    let mut layout_cache = LayoutCache::default();
    let mut dock = "left".to_string();
    let mut term_size = TERMINAL_W;
    let mut win_w = WINDOW_W;
    let mut win_h = WINDOW_H;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            tao::event::Event::WindowEvent {
                event: tao::event::WindowEvent::CloseRequested, ..
            } => {
                session.terminate();
                *control_flow = ControlFlow::Exit;
                return;
            }
            tao::event::Event::WindowEvent { event: tao::event::WindowEvent::Resized(size), .. } => {
                // tao reports physical pixels; child WebView bounds are logical.
                win_w = (size.width as f64 / scale) as i32;
                win_h = (size.height as f64 / scale) as i32;
                layout_cache.dirty = true;
                controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                    &terminal, &divider, &model_panel, &model_divider, model_size, &toolbar, &tabs, &tab_strip, origin, scale));
                let _ = terminal.evaluate_script("window.__fit && window.__fit()");
            }
            tao::event::Event::WindowEvent { event: tao::event::WindowEvent::Moved(pos), .. } => {
                origin = pos;
                layout_cache.dirty = true;
                controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                    &terminal, &divider, &model_panel, &model_divider, model_size, &toolbar, &tabs, &tab_strip, origin, scale));
            }
            tao::event::Event::WindowEvent {
                event: tao::event::WindowEvent::ScaleFactorChanged { scale_factor, .. }, ..
            } => {
                scale = scale_factor;
                layout_cache.dirty = true;
                controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                    &terminal, &divider, &model_panel, &model_divider, model_size, &toolbar, &tabs, &tab_strip, origin, scale));
                let _ = terminal.evaluate_script("window.__fit && window.__fit()");
            }
            tao::event::Event::UserEvent(ev) => match ev {
                crate::ipc::UserEvent::TermOutput { data } => {
                    if !term_ready { pending_output.push(data); return; }
                    if let Ok(js) = serde_json::to_string(&data) {
                        let _ = terminal.evaluate_script(&format!("window.__termWrite({js})"));
                    }
                }
                crate::ipc::UserEvent::TermReady => {
                    term_ready = true;
                    for chunk in pending_output.drain(..) {
                        if let Ok(js) = serde_json::to_string(&chunk) {
                            let _ = terminal.evaluate_script(&format!("window.__termWrite({js})"));
                        }
                    }
                }
                crate::ipc::UserEvent::Ipc { mut cmd, reply } => {
                    if matches!(cmd, crate::protocol::Command::ListTabs) {
                        controller.log_command(&cmd, true);
                        let list: Vec<crate::protocol::TabInfo> = titles.iter()
                            .zip(urls.iter()).enumerate()
                            .filter(|(i, _)| !closed[*i])
                            .map(|(i, (t, u))| crate::protocol::TabInfo {
                                index: i, title: t.clone(), url: u.clone(), active: i == active,
                            }).collect();
                        let _ = reply.send(crate::protocol::Response::Tabs { tabs: list });
                        return;
                    }
                    if let crate::protocol::Command::CloseTab { index } = &cmd {
                        let open_count = closed.iter().filter(|c| !**c).count();
                        let valid = *index < tabs.len() && !closed[*index]
                            && (open_count > 1 || crate::policy::policy().allow_close_last_tab);
                        controller.log_command(&cmd, valid);
                        if !valid {
                            let _ = reply.send(crate::protocol::Response::Error {
                                message: "cannot close that tab (out of range, already closed, or last tab; set allow_close_last_tab to override)".into(),
                            });
                            return;
                        }
                        closed[*index] = true;
                        tabs[*index].set_visible(false).ok();
                        if closed.iter().all(|c| *c) {
                            closed[*index] = false;
                            tabs[*index].load_url(newtab_url).ok();
                            urls[*index] = newtab_url.to_string();
                            titles[*index] = "New Tab".into();
                        }
                        if active == *index || closed[active] {
                            if let Some(next) = closed.iter().position(|c| !c) {
                                active = activate_tab(next, &tabs, &toolbar, &urls);
                            }
                        }
                        layout_cache.dirty = true;
                        refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
                        let _ = reply.send(crate::protocol::Response::Ok {
                            message: format!("closed tab {}", *index),
                        });
                        return;
                    }
                    if let crate::protocol::Command::FocusTab { index } = &cmd {
                        controller.log_command(&cmd, *index < tabs.len() && !closed[*index]);
                        if *index < tabs.len() && !closed[*index] {
                            active = activate_tab(*index, &tabs, &toolbar, &urls);
                            layout_cache.dirty = true;
                            refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
                            let _ = reply.send(crate::protocol::Response::Ok {
                                message: format!("focused tab {active}"),
                            });
                        } else {
                            let _ = reply.send(crate::protocol::Response::Error {
                                message: "tab index out of range".into(),
                            });
                        }
                        return;
                    }
                    if let crate::protocol::Command::Navigate { url } = &cmd {
                        let allowed = crate::policy::policy().allow_url_navigation;
                        let target_url = if allowed { navigation_url(url) } else { None };
                        let valid = target_url.is_some();
                        controller.log_command(&cmd, valid);
                        if let Some(u) = target_url {
                            let _ = tabs[active].load_url(&u);
                            layout_cache.dirty = true;
                            let _ = toolbar.evaluate_script("window.__setStatus('')");
                            let _ = reply.send(crate::protocol::Response::Ok {
                                message: format!("navigating active tab to {u}"),
                            });
                        } else {
                            let _ = reply.send(crate::protocol::Response::Error {
                                message: if allowed {
                                    "URL could not be parsed".into()
                                } else {
                                    "URL navigation is disabled in agent-policy.json".into()
                                },
                            });
                        }
                        return;
                    }
                    if let crate::protocol::Command::NewTab { url } = &cmd {
                        let policy = crate::policy::policy();
                        controller.log_command(&cmd, policy.allow_new_tabs);
                        if !policy.allow_new_tabs {
                            let _ = reply.send(crate::protocol::Response::Error {
                                message: "new tabs are disabled in agent-policy.json".into(),
                            });
                            return;
                        }
                        let target = match url.as_deref() {
                            Some(raw) => match navigation_url(raw) {
                                Some(u) => u,
                                None => {
                                    let _ = reply.send(crate::protocol::Response::Error {
                                        message: "URL could not be parsed".into(),
                                    });
                                    return;
                                }
                            },
                            None => newtab_url.to_string(),
                        };
                        let label = if target == newtab_url {
                            "New Tab".to_string()
                        } else {
                            target.clone()
                        };
                        let (next, reused) = if let Some(i) = closed.iter().position(|c| *c) {
                            (i, true)
                        } else {
                            // No free slot: create a real WebView instead of
                            // capping the number of tabs.
                            match build_tab(
                                &mut context, &window, &target, tabs.len(),
                                tab_proxy_template.clone(), tab_ipc_template.clone(),
                                &term_dir, &downloads, &logs,
                            ) {
                                Ok(tab) => {
                                    let i = tabs.len();
                                    tabs.push(tab);
                                    titles.push(label.clone());
                                    urls.push(target.clone());
                                    closed.push(false);
                                    (i, false)
                                }
                                Err(e) => {
                                    let _ = reply.send(crate::protocol::Response::Error {
                                        message: format!("could not create tab: {e}"),
                                    });
                                    return;
                                }
                            }
                        };
                        if reused {
                            closed[next] = false;
                            let _ = tabs[next].load_url(&target);
                            titles[next] = label;
                            urls[next] = target.clone();
                        }
                        controller.set_viewport(layout_bounds(
                            &dock, term_size, win_w, win_h,
                            &terminal, &divider, &model_panel, &model_divider, model_size, &toolbar, &tabs, &tab_strip, origin, scale,
                        ));
                        active = activate_tab(next, &tabs, &toolbar, &urls);
                        layout_cache.dirty = true;
                        refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
                        let _ = reply.send(crate::protocol::Response::Ok {
                            message: format!("opened tab {next} at {target}"),
                        });
                        return;
                    }
                    if matches!(cmd, crate::protocol::Command::Back) {
                        controller.log_command(&cmd, true);
                        let _ = tabs[active].go_back();
                        layout_cache.dirty = true;
                        let _ = reply.send(crate::protocol::Response::Ok {
                            message: "went back".into(),
                        });
                        return;
                    }
                    if matches!(cmd, crate::protocol::Command::Forward) {
                        controller.log_command(&cmd, true);
                        let _ = tabs[active].go_forward();
                        layout_cache.dirty = true;
                        let _ = reply.send(crate::protocol::Response::Ok {
                            message: "went forward".into(),
                        });
                        return;
                    }
                    if matches!(cmd, crate::protocol::Command::Reload) {
                        controller.log_command(&cmd, true);
                        let _ = tabs[active].reload();
                        layout_cache.dirty = true;
                        let _ = reply.send(crate::protocol::Response::Ok {
                            message: "reloaded".into(),
                        });
                        return;
                    }
                    if let crate::protocol::Command::GetPolicy { reload } = &cmd {
                        controller.log_command(&cmd, true);
                        let current = if *reload {
                            crate::policy::reload_policy()
                        } else {
                            crate::policy::policy()
                        };
                        let _ = reply.send(crate::protocol::Response::Text {
                            text: current.as_json(),
                        });
                        return;
                    }
                    if matches!(cmd, crate::protocol::Command::ModelStatus) {
                        controller.log_command(&cmd, true);
                        let _ = reply.send(crate::protocol::Response::Text {
                            text: local_model.status(),
                        });
                        return;
                    }
                    if let crate::protocol::Command::ModelLoad { path } = &cmd {
                        controller.log_command(&cmd, true);
                        let model = local_model.clone();
                        let target = path.clone();
                        let proxy = ui_proxy.clone();
                        std::thread::spawn(move || {
                            let result = model.load(&target);
                            let _ = proxy.send_event(crate::ipc::UserEvent::ModelLoaded {
                                result, reply: Some(reply),
                            });
                        });
                        return;
                    }
                    if let crate::protocol::Command::ModelUse { provider } = &cmd {
                        let result = local_model.use_provider(provider);
                        controller.log_command(&cmd, result.is_ok());
                        push_model_status(&model_panel, &local_model);
                        let _ = reply.send(match result {
                            Ok(msg) => crate::protocol::Response::Ok { message: msg },
                            Err(msg) => crate::protocol::Response::Error { message: msg },
                        });
                        return;
                    }
                    if matches!(cmd, crate::protocol::Command::ModelStop) {
                        let msg = local_model.stop();
                        controller.log_command(&cmd, true);
                        push_model_status(&model_panel, &local_model);
                        let _ = reply.send(crate::protocol::Response::Ok { message: msg });
                        return;
                    }
                    if let crate::protocol::Command::Dock { mode } = &cmd {
                        let valid = matches!(mode.as_str(), "left" | "bottom" | "toggle");
                        controller.log_command(&cmd, valid);
                        if !valid {
                            let _ = reply.send(crate::protocol::Response::Error {
                                message: "dock must be left, bottom, or toggle".into(),
                            });
                            return;
                        }
                        let _ = ui_proxy.send_event(crate::ipc::UserEvent::SetDock { mode: mode.clone() });
                        let _ = reply.send(crate::protocol::Response::Ok { message: "dock requested".into() });
                        return;
                    }
                    // Virtual cursor follows every agent pointer action.
                    if let Some((x, y)) = crate::protocol::action_point(&cmd) {
                        let v = controller.viewport();
                        let fx = (x - v.x) as f64 / v.width.max(1) as f64;
                        let fy = (y - v.y) as f64 / v.height.max(1) as f64;
                        let _ = tabs[active].evaluate_script(&agent_cursor_js(fx, fy));
                    }
                    if matches!(cmd, crate::protocol::Command::Scroll { .. }) {
                        layout_cache.dirty = true;
                    }
                    if let crate::protocol::Command::ScrollDom { dx, dy } = &cmd {
                        let js = format!("window.scrollBy({dx},{dy});");
                        let ok = tabs[active].evaluate_script(&js).is_ok();
                        layout_cache.dirty = true;
                        controller.log_command(&cmd, ok);
                        let _ = reply.send(if ok {
                            crate::protocol::Response::Ok { message: "scrolled DOM".into() }
                        } else {
                            crate::protocol::Response::Error { message: "DOM scroll failed".into() }
                        });
                        return;
                    }
                    if let crate::protocol::Command::Layout { refresh } = &cmd {
                        let valid = !refresh && !layout_cache.dirty
                            && layout_cache.tab == active
                            && layout_cache.url == urls[active];
                        if valid {
                            controller.log_command(&cmd, true);
                            let _ = reply.send(crate::protocol::Response::Text {
                                text: layout_json(&layout_cache),
                            });
                            return;
                        }
                        let target_tab = active;
                        let slot = Arc::new(Mutex::new(Some(("layout".to_string(), reply, target_tab))));
                        let slot_cb = slot.clone();
                        let proxy_cb = ui_proxy.clone();
                        if let Err(e) = tabs[target_tab].evaluate_script_with_callback(
                            &page_elements_js(), move |text| {
                                if let Some((kind, reply, tab)) = slot_cb.lock().unwrap().take() {
                                    let _ = proxy_cb.send_event(crate::ipc::UserEvent::PageQueryResult {
                                        kind, text, reply, tab,
                                    });
                                }
                            },
                        ) {
                            if let Some((_, reply, _)) = slot.lock().unwrap().take() {
                                let _ = reply.send(crate::protocol::Response::Error {
                                    message: format!("layout scan failed: {e}"),
                                });
                            }
                        }
                        return;
                    }

                    let mut pending_type: Option<String> = None;
                    let mut resolved_id: Option<String> = None;
                    if let crate::protocol::Command::ClickId { id } = &cmd {
                        resolved_id = Some(id.clone());
                    }
                    if let crate::protocol::Command::TypeId { id, text } = &cmd {
                        resolved_id = Some(id.clone());
                        pending_type = Some(text.clone());
                    }
                    if let Some(id) = resolved_id {
                        if layout_cache.dirty
                            || layout_cache.tab != active
                            || layout_cache.url != urls[active]
                        {
                            let _ = reply.send(crate::protocol::Response::Error {
                                message: "layout is stale; call browser_layout {refresh:true}".into(),
                            });
                            return;
                        }
                        let found = layout_cache.entries.iter().find(|e| e.id == id).cloned();
                        match found {
                            Some(el) => {
                                cmd = crate::protocol::Command::Click {
                                    x: el.sx,
                                    y: el.sy,
                                    button: crate::protocol::MouseButton::Left,
                                };
                            }
                            None => {
                                let _ = reply.send(crate::protocol::Response::Error {
                                    message: format!("unknown layout id: {id}; call browser_layout"),
                                });
                                return;
                            }
                        }
                    }

                    let query = match &cmd {
                        crate::protocol::Command::PageText => Some(("page_text", page_text_js())),
                        crate::protocol::Command::PageElements => Some(("page_elements", page_elements_js())),
                        crate::protocol::Command::ScrollIntoView { text } => {
                            Some(("scroll_into_view", scroll_into_view_js(text)))
                        }
                        _ => None,
                    };
                    if let Some((kind, js)) = query {
                        if kind == "scroll_into_view" { layout_cache.dirty = true; }
                        let target_tab = active;
                        controller.log_command(&cmd, true);
                        let slot = Arc::new(Mutex::new(Some((kind.to_string(), reply, target_tab))));
                        let slot_cb = slot.clone();
                        let proxy_cb = ui_proxy.clone();
                        if let Err(e) = tabs[target_tab].evaluate_script_with_callback(&js, move |text| {
                            if let Some((kind, reply, tab)) = slot_cb.lock().unwrap().take() {
                                let _ = proxy_cb.send_event(crate::ipc::UserEvent::PageQueryResult {
                                    kind, text, reply, tab,
                                });
                            }
                        }) {
                            if let Some((_, reply, _)) = slot.lock().unwrap().take() {
                                let _ = reply.send(crate::protocol::Response::Error {
                                    message: format!("page query failed: {e}"),
                                });
                            }
                        }
                        return;
                    }
                    // A deliberate click outside the page has no page element
                    // to inspect, so the fail-closed password check would
                    // refuse it. Skip that check for outside clicks; password
                    // fields inside the page are still protected.
                    let outside_click = matches!(cmd, crate::protocol::Command::Click { x, y, .. }
                        if crate::policy::policy().allow_outside_viewport
                            && !controller.viewport().contains(x, y));
                    let key_writes = matches!(&cmd,
                        crate::protocol::Command::Key { key }
                            if crate::control::key_produces_text(key));
                    let needs_check = (matches!(cmd,
                        crate::protocol::Command::Click { .. }
                            | crate::protocol::Command::Type { .. })
                        || key_writes)
                        && !outside_click;
                    if needs_check {
                        let relative = match &cmd {
                            crate::protocol::Command::Click { x, y, .. } => {
                                let v = controller.viewport();
                                Some((x - v.x, y - v.y))
                            }
                            _ => None,
                        };
                        let js = crate::control::Controller::password_check_js(relative);
                        let target_tab = active;
                        let slot = Arc::new(Mutex::new(Some((cmd, reply))));
                        let slot_cb = slot.clone();
                        let proxy_cb = ui_proxy.clone();
                        if let Err(e) = tabs[target_tab].evaluate_script_with_callback(&js, move |value| {
                            if let Some((cmd, reply)) = slot_cb.lock().unwrap().take() {
                                let _ = proxy_cb.send_event(crate::ipc::UserEvent::PasswordChecked {
                                    cmd, reply, is_password: value.contains("true"), tab: target_tab,
                                    then_type: pending_type.clone(),
                                });
                            }
                        }) {
                            if let Some((_, reply)) = slot.lock().unwrap().take() {
                                let _ = reply.send(crate::protocol::Response::Error {
                                    message: format!("password check failed: {e}"),
                                });
                            }
                        }
                    } else {
                        let response = controller.execute(&cmd, &tabs[active], false);
                        let _ = reply.send(response);
                    }
                }
                crate::ipc::UserEvent::PasswordChecked { cmd, reply, is_password, tab, then_type } => {
                    if tab != active {
                        let _ = reply.send(crate::protocol::Response::Error {
                            message: "tab changed during safety check; action cancelled".into(),
                        });
                        return;
                    }
                    let response = controller.execute(&cmd, &tabs[tab], is_password);
                    let failed = matches!(response, crate::protocol::Response::Error { .. });
                    if !failed {
                        if let Some(text) = then_type {
                            // The click may have just focused a password field.
                            // Re-check the active element before typing instead
                            // of reusing the pre-click classification.
                            let slot = Arc::new(Mutex::new(Some((
                                crate::protocol::Command::Type { text }, reply,
                            ))));
                            let slot_cb = slot.clone();
                            let proxy_cb = ui_proxy.clone();
                            let js = crate::control::Controller::password_check_js(None);
                            if let Err(e) = tabs[tab].evaluate_script_with_callback(&js, move |value| {
                                if let Some((cmd, reply)) = slot_cb.lock().unwrap().take() {
                                    let _ = proxy_cb.send_event(crate::ipc::UserEvent::PasswordChecked {
                                        cmd, reply, is_password: value.contains("true"),
                                        tab, then_type: None,
                                    });
                                }
                            }) {
                                if let Some((_, reply)) = slot.lock().unwrap().take() {
                                    let _ = reply.send(crate::protocol::Response::Error {
                                        message: format!("password check failed: {e}"),
                                    });
                                }
                            }
                            return;
                        }
                    }
                    let _ = reply.send(response);
                }
                crate::ipc::UserEvent::BrowserAction { kind, value, tab } => {
                    let target = tab.unwrap_or(active).min(tabs.len() - 1);
                    match kind.as_str() {
                        "navigate" => {
                            let raw = value.unwrap_or_default();
                            if let Some(u) = navigation_url(&raw) {
                                let _ = tabs[target].load_url(&u);
                                let _ = toolbar.evaluate_script("window.__setStatus('')");
                            } else {
                                let _ = toolbar.evaluate_script("window.__setStatus('Invalid URL')");
                            }
                        }
                        "back" => { let _ = tabs[target].go_back(); }
                        "forward" => { let _ = tabs[target].go_forward(); }
                        "reload" => { let _ = tabs[target].reload(); }
                        "home" => { let _ = tabs[target].load_url(newtab_url); }
                        _ => {}
                    }
                }
                crate::ipc::UserEvent::PageLoaded { url, tab } => {
                    if tab == active { layout_cache.dirty = true; }
                    if tab < urls.len() {
                        urls[tab] = url.clone();
                        titles[tab] = if url == newtab_url {
                            "New Tab".into()
                        } else {
                            Url::parse(&url).ok().and_then(|u| u.host_str().map(|h| h.to_string()))
                                .unwrap_or_else(|| url.clone())
                        };
                        refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
                        if tab == active {
                            if let Ok(js) = serde_json::to_string(&url) {
                                let _ = toolbar.evaluate_script(&format!("window.__setUrl({js})"));
                            }
                        }
                    }
                }
                crate::ipc::UserEvent::TabAction { kind, index } => match kind.as_str() {
                    "focus" => {
                        if let Some(i) = index.filter(|i| *i < tabs.len() && !closed[*i]) {
                            active = activate_tab(i, &tabs, &toolbar, &urls);
                            layout_cache.dirty = true;
                            refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
                        }
                    }
                    "close" => {
                        if let Some(i) = index.filter(|i| *i < tabs.len() && !closed[*i]) {
                            closed[i] = true;
                            tabs[i].set_visible(false).ok();
                            if closed.iter().all(|c| *c) {
                                // Never leave a blank page with a hidden active tab.
                                closed[i] = false;
                                tabs[i].load_url(newtab_url).ok();
                                urls[i] = newtab_url.to_string();
                                titles[i] = "New Tab".into();
                            }
                            if active == i || closed[active] {
                                if let Some(next) = closed.iter().position(|c| !c) {
                                    active = activate_tab(next, &tabs, &toolbar, &urls);
                                }
                            }
                            layout_cache.dirty = true;
                            refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
                        }
                    }
                    "reopen" => {
                        if let Some(i) = index.filter(|i| *i < tabs.len() && closed[*i]) {
                            closed[i] = false;
                            let _ = tabs[i].load_url(newtab_url);
                            urls[i] = newtab_url.to_string();
                            titles[i] = "New Tab".into();
                            active = activate_tab(i, &tabs, &toolbar, &urls);
                            layout_cache.dirty = true;
                            refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
                        }
                    }
                    "newtab" => {
                        let next = if let Some(i) = closed.iter().position(|c| *c) {
                            tabs[i].load_url(newtab_url).ok();
                            urls[i] = newtab_url.to_string();
                            titles[i] = "New Tab".into();
                            closed[i] = false;
                            i
                        } else {
                            // Never overwrite an already-open tab.
                            match build_tab(
                                &mut context, &window, newtab_url, tabs.len(),
                                tab_proxy_template.clone(), tab_ipc_template.clone(),
                                &term_dir, &downloads, &logs,
                            ) {
                                Ok(tab) => {
                                    let i = tabs.len();
                                    let _ = tab.set_visible(true);
                                    tabs.push(tab);
                                    titles.push("New Tab".into());
                                    urls.push(newtab_url.to_string());
                                    closed.push(false);
                                    i
                                }
                                Err(e) => {
                                    eprintln!("agent-browser: could not create tab: {e}");
                                    return;
                                }
                            }
                        };
                        controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                            &terminal, &divider, &model_panel, &model_divider, model_size,
                            &toolbar, &tabs, &tab_strip, origin, scale));
                        active = activate_tab(next, &tabs, &toolbar, &urls);
                        layout_cache.dirty = true;
                        refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
                    }
                    _ => {}
                },
                crate::ipc::UserEvent::PageQueryResult { kind, text, reply, tab } => {
                    if tab != active {
                        let _ = reply.send(crate::protocol::Response::Error {
                            message: "tab changed during page query".into(),
                        });
                        return;
                    }
                    let is_scan = kind == "page_elements" || kind == "layout";
                    let converted = if is_scan {
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(raw) => {
                                let mut v = match raw {
                                    serde_json::Value::String(s) => {
                                        serde_json::from_str::<serde_json::Value>(&s)
                                            .unwrap_or(serde_json::Value::Null)
                                    }
                                    other => other,
                                };
                                let inner_w = v.get("innerWidth").and_then(|x| x.as_f64()).unwrap_or(1.0);
                                let inner_h = v.get("innerHeight").and_then(|x| x.as_f64()).unwrap_or(1.0);
                                let vv = controller.viewport();
                                let rx = vv.width as f64 / inner_w.max(1.0);
                                let ry = vv.height as f64 / inner_h.max(1.0);
                                if let Some(elements) = v.get_mut("elements").and_then(|e| e.as_array_mut()) {
                                    for el in elements.iter_mut() {
                                        if let (Some(x), Some(y)) = (
                                            el.get("x").and_then(|n| n.as_f64()),
                                            el.get("y").and_then(|n| n.as_f64()),
                                        ) {
                                            el["sx"] = serde_json::json!((vv.x as f64 + x * rx) as i32);
                                            el["sy"] = serde_json::json!((vv.y as f64 + y * ry) as i32);
                                        }
                                    }
                                }
                                v
                            }
                            Err(_) => {
                                let _ = reply.send(crate::protocol::Response::Error {
                                    message: "invalid layout scan result".into(),
                                });
                                return;
                            }
                        }
                    } else {
                        serde_json::Value::String(text)
                    };
                    // WebView2 serializes the JS string result; unwrap once.
                    let converted = match converted {
                        serde_json::Value::String(s) => {
                            serde_json::from_str::<serde_json::Value>(&s)
                                .unwrap_or(serde_json::Value::Null)
                        }
                        other => other,
                    };

                    if kind == "layout" {
                        let mut entries = Vec::new();
                        if let Some(elements) = converted.get("elements").and_then(|e| e.as_array()) {
                            for (n, el) in elements.iter().enumerate() {
                                entries.push(LayoutEntry {
                                    id: format!("e{}", n + 1),
                                    tag: el.get("tag").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    role: el.get("role").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    label: el.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    sx: el.get("sx").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                                    sy: el.get("sy").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                                    w: el.get("w").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                                    h: el.get("h").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                                });
                            }
                        }
                        layout_cache = LayoutCache {
                            tab,
                            url: urls[tab].clone(),
                            entries,
                            dirty: false,
                        };
                        let _ = reply.send(crate::protocol::Response::Text {
                            text: layout_json(&layout_cache),
                        });
                        return;
                    }

                    // page_text and scroll_into_view both return a JSON object
                    // string from JS; after unwrapping once `converted` is that
                    // object. Indexing [0] used to yield an empty string.
                    let output = converted.to_string();
                    let _ = reply.send(crate::protocol::Response::Text { text: output });
                }
                crate::ipc::UserEvent::ModelAction { kind, value } => match kind.as_str() {
                    "model_ready" => {
                        push_model_status(&model_panel, &local_model);
                    }
                    "model_toggle" => {
                        model_size = if model_size > 0 { 0 } else { 220 };
                        let _ = terminal.evaluate_script(&format!(
                            "window.__setModelVisible && window.__setModelVisible({})",
                            model_size > 0
                        ));
                        controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                            &terminal, &divider, &model_panel, &model_divider, model_size,
                            &toolbar, &tabs, &tab_strip, origin, scale));
                        push_model_status(&model_panel, &local_model);
                    }
                    "model_split" => {
                        if let Some(delta) = value.and_then(|v| v.parse::<i32>().ok()) {
                            model_size = (model_size - delta).clamp(120, 600);
                            controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                                &terminal, &divider, &model_panel, &model_divider, model_size,
                                &toolbar, &tabs, &tab_strip, origin, scale));
                        }
                    }
                    "model_load" => {
                        if let Some(path) = value {
                            let model = local_model.clone();
                            let proxy = ui_proxy.clone();
                            std::thread::spawn(move || {
                                let result = model.load(&path);
                                let _ = proxy.send_event(crate::ipc::UserEvent::ModelLoaded {
                                    result, reply: None,
                                });
                            });
                        }
                    }
                    "model_use" => {
                        if let Some(provider) = value {
                            let _ = local_model.use_provider(&provider);
                            push_model_status(&model_panel, &local_model);
                        }
                    }
                    "model_stop" => {
                        let _ = local_model.stop();
                        push_model_status(&model_panel, &local_model);
                    }
                    _ => {}
                },
                crate::ipc::UserEvent::ModelLoaded { result, reply } => {
                    let ok = result.is_ok();
                    push_model_status(&model_panel, &local_model);
                    if let Some(reply) = reply {
                        let _ = reply.send(match result {
                            Ok(msg) => crate::protocol::Response::Ok { message: msg },
                            Err(msg) => crate::protocol::Response::Error { message: msg },
                        });
                    } else if !ok {
                        eprintln!("agent-browser: UI model load failed");
                    }
                }
                crate::ipc::UserEvent::WindowAction { action } => {
                    if action == "close" {
                        session.terminate();
                        *control_flow = ControlFlow::Exit;
                    } else {
                        window_command(&action);
                    }
                }
                crate::ipc::UserEvent::LayoutDirty => { layout_cache.dirty = true; }
                crate::ipc::UserEvent::UpdateCheck => {
                    let p = ui_proxy.clone();
                    let _ = terminal.evaluate_script("window.__setUpdateStatus('Checkingâ€¦')");
                    std::thread::spawn(move || {
                        let result = crate::updater::check_latest();
                        let _ = p.send_event(crate::ipc::UserEvent::UpdateChecked { result });
                    });
                }
                crate::ipc::UserEvent::UpdateChecked { result } => {
                    use windows_sys::Win32::UI::WindowsAndMessaging::{
                        MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK,
                        MB_SETFOREGROUND, MB_TOPMOST, MB_YESNOCANCEL,
                    };
                    match result {
                        Err(msg) => {
                            let short: String = msg.chars().take(70).collect();
                            if let Ok(js) = serde_json::to_string(&format!("Check failed: {short}")) {
                                let _ = terminal.evaluate_script(
                                    &format!("window.__setUpdateStatus({js})"));
                            }
                            message_box(
                                &format!("Could not check for updates.\n\n{msg}"),
                                "Agent Browser",
                                MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
                            );
                        }
                        Ok(None) => {
                            let _ = terminal.evaluate_script("window.__setUpdateStatus('Up to date')");
                            message_box(
                                "You are running the latest version.",
                                "Agent Browser",
                                MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
                            );
                        }
                        Ok(Some(info)) => {
                            if let Ok(js) = serde_json::to_string(&format!("Update {} available", info.version)) {
                                let _ = terminal.evaluate_script(&format!("window.__setUpdateStatus({js})"));
                            }
                            if crate::updater::is_skipped(&root, &info.tag) {
                                message_box(
                                    &format!("Update {} is available, but it was skipped.", info.version),
                                    "Agent Browser",
                                    MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
                                );
                                return;
                            }
                            let notes: String = info.notes.chars().take(600).collect();
                            let answer = message_box(
                                &format!(
                                    "Agent Browser {} is available.\n\n{}\n\nYes = update now\nNo = remind me later\nCancel = skip this version",
                                    info.version, notes
                                ),
                                "Agent Browser update",
                                MB_YESNOCANCEL | MB_ICONQUESTION | MB_SETFOREGROUND | MB_TOPMOST,
                            );
                            if answer == 2 {
                                crate::updater::skip(&root, &info.tag);
                            } else if answer == 6 {
                                let p = ui_proxy.clone();
                                let app_root = root.clone();
                                let release = info.clone();
                                let _ = terminal.evaluate_script(
                                    "window.__setUpdateStatus('Downloading updateâ€¦')");
                                // Do NOT exit here: the updater script owns shutdown
                                // after the download completes. Exiting now kills
                                // this detached thread before it can install.
                                std::thread::spawn(move || {
                                    let installer = std::env::temp_dir().join(format!(
                                        "agent-browser-{}-{}",
                                        release.version, release.asset_name
                                    ));
                                    if let Err(e) = crate::updater::download_verified(
                                        &release, &installer,
                                    ) {
                                        let _ = p.send_event(crate::ipc::UserEvent::UpdateFailed {
                                            message: e,
                                        });
                                        return;
                                    }
                                    let script = app_root.join("apply-update.ps1");
                                    let launcher = app_root.join("start-agent-browser.ps1");
                                    let result = std::process::Command::new("powershell.exe")
                                        .args([
                                            "-NoProfile", "-WindowStyle", "Hidden",
                                            "-ExecutionPolicy", "Bypass", "-File",
                                        ])
                                        .arg(&script)
                                        .arg(&installer)
                                        .arg(&app_root)
                                        .arg(&launcher)
                                        .spawn();
                                    if let Err(e) = result {
                                        let _ = p.send_event(crate::ipc::UserEvent::UpdateFailed {
                                            message: format!("could not start updater: {e}"),
                                        });
                                    }
                                });
                            }
                        }
                    }
                }
                crate::ipc::UserEvent::UpdateFailed { message } => {
                    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK};
                    message_box(
                        &format!("Update failed.\n\n{message}"),
                        "Agent Browser",
                        MB_OK | MB_ICONERROR,
                    );
                }
                crate::ipc::UserEvent::TabsReady => {
                    refresh_tabs(&tab_strip, &titles, &urls, active, &closed);
                }
                crate::ipc::UserEvent::SetDock { mode } => {
                    dock = if mode == "toggle" {
                        if dock == "left" { "bottom".into() } else { "left".into() }
                    } else { mode };
                    term_size = if dock == "left" { TERMINAL_W } else { win_h / 2 };
                    layout_cache.dirty = true;
                    let _ = terminal.evaluate_script(&format!("window.__setDock('{dock}')"));
                    let _ = divider.evaluate_script(&format!("window.__setDock('{dock}')"));
                    controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                        &terminal, &divider, &model_panel, &model_divider, model_size, &toolbar, &tabs, &tab_strip, origin, scale));
                    let _ = terminal.evaluate_script("window.__fit && window.__fit()");
                }
                crate::ipc::UserEvent::SplitDelta { delta } => {
                    let max_size = if dock == "left" { win_w - TAB_W - 360 } else { win_h - 300 };
                    term_size = (term_size + delta).clamp(MIN_TERM, max_size);
                    layout_cache.dirty = true;
                    controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                        &terminal, &divider, &model_panel, &model_divider, model_size, &toolbar, &tabs, &tab_strip, origin, scale));
                    let _ = terminal.evaluate_script("window.__fit && window.__fit()");
                }
            },
            _ => {}
        }
        let _ = (&terminal, &divider, &model_panel, &model_divider, &toolbar, &tabs, &tab_strip, &session, &context);
    });
}
