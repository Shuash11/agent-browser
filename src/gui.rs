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

fn page_text_js() -> String {
    "(function(){return JSON.stringify({title:document.title,url:location.href,\
     text:(document.body&&document.body.innerText||'').slice(0,8000)});})()".to_string()
}

fn page_elements_js() -> String {
    "(function(){var out=[];var els=document.querySelectorAll(\
     'a,button,input,textarea,select,[role=button],[role=link],[role=textbox],[onclick]');\
     for(var i=0;i<els.length;i++){var e=els[i];var r=e.getBoundingClientRect();\
     if(r.width<2||r.height<2||r.bottom<0||r.top>window.innerHeight)continue;\
     var s=getComputedStyle(e);if(s.visibility==='hidden'||s.display==='none'||s.opacity==='0')continue;\
     var label=(e.innerText||e.value||e.getAttribute('aria-label')||e.placeholder||'');\
     out.push({tag:e.tagName,role:e.getAttribute('role')||'',\
     label:String(label).trim().slice(0,100),\
     x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),\
     w:Math.round(r.width),h:Math.round(r.height)});\
     if(out.length>=120)break;}\
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
         if(area>0&&area<bestArea){{best=e;bestArea=area;bestText=t.slice(0,120);}}}}\
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
            x: origin.x + (area_x as f64 * scale) as i32,
            y: origin.y + (TOOLBAR_H as f64 * scale) as i32,
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
var orig = Element.prototype.attachShadow;
Element.prototype.attachShadow = function (init) {
  try { window.__abClosedHosts.add(this); } catch (e) {}
  return orig.call(this, init);
};
})();"#;

fn opencode_command() -> (String, Vec<String>) {
    let npm = std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("npm").join("opencode.cmd"))
        .unwrap_or_default();
    if npm.is_file() {
        return (npm.to_string_lossy().to_string(), Vec::new());
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["opencode.cmd", "opencode.exe", "opencode"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return (candidate.to_string_lossy().to_string(), Vec::new());
                }
            }
        }
    }
    // Last resort: let cmd.exe resolve it from PATH or show a clear error.
    ("cmd.exe".to_string(), vec!["/c".to_string(), "opencode".to_string()])
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

    let (opencode_prog, opencode_args) = opencode_command();
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
            .with_initialization_script(SHADOW_TRACK_JS)
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

    let mut scale = window.scale_factor();
    let mut origin = window.inner_position()?;
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
    println!("GUI ready: root={}; {} tabs, page viewport {:?}", root.display(), tabs.len(), viewport);

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
                controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                    &terminal, &divider, &toolbar, &tabs, &tab_strip, origin, scale));
                let _ = terminal.evaluate_script("window.__fit && window.__fit()");
            }
            tao::event::Event::WindowEvent { event: tao::event::WindowEvent::Moved(pos), .. } => {
                origin = pos;
                controller.set_viewport(layout_bounds(&dock, term_size, win_w, win_h,
                    &terminal, &divider, &toolbar, &tabs, &tab_strip, origin, scale));
            }
            tao::event::Event::WindowEvent {
                event: tao::event::WindowEvent::ScaleFactorChanged { scale_factor, .. }, ..
            } => {
                scale = scale_factor;
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
                        controller.log_command(&cmd, true);
                        let list: Vec<crate::protocol::TabInfo> = titles.iter()
                            .zip(urls.iter()).enumerate()
                            .map(|(i, (t, u))| crate::protocol::TabInfo {
                                index: i, title: t.clone(), url: u.clone(), active: i == active,
                            }).collect();
                        let _ = reply.send(crate::protocol::Response::Tabs { tabs: list });
                        return;
                    }
                    if let crate::protocol::Command::FocusTab { index } = &cmd {
                        controller.log_command(&cmd, *index < tabs.len());
                        if *index < tabs.len() {
                            active = activate_tab(*index, &tabs, &toolbar, &urls);
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
                    if let crate::protocol::Command::ScrollDom { dx, dy } = &cmd {
                        let js = format!("window.scrollBy({dx},{dy});");
                        let ok = tabs[active].evaluate_script(&js).is_ok();
                        controller.log_command(&cmd, ok);
                        let _ = reply.send(if ok {
                            crate::protocol::Response::Ok { message: "scrolled DOM".into() }
                        } else {
                            crate::protocol::Response::Error { message: "DOM scroll failed".into() }
                        });
                        return;
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
                        let target_tab = active;
                        let slot = Arc::new(Mutex::new(Some((cmd, reply))));
                        let slot_cb = slot.clone();
                        let proxy_cb = ui_proxy.clone();
                        if let Err(e) = tabs[target_tab].evaluate_script_with_callback(&js, move |value| {
                            if let Some((cmd, reply)) = slot_cb.lock().unwrap().take() {
                                let _ = proxy_cb.send_event(crate::ipc::UserEvent::PasswordChecked {
                                    cmd, reply, is_password: value.contains("true"), tab: target_tab,
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
                crate::ipc::UserEvent::PasswordChecked { cmd, reply, is_password, tab } => {
                    if tab != active {
                        let _ = reply.send(crate::protocol::Response::Error {
                            message: "tab changed during safety check; action cancelled".into(),
                        });
                        return;
                    }
                    let response = controller.execute(&cmd, &tabs[tab], is_password);
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
                            active = activate_tab(i, &tabs, &toolbar, &urls);
                            refresh_tabs(&tab_strip, &titles, &urls, active);
                        }
                    }
                    "newtab" => {
                        let next = (active + 1) % tabs.len();
                        let _ = tabs[next].load_url(newtab_url);
                        urls[next] = newtab_url.to_string();
                        titles[next] = "New Tab".into();
                        active = activate_tab(next, &tabs, &toolbar, &urls);
                        refresh_tabs(&tab_strip, &titles, &urls, active);
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
                    let output = if kind == "page_elements" {
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(mut v) => {
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
                                serde_json::to_string(&v).unwrap_or(text)
                            }
                            Err(_) => text,
                        }
                    } else {
                        text
                    };
                    let _ = reply.send(crate::protocol::Response::Text { text: output });
                }
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
