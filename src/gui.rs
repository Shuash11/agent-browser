//! GUI shell: tao window with embedded OpenCode terminal, browser toolbar, page, and splitter.

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
const MIN_WINDOW_W: u32 = 1000;
const MIN_WINDOW_H: u32 = 640;
const TERMINAL_W: i32 = 540;
const DIVIDER_W: i32 = 8;
const MIN_TERM: i32 = 260;
const TOOLBAR_H: i32 = 44;

fn project_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
    content: &wry::WebView,
    origin: tao::dpi::PhysicalPosition<i32>,
    scale: f64,
) -> crate::protocol::Viewport {
    if dock == "left" {
        let max_term = (w * 38 / 100).max(MIN_TERM).min(w - 400);
        let ts = term_size.min(max_term).max(MIN_TERM);
        terminal.set_bounds(rect(0, 0, ts, h)).ok();
        divider.set_bounds(rect(ts, 0, DIVIDER_W, h)).ok();
        toolbar.set_bounds(rect(ts + DIVIDER_W, 0, w - ts - DIVIDER_W, TOOLBAR_H)).ok();
        content.set_bounds(rect(ts + DIVIDER_W, TOOLBAR_H,
            w - ts - DIVIDER_W, h - TOOLBAR_H)).ok();
        crate::protocol::Viewport {
            x: (origin.x as f64 * scale) as i32
                + ((ts + DIVIDER_W) as f64 * scale) as i32,
            y: (origin.y as f64 * scale) as i32 + (TOOLBAR_H as f64 * scale) as i32,
            width: ((w - ts - DIVIDER_W) as f64 * scale) as i32,
            height: ((h - TOOLBAR_H) as f64 * scale) as i32,
        }
    } else {
        let max_term = (h * 42 / 100).max(180).min(h - 300);
        let ts = term_size.min(max_term).max(180);
        terminal.set_bounds(rect(0, h - ts, w, ts)).ok();
        divider.set_bounds(rect(0, h - ts - DIVIDER_W, w, DIVIDER_W)).ok();
        toolbar.set_bounds(rect(0, 0, w, TOOLBAR_H)).ok();
        content.set_bounds(rect(0, TOOLBAR_H, w, h - ts - DIVIDER_W - TOOLBAR_H)).ok();
        crate::protocol::Viewport {
            x: (origin.x as f64 * scale) as i32,
            y: (origin.y as f64 * scale) as i32 + (TOOLBAR_H as f64 * scale) as i32,
            width: (w as f64 * scale) as i32,
            height: ((h - ts - DIVIDER_W - TOOLBAR_H) as f64 * scale) as i32,
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
    let session = crate::terminal::TerminalSession::spawn(
        &npm.to_string_lossy(), &root, term_proxy,
    )?;
    let writer = session.writer_handle();
    let resize_session = session.clone();
    let term_proxy_ipc = event_loop.create_proxy();

    let term_dir = root.join("assets").join("terminal");
    let term_assets = term_dir.clone();
    let term_url = "http://abterm.localhost/index.html".to_string();

    let mut context = WebContext::new(Some(data_dir));

    let terminal = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(0, 0, TERMINAL_W, WINDOW_H))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            let path = request.uri().path();
            let (file, mime) = match path {
                "/xterm.min.js" => ("xterm.min.js", "application/javascript"),
                "/xterm.min.css" => ("xterm.min.css", "text/css"),
                "/addon-fit.min.js" => ("addon-fit.min.js", "application/javascript"),
                "/divider.html" => ("divider.html", "text/html"),
                "/toolbar.html" => ("toolbar.html", "text/html"),
                "/theme.css" => ("theme.css", "text/css"),
                _ => ("index.html", "text/html"),
            };
            let bytes = std::fs::read(term_assets.join(file)).unwrap_or_default();
            wry::http::Response::builder()
                .header("content-type", mime)
                .header("cache-control", "no-store")
                .body(std::borrow::Cow::Owned(bytes))
                .unwrap()
        })
        .with_url(&term_url)
        .with_ipc_handler(move |request| {
            let body = request.body();
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(body) {
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
                    Some("jserror") => {
                        eprintln!("terminal JS error: {:?}", msg.get("message"));
                    }
                    _ => {}
                }
            }
        })
        .build_as_child(&window)?;

    let divider_proxy = event_loop.create_proxy();
    let divider_assets = term_dir.clone();
    let divider = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(TERMINAL_W, 0, DIVIDER_W, WINDOW_H))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            let file = if request.uri().path() == "/theme.css" { "theme.css" } else { "divider.html" };
            let bytes = std::fs::read(divider_assets.join(file)).unwrap_or_default();
            wry::http::Response::builder()
                .header("content-type", "text/html")
                .header("cache-control", "no-store")
                .body(std::borrow::Cow::Owned(bytes))
                .unwrap()
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

    let browser_proxy = event_loop.create_proxy();
    let toolbar_proxy = browser_proxy.clone();
    let toolbar_assets = term_dir.clone();
    let toolbar_url = match std::env::var("AGENT_BROWSER_TEST_NAV") {
        Ok(url) => format!("http://abterm.localhost/toolbar.html?nav={}", urlencoding(&url)),
        Err(_) => "http://abterm.localhost/toolbar.html".to_string(),
    };
    let toolbar = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(TERMINAL_W + DIVIDER_W, 0,
            WINDOW_W - TERMINAL_W - DIVIDER_W, TOOLBAR_H))
        .with_custom_protocol("abterm".into(), move |_id, request| {
            let file = if request.uri().path() == "/theme.css" { "theme.css" } else { "toolbar.html" };
            let bytes = std::fs::read(toolbar_assets.join(file)).unwrap_or_default();
            wry::http::Response::builder()
                .header("content-type", if file == "theme.css" { "text/css" } else { "text/html" })
                .header("cache-control", "no-store")
                .body(std::borrow::Cow::Owned(bytes))
                .unwrap()
        })
        .with_url(&toolbar_url)
        .with_ipc_handler(move |request| {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(request.body()) {
                let kind = msg.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let value = msg.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                let _ = toolbar_proxy.send_event(crate::ipc::UserEvent::BrowserAction {
                    kind, value,
                });
            }
        })
        .build_as_child(&window)?;

    let fixture = root.join("tests").join("fixture.html");
    let fixture_url = Url::from_file_path(&fixture)
        .map_err(|_| format!("invalid fixture path: {}", fixture.display()))?
        .to_string();
    let download_dir = downloads.clone();
    let log_dir = logs.clone();
    let load_proxy = browser_proxy.clone();
    let content = WebViewBuilder::new_with_web_context(&mut context)
        .with_bounds(rect(TERMINAL_W + DIVIDER_W, TOOLBAR_H,
            WINDOW_W - TERMINAL_W - DIVIDER_W, WINDOW_H - TOOLBAR_H))
        .with_url(&fixture_url)
        .with_on_page_load_handler(move |_event, url| {
            let _ = load_proxy.send_event(crate::ipc::UserEvent::PageLoaded { url });
        })
        .with_download_started_handler(move |_url, path| {
            let p = path;
            *p = download_dir.join(p.file_name().unwrap_or_default());
            true
        })
        .with_download_completed_handler(move |url, path, ok| {
            let line = format!(
                "{{\"url\":{:?},\"path\":{:?},\"ok\":{}}}\n",
                url,
                path.as_ref().map(|p| p.to_string_lossy().to_string()),
                ok
            );
            let _ = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_dir.join("downloads.jsonl"))
                .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
        })
        .build_as_child(&window)?;

    let scale = window.scale_factor();
    let origin = window.inner_position()?;
    let viewport = layout_bounds("left", TERMINAL_W, WINDOW_W, WINDOW_H,
        &terminal, &divider, &toolbar, &content, origin, scale);
    let mut controller = crate::control::Controller::new(viewport.clone(), logs.join("actions.jsonl"))?;
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => { eprintln!("agent-browser IPC runtime error: {e}"); return; }
        };
        if let Err(e) = rt.block_on(crate::ipc::serve(proxy)) {
            eprintln!("agent-browser IPC server error: {e}");
        }
    });

    println!("GUI ready: page viewport {:?}", viewport);

    let mut term_ready = false;
    let mut pending_output: Vec<String> = Vec::new();
    let mut dock = "left".to_string();
    let mut term_size = TERMINAL_W;
    let mut win_w = WINDOW_W;
    let mut win_h = WINDOW_H;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            tao::event::Event::WindowEvent {
                event: tao::event::WindowEvent::Resized(size), ..
            } => {
                win_w = size.width as i32;
                win_h = size.height as i32;
                controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                    &terminal, &divider, &toolbar, &content, origin, scale));
                let _ = terminal.evaluate_script("window.__fit && window.__fit()");
            }
            tao::event::Event::UserEvent(user_event) => match user_event {
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
                    if let crate::protocol::Command::Dock { mode } = &cmd {
                        let _ = ui_proxy.send_event(crate::ipc::UserEvent::SetDock {
                            mode: mode.clone(),
                        });
                        let _ = reply.send(crate::protocol::Response::Ok {
                            message: "dock requested".into(),
                        });
                        return;
                    }
                    let needs_check = matches!(cmd,
                        crate::protocol::Command::Click { .. }
                            | crate::protocol::Command::Type { .. });
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
                        if let Err(e) = content.evaluate_script_with_callback(&js, move |value| {
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
                        let response = controller.execute(&cmd, &content, false);
                        let _ = reply.send(response);
                    }
                }
                crate::ipc::UserEvent::PasswordChecked { cmd, reply, is_password } => {
                    let response = controller.execute(&cmd, &content, is_password);
                    let _ = reply.send(response);
                }
                crate::ipc::UserEvent::BrowserAction { kind, value } => match kind.as_str() {
                    "navigate" => {
                        let raw = value.unwrap_or_default();
                        let candidate = if raw.contains("://") { raw.clone() } else { format!("https://{raw}") };
                        match Url::parse(&candidate) {
                            Ok(u) if u.scheme() == "http" || u.scheme() == "https" => {
                                let _ = content.load_url(u.as_str());
                                let _ = toolbar.evaluate_script("window.__setStatus('')");
                            }
                            _ => { let _ = toolbar.evaluate_script("window.__setStatus('Use http(s)')"); }
                        }
                    }
                    "back" => { let _ = content.go_back(); }
                    "forward" => { let _ = content.go_forward(); }
                    "reload" => { let _ = content.reload(); }
                    "home" => { let _ = content.load_url(&fixture_url); }
                    _ => {}
                },
                crate::ipc::UserEvent::PageLoaded { url } => {
                    if let Ok(js) = serde_json::to_string(&url) {
                        let _ = toolbar.evaluate_script(&format!("window.__setUrl({js})"));
                    }
                }
                crate::ipc::UserEvent::SetDock { mode } => {
                    dock = if mode == "toggle" {
                        if dock == "left" { "bottom".into() } else { "left".into() }
                    } else { mode };
                    term_size = if dock == "left" { TERMINAL_W } else { win_h / 2 };
                    let _ = terminal.evaluate_script(&format!("window.__setDock('{dock}')"));
                    let _ = divider.evaluate_script(&format!("window.__setDock('{dock}')"));
                    controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                        &terminal, &divider, &toolbar, &content, origin, scale));
                    let _ = terminal.evaluate_script("window.__fit && window.__fit()");
                }
                crate::ipc::UserEvent::SplitDelta { delta } => {
                    let max_size = if dock == "left" { win_w - 400 } else { win_h - 300 };
                    term_size = (term_size + delta).clamp(MIN_TERM, max_size);
                    controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                        &terminal, &divider, &toolbar, &content, origin, scale));
                    let _ = terminal.evaluate_script("window.__fit && window.__fit()");
                }
            },
            _ => {}
        }
        let _ = (&terminal, &divider, &toolbar, &content, &session);
    });
}
