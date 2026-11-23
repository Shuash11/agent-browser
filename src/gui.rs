//! GUI shell: embedded OpenCode terminal, browser toolbar, multi-tab page area,
//! right-edge tab strip, draggable splitter, and a virtual agent cursor.

use std::fs;
use std::path::PathBuf;
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
window.__abHideTimer=setTimeout(function(){{if(s.parentNode)s.parentNode.removeChild(s);}},900);
}})()"#
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
        terminal.set_bounds(rect(0, 0, ts, h)).ok();
        divider.set_bounds(rect(ts, 0, DIVIDER_W, h)).ok();
        toolbar.set_bounds(rect(area_x, 0, area_w, TOOLBAR_H)).ok();
        for tab in tabs { tab.set_bounds(rect(area_x, TOOLBAR_H, area_w, h - TOOLBAR_H)).ok(); }
        crate::protocol::Viewport {
            x: (origin.x as f64 * scale) as i32 + (area_x as f64 * scale) as i32,
            y: (origin.y as f64 * scale) as i32 + (TOOLBAR_H as f64 * scale) as i32,
            width: (area_w as f64 * scale) as i32,
            height: ((h - TOOLBAR_H) as f64 * scale) as i32,
        }
    } else {
        let max_term = (h * 40 / 100).max(180).min(h - 300);
        let ts = term_size.min(max_term).max(180);
        let area_h = h - ts - DIVIDER_W - TOOLBAR_H;
        terminal.set_bounds(rect(0, h - ts, w - TAB_W, ts)).ok();
        divider.set_bounds(rect(0, h - ts - DIVIDER_W, w - TAB_W, DIVIDER_W)).ok();
        toolbar.set_bounds(rect(0, 0, w - TAB_W, TOOLBAR_H)).ok();
        for tab in tabs { tab.set_bounds(rect(0, TOOLBAR_H, w - TAB_W, area_h)).ok(); }
        crate::protocol::Viewport {
            x: (origin.x as f64 * scale) as i32,
            y: (origin.y as f64 * scale) as i32 + (TOOLBAR_H as f64 * scale) as i32,
            width: ((w - TAB_W) as f64 * scale) as i32,
            height: (area_h as f64 * scale) as i32,
        }
    }
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
        .with_title("Agent Browser — embedded OpenCode")
        .with_inner_size(TaoLogicalSize::new(WINDOW_W, WINDOW_H))
        .with_min_inner_size(TaoLogicalSize::new(MIN_WINDOW_W, MIN_WINDOW_H))
        .with_resizable(true)
        .build(&event_loop)?;

    let npm = std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("npm").join("opencode.cmd"))
        .unwrap_or_else(|_| PathBuf::from("cmd.exe"));
    let session = crate::terminal::TerminalSession::spawn(&npm.to_string_lossy(), &root, term_proxy)?;
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
            let (file, mime) = match request.uri().path() {
                "/xterm.min.js" => ("xterm.min.js", "application/javascript"),
                "/xterm.min.css" => ("xterm.min.css", "text/css"),
                "/addon-fit.min.js" => ("addon-fit.min.js", "application/javascript"),
                "/divider.html" => ("divider.html", "text/html"),
                "/toolbar.html" => ("toolbar.html", "text/html"),
                "/newtab.html" => ("newtab.html", "text/html"),
                "/tabs.html" => ("tabs.html", "text/html"),
                "/theme.css" => ("theme.css", "text/css"),
                _ => ("index.html", "text/html"),
            };
            let bytes = std::fs::read(term_assets.join(file)).unwrap_or_default();
            wry::http::Response::builder()
                .header("content-type", mime)
                .header("cache-control", "no-store")
                .body(std::borrow::Cow::Owned(bytes)).unwrap()
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
            let is_css = request.uri().path() == "/theme.css";
            let file = if is_css { "theme.css" } else { "divider.html" };
            let bytes = std::fs::read(divider_assets.join(file)).unwrap_or_default();
            wry::http::Response::builder()
                .header("content-type", if is_css { "text/css" } else { "text/html" })
                .header("cache-control", "no-store")
                .body(std::borrow::Cow::Owned(bytes)).unwrap()
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
    let tab_urls = vec![newtab_url.to_string(), fixture_url,
                        newtab_url.to_string(), newtab_url.to_string()];
    let mut tabs: Vec<wry::WebView> = Vec::new();
    for (i, url) in tab_urls.iter().enumerate() {
        let browser_proxy_tab = event_loop.create_proxy();
        let tab_ipc_proxy = event_loop.create_proxy();
        let tab_assets = term_dir.clone();
        let dl_dir = downloads.clone();
        let lg_dir = logs.clone();
        let b = WebViewBuilder::new_with_web_context(&mut context)
            .with_bounds(rect(TERMINAL_W + DIVIDER_W, TOOLBAR_H,
                WINDOW_W - TERMINAL_W - DIVIDER_W - TAB_W, WINDOW_H - TOOLBAR_H))
            .with_custom_protocol("abterm".into(), move |_id, request| {
                let file = if request.uri().path() == "/theme.css" { "theme.css" } else { "newtab.html" };
                let bytes = std::fs::read(tab_assets.join(file)).unwrap_or_default();
                wry::http::Response::builder()
                    .header("content-type", if file == "theme.css" { "text/css" } else { "text/html" })
                    .header("cache-control", "no-store")
                    .body(std::borrow::Cow::Owned(bytes)).unwrap()
            })
            .with_url(url)
            .with_on_page_load_handler(move |_event, loaded| {
                let _ = browser_proxy_tab.send_event(crate::ipc::UserEvent::PageLoaded {
                    url: loaded, tab: i,
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
                    let value = msg.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let _ = tab_ipc_proxy.send_event(crate::ipc::UserEvent::BrowserAction {
                        kind, value, tab: Some(i),
                    });
                }
            });
        tabs.push(b.build_as_child(&window)?);
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
            let file = if request.uri().path() == "/theme.css" { "theme.css" } else { "toolbar.html" };
            let bytes = std::fs::read(toolbar_assets.join(file)).unwrap_or_default();
            wry::http::Response::builder()
                .header("content-type", if file == "theme.css" { "text/css" } else { "text/html" })
                .header("cache-control", "no-store")
                .body(std::borrow::Cow::Owned(bytes)).unwrap()
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
            let is_css = request.uri().path() == "/theme.css";
            let file = if is_css { "theme.css" } else { "tabs.html" };
            let bytes = std::fs::read(tabs_assets.join(file)).unwrap_or_default();
            wry::http::Response::builder()
                .header("content-type", if is_css { "text/css" } else { "text/html" })
                .header("cache-control", "no-store")
                .body(std::borrow::Cow::Owned(bytes)).unwrap()
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

    let scale = window.scale_factor();
    let origin = window.inner_position()?;
    let viewport = layout_bounds("left", TERMINAL_W, WINDOW_W, WINDOW_H,
        &terminal, &divider, &toolbar, &tabs, &tab_strip, origin, scale);
    let mut controller = crate::control::Controller::new(viewport.clone(), logs.join("actions.jsonl"))?;
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt, Err(e) => { eprintln!("IPC runtime error: {e}"); return; }
        };
        if let Err(e) = rt.block_on(crate::ipc::serve(proxy)) {
            eprintln!("IPC server error: {e}");
        }
    });

    let mut active = 0usize;
    let mut titles = vec!["New Tab".to_string(), "Fixture".to_string(),
                          "New Tab".to_string(), "New Tab".to_string()];
    let mut urls = tab_urls.clone();
    let refresh_tabs = |tabs_w: &wry::WebView, titles: &Vec<String>, urls: &Vec<String>, active: usize| {
        if let (Ok(t), Ok(u)) = (serde_json::to_string(titles), serde_json::to_string(urls)) {
            let _ = tabs_w.evaluate_script(&format!("window.__setTabs({t},{u},{active})"));
        }
    };
    for (i, tab) in tabs.iter().enumerate() { tab.set_visible(i == 0).ok(); }
    refresh_tabs(&tab_strip, &titles, &urls, active);
    println!("GUI ready: {} tabs, page viewport {:?}", tabs.len(), viewport);

    let mut term_ready = false;
    let mut pending_output: Vec<String> = Vec::new();
    let mut dock = "left".to_string();
    let mut term_size = TERMINAL_W;
    let mut win_w = WINDOW_W;
    let mut win_h = WINDOW_H;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            tao::event::Event::WindowEvent { event: tao::event::WindowEvent::Resized(size), .. } => {
                win_w = size.width as i32;
                win_h = size.height as i32;
                controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                    &terminal, &divider, &toolbar, &tabs, &tab_strip, origin, scale));
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
                crate::ipc::UserEvent::Ipc { cmd, reply } => {
                    if matches!(cmd, crate::protocol::Command::ListTabs) {
                        let list: Vec<crate::protocol::TabInfo> = titles.iter()
                            .zip(urls.iter()).enumerate()
                            .map(|(i, (t, u))| crate::protocol::TabInfo {
                                index: i, title: t.clone(), url: u.clone(), active: i == active,
                            }).collect();
                        let _ = reply.send(crate::protocol::Response::Tabs { tabs: list });
                        return;
                    }
                    if let crate::protocol::Command::FocusTab { index } = &cmd {
                        if *index < tabs.len() {
                            for (n, tab) in tabs.iter().enumerate() { tab.set_visible(n == *index).ok(); }
                            active = *index;
                            refresh_tabs(&tab_strip, &titles, &urls, active);
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
                    if let crate::protocol::Command::Dock { mode } = &cmd {
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
                    let needs_check = matches!(cmd,
                        crate::protocol::Command::Click { .. } | crate::protocol::Command::Type { .. });
                    if needs_check {
                        let relative = match &cmd {
                            crate::protocol::Command::Click { x, y, .. } => {
                                let v = controller.viewport();
                                Some((x - v.x, y - v.y))
                            }
                            _ => None,
                        };
                        let js = crate::control::Controller::password_check_js(relative);
                        let slot = Arc::new(Mutex::new(Some((cmd, reply))));
                        let slot_cb = slot.clone();
                        let proxy_cb = ui_proxy.clone();
                        if let Err(e) = tabs[active].evaluate_script_with_callback(&js, move |value| {
                            if let Some((cmd, reply)) = slot_cb.lock().unwrap().take() {
                                let _ = proxy_cb.send_event(crate::ipc::UserEvent::PasswordChecked {
                                    cmd, reply, is_password: value.contains("true"),
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
                crate::ipc::UserEvent::PasswordChecked { cmd, reply, is_password } => {
                    let response = controller.execute(&cmd, &tabs[active], is_password);
                    let _ = reply.send(response);
                }
                crate::ipc::UserEvent::BrowserAction { kind, value, tab } => {
                    let target = tab.unwrap_or(active).min(tabs.len() - 1);
                    match kind.as_str() {
                        "navigate" => {
                            let raw = value.unwrap_or_default();
                            let candidate = if raw.contains("://") { raw.clone() } else { format!("https://{raw}") };
                            match Url::parse(&candidate) {
                                Ok(u) if u.scheme() == "http" || u.scheme() == "https" => {
                                    let _ = tabs[target].load_url(u.as_str());
                                    let _ = toolbar.evaluate_script("window.__setStatus('')");
                                }
                                _ => { let _ = toolbar.evaluate_script("window.__setStatus('Use http(s)')"); }
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
                    if tab < urls.len() {
                        urls[tab] = url.clone();
                        titles[tab] = if url == newtab_url {
                            "New Tab".into()
                        } else {
                            Url::parse(&url).ok().and_then(|u| u.host_str().map(|h| h.to_string()))
                                .unwrap_or_else(|| url.clone())
                        };
                        refresh_tabs(&tab_strip, &titles, &urls, active);
                        if tab == active {
                            if let Ok(js) = serde_json::to_string(&url) {
                                let _ = toolbar.evaluate_script(&format!("window.__setUrl({js})"));
                            }
                        }
                    }
                }
                crate::ipc::UserEvent::TabAction { kind, index } => match kind.as_str() {
                    "focus" => {
                        if let Some(i) = index.filter(|i| *i < tabs.len()) {
                            for (n, tab) in tabs.iter().enumerate() { tab.set_visible(n == i).ok(); }
                            active = i;
                            refresh_tabs(&tab_strip, &titles, &urls, active);
                            if let Ok(js) = serde_json::to_string(&urls[active]) {
                                let _ = toolbar.evaluate_script(&format!("window.__setUrl({js})"));
                            }
                        }
                    }
                    "newtab" => {
                        let next = (active + 1) % tabs.len();
                        let _ = tabs[next].load_url(newtab_url);
                        for (n, tab) in tabs.iter().enumerate() { tab.set_visible(n == next).ok(); }
                        active = next;
                        urls[next] = newtab_url.to_string();
                        titles[next] = "New Tab".into();
                        refresh_tabs(&tab_strip, &titles, &urls, active);
                    }
                    _ => {}
                },
                crate::ipc::UserEvent::TabsReady => {
                    refresh_tabs(&tab_strip, &titles, &urls, active);
                }
                crate::ipc::UserEvent::SetDock { mode } => {
                    dock = if mode == "toggle" {
                        if dock == "left" { "bottom".into() } else { "left".into() }
                    } else { mode };
                    term_size = if dock == "left" { TERMINAL_W } else { win_h / 2 };
                    let _ = terminal.evaluate_script(&format!("window.__setDock('{dock}')"));
                    let _ = divider.evaluate_script(&format!("window.__setDock('{dock}')"));
                    controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                        &terminal, &divider, &toolbar, &tabs, &tab_strip, origin, scale));
                    let _ = terminal.evaluate_script("window.__fit && window.__fit()");
                }
                crate::ipc::UserEvent::SplitDelta { delta } => {
                    let max_size = if dock == "left" { win_w - TAB_W - 360 } else { win_h - 300 };
                    term_size = (term_size + delta).clamp(MIN_TERM, max_size);
                    controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                        &terminal, &divider, &toolbar, &tabs, &tab_strip, origin, scale));
                    let _ = terminal.evaluate_script("window.__fit && window.__fit()");
                }
            },
            _ => {}
        }
        let _ = (&terminal, &divider, &toolbar, &tabs, &tab_strip, &session, &context);
    });
}
