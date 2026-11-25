//! GUI-side action executor. Enforces viewport scoping and credential safety.

use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use screenshots::Screen;
use wry::WebView;

use crate::protocol::{action_point, Command, MouseButton, Response, Viewport};

pub const WINDOW_TITLE: &str = "Agent Browser — embedded OpenCode";

/// Map a friendly key name to an enigo key. Returns None for printable
/// characters, which are handled as `Key::Unicode`.
fn named_key(name: &str) -> Option<Key> {
    Some(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "space" | "spacebar" => Key::Space,
        "esc" | "escape" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "up" | "uparrow" => Key::UpArrow,
        "down" | "downarrow" => Key::DownArrow,
        "left" | "leftarrow" => Key::LeftArrow,
        "right" | "rightarrow" => Key::RightArrow,
        "capslock" => Key::CapsLock,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => return None,
    })
}

pub struct Controller {
    viewport: Viewport,
    log_path: PathBuf,
    enigo: Enigo,
}

impl Controller {
    pub fn new(viewport: Viewport, log_path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let enigo = Enigo::new(&Settings::default())?;
        Ok(Self { viewport, log_path, enigo })
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.viewport = viewport;
    }

    /// Press a key or chord such as "Enter", "Ctrl+C", "Shift+Tab", "F5",
    /// or a single printable character. Modifiers are held only for the
    /// duration of the key press.
    fn dispatch_key(&mut self, key: &str) -> Response {
        let mut parts: Vec<String> = key
            .split('+')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if parts.is_empty() {
            return Response::Error { message: "empty key".into() };
        }
        let main = parts.pop().unwrap();
        let mut modifiers = Vec::new();
        for part in &parts {
            let modifier = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Key::Control,
                "shift" => Key::Shift,
                "alt" => Key::Alt,
                "meta" | "win" | "super" | "cmd" => Key::Meta,
                other => {
                    return Response::Error {
                        message: format!("unsupported modifier: {other}"),
                    }
                }
            };
            modifiers.push(modifier);
        }
        let main_key = match named_key(&main) {
            Some(k) => k,
            None if main.chars().count() == 1 => Key::Unicode(main.chars().next().unwrap()),
            None => {
                return Response::Error {
                    message: format!("unsupported key: {main}"),
                }
            }
        };
        for m in &modifiers {
            let _ = self.enigo.key(*m, Direction::Press);
        }
        let result = self.enigo.key(main_key, Direction::Click);
        for m in modifiers.iter().rev() {
            let _ = self.enigo.key(*m, Direction::Release);
        }
        if result.is_ok() {
            Response::Ok { message: format!("pressed {key}") }
        } else {
            Response::Error { message: format!("key {key} failed") }
        }
    }

    /// Build the in-page check. The result is delivered through a WebView2
    /// callback, which must NOT be awaited on the tao UI thread (deadlock).
    /// Fail closed: any unknown/iframe/shadow target is treated as sensitive.
    /// Coordinates are physical screen pixels relative to the viewport, so the
    /// page converts them with devicePixelRatio before calling elementFromPoint.
    pub fn password_check_js(relative: Option<(i32, i32)>) -> String {
        match relative {
            Some((x, y)) => format!(
                "(function(){{var d=window.devicePixelRatio||1;\
                 var e=document.elementFromPoint({x}/d,{y}/d);\
                 if(!e)return true;\
                 if(e.tagName==='IFRAME'||e.shadowRoot)return true;\
                 if(window.__abClosedHosts&&window.__abClosedHosts.has(e))return true;\
                 if(e.tagName.indexOf('-')>=0)return true;\
                 return !!(e.closest&&e.closest('input[type=password]'));}})()"
            ),
            None => "(function(){var e=document.activeElement;\
                      if(!e)return true;\
                      if(e.tagName==='IFRAME'||e.shadowRoot)return true;\
                      if(window.__abClosedHosts&&window.__abClosedHosts.has(e))return true;\
                      if(e.tagName.indexOf('-')>=0)return true;\
                      return !!(e.tagName==='INPUT'&&e.type==='password');})()"
                .to_string(),
        }
    }

    pub fn log_command(&self, cmd: &Command, ok: bool) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let name = match cmd {
            Command::Screenshot { .. } => "screenshot",
            Command::Move { .. } => "move",
            Command::Click { .. } => "click",
            Command::Scroll { .. } => "scroll",
            Command::Type { .. } => "type",
            Command::Key { .. } => "key",
            Command::ListTabs => "list_tabs",
            Command::FocusTab { .. } => "focus_tab",
            Command::GetViewport => "get_viewport",
            Command::Dock { .. } => "dock",
            Command::PageText => "page_text",
            Command::PageElements => "page_elements",
            Command::ScrollDom { .. } => "scroll_dom",
            Command::ScrollIntoView { .. } => "scroll_into_view",
            Command::Layout { .. } => "layout",
            Command::ClickId { .. } => "click_id",
            Command::TypeId { .. } => "type_id",
            Command::CloseTab { .. } => "close_tab",
            Command::Navigate { .. } => "navigate",
            Command::NewTab { .. } => "new_tab",
            Command::Back => "back",
            Command::Forward => "forward",
            Command::Reload => "reload",
            Command::GetPolicy { .. } => "get_policy",
            Command::Focus => "focus",
            Command::ModelStatus => "model_status",
            Command::ModelLoad { .. } => "model_load",
            Command::ModelUse { .. } => "model_use",
            Command::ModelStop => "model_stop",
        };
        let line = format!("{{\"ts\":{ts},\"action\":\"{name}\",\"ok\":{ok}}}\n");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn find_gui_window() -> windows_sys::Win32::Foundation::HWND {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, FindWindowW, GetWindowTextW,
        };
        let title: Vec<u16> = format!("{WINDOW_TITLE}\0").encode_utf16().collect();
        let exact = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if !exact.is_null() {
            return exact;
        }
        // The title can contain an em dash or a plain hyphen depending on the
        // build, so fall back to a prefix scan rather than failing outright.
        unsafe extern "system" fn scan(hwnd: HWND, param: isize) -> i32 {
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if n > 0 && String::from_utf16_lossy(&buf[..n as usize]).starts_with("Agent Browser") {
                *(param as *mut HWND) = hwnd;
                return 0;
            }
            1
        }
        let mut found: HWND = std::ptr::null_mut();
        unsafe {
            EnumWindows(Some(scan), &mut found as *mut HWND as isize);
        }
        found
    }

    fn focus_gui_window(&mut self) -> bool {
        use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId, Sleep};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
            SetForegroundWindow, ShowWindow, SW_RESTORE,
        };
        let hwnd = Self::find_gui_window();
        if hwnd.is_null() {
            return false;
        }
        unsafe {
            ShowWindow(hwnd, SW_RESTORE);
            for _ in 0..4 {
                let fg = GetForegroundWindow();
                let fg_thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());
                let our_thread = GetCurrentThreadId();
                let attached = fg_thread != 0
                    && fg_thread != our_thread
                    && AttachThreadInput(our_thread, fg_thread, 1) != 0;
                BringWindowToTop(hwnd);
                SetForegroundWindow(hwnd);
                if attached {
                    AttachThreadInput(our_thread, fg_thread, 0);
                }
                if GetForegroundWindow() == hwnd {
                    return true;
                }
                Sleep(60);
            }
            false
        }
    }

    /// `is_password` is resolved asynchronously by the GUI before calling here.
    pub fn execute(&mut self, cmd: &Command, webview: &WebView, is_password: bool) -> Response {
        // Focus is best-effort. A failed focus used to hard-block every input
        // tool; now the agent can call browser_focus explicitly and input
        // still proceeds, so window focus is never a hard block.
        let _ = webview.focus();
        let _ = self.focus_gui_window();
        // Coordinate actions are unrestricted by default; the user may still
        // opt back into page-only scoping with agent-policy.json.
        if let Some((x, y)) = action_point(cmd) {
            if !crate::policy::policy().allow_outside_viewport && !self.viewport.contains(x, y) {
                self.log_command(cmd, false);
                return Response::Error {
                    message: format!(
                        "point ({x},{y}) is outside the content viewport {:?}",
                        self.viewport
                    ),
                };
            }
        }

        let response = match cmd {
            Command::Screenshot { path } => {
                let v = &self.viewport;
                match Screen::from_point(v.x + 1, v.y + 1)
                    .and_then(|s| s.capture_area(v.x, v.y, v.width as u32, v.height as u32))
                {
                    Ok(img) => match img.save(path) {
                        Ok(()) => Response::Screenshot {
                            path: path.clone(),
                            width: v.width as u32,
                            height: v.height as u32,
                        },
                        Err(e) => Response::Error { message: e.to_string() },
                    },
                    Err(e) => Response::Error { message: e.to_string() },
                }
            }
            Command::Move { x, y } => {
                if self.enigo.move_mouse(*x, *y, Coordinate::Abs).is_ok() {
                    Response::Ok { message: "moved".into() }
                } else {
                    Response::Error { message: "mouse move failed".into() }
                }
            }
            Command::Click { x, y, button } => {
                if is_password && !crate::policy::policy().allow_password_entry {
                    Response::Error { message: "refused: password field".into() }
                } else {
                    let b = match button {
                        MouseButton::Left => Button::Left,
                        MouseButton::Right => Button::Right,
                        MouseButton::Middle => Button::Middle,
                    };
                    let _ = self.enigo.move_mouse(*x, *y, Coordinate::Abs);
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    if self.enigo.button(b, Direction::Click).is_ok() {
                        Response::Ok { message: "clicked".into() }
                    } else {
                        Response::Error { message: "click failed".into() }
                    }
                }
            }
            Command::Scroll { x, y, dx, dy } => {
                let _ = self.enigo.move_mouse(*x, *y, Coordinate::Abs);
                if *dy != 0 && self.enigo.scroll(*dy, Axis::Vertical).is_ok() {
                    Response::Ok { message: "scrolled".into() }
                } else if *dx != 0 && self.enigo.scroll(*dx, Axis::Horizontal).is_ok() {
                    Response::Ok { message: "scrolled horizontally".into() }
                } else {
                    Response::Error { message: "scroll failed".into() }
                }
            }
            Command::Type { text } => {
                if is_password && !crate::policy::policy().allow_password_entry {
                    Response::Error { message: "refused: password field".into() }
                } else if self.enigo.text(text).is_ok() {
                    Response::Ok { message: "typed".into() }
                } else {
                    Response::Error { message: "typing failed".into() }
                }
            }
            Command::Key { key } => self.dispatch_key(key),
            Command::ListTabs => Response::Tabs {
                tabs: vec![crate::protocol::TabInfo {
                    index: 0,
                    title: webview.url().unwrap_or_default(),
                    url: webview.url().unwrap_or_default(),
                    active: true,
                }],
            },
            Command::FocusTab { index } => {
                if *index == 0 {
                    Response::Ok { message: "focused tab 0".into() }
                } else {
                    Response::Error { message: "tab index out of range".into() }
                }
            }
            Command::GetViewport => Response::Viewport { viewport: self.viewport.clone() },
            Command::Focus => {
                if self.focus_gui_window() {
                    Response::Ok { message: "Agent Browser focused".into() }
                } else {
                    Response::Error { message: "could not focus Agent Browser window".into() }
                }
            }
            Command::Dock { .. } => Response::Ok { message: "dock requested".into() },
            Command::PageText | Command::PageElements | Command::ScrollDom { .. }
            | Command::ScrollIntoView { .. } | Command::Layout { .. }
            | Command::ClickId { .. } | Command::TypeId { .. }
            | Command::CloseTab { .. } | Command::Navigate { .. }
            | Command::NewTab { .. } | Command::Back | Command::Forward
            | Command::Reload | Command::GetPolicy { .. }
            | Command::ModelStatus | Command::ModelLoad { .. } | Command::ModelUse { .. }
            | Command::ModelStop => Response::Error {
                message: "command must be handled by the GUI event loop".into(),
            },
        };

        let ok = !matches!(response, Response::Error { .. });
        self.log_command(cmd, ok);
        response
    }
}
