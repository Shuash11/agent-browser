//! Local named-pipe IPC between the MCP bridge process and the GUI process.
//! No TCP port is opened, and remote clients are rejected by Windows.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};
use tokio::sync::oneshot;

use crate::protocol::{Command, Response};

pub enum UserEvent {
    Ipc {
        cmd: Command,
        reply: oneshot::Sender<Response>,
    },
    /// Result of the asynchronous in-page password-field check.
    PasswordChecked {
        cmd: Command,
        reply: oneshot::Sender<Response>,
        is_password: bool,
        tab: usize,
    },
    /// Output chunk from the embedded OpenCode PTY.
    TermOutput { data: String },
    /// The xterm.js page is ready to receive output.
    TermReady,
    /// Layout change requested by the terminal/split UI.
    SetDock { mode: String },
    SplitDelta { delta: i32 },
    /// User action from the browser toolbar or a tab page. `tab` identifies the source tab.
    BrowserAction { kind: String, value: Option<String>, tab: Option<usize> },
    /// User action from the tab strip.
    TabAction { kind: String, index: Option<usize> },
    /// Tab strip page finished loading; safe to inject the tab list.
    TabsReady,
    /// Result of a page text/element/scroll query.
    PageQueryResult {
        kind: String,
        text: String,
        reply: oneshot::Sender<Response>,
        tab: usize,
    },
    /// User clicked "Check for updates" in the terminal header.
    UpdateCheck,
    /// Result of the background GitHub release check.
    UpdateChecked { result: Result<Option<crate::updater::ReleaseInfo>, String> },
    /// Download/install preparation failed.
    UpdateFailed { message: String },
    /// A tab finished loading; used to sync the address bar and tab strip.
    PageLoaded { url: String, tab: usize },
}

pub fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string());
    format!(r"\\.\pipe\agent-browser-{user}")
}

async fn read_line<R: AsyncReadExt + Unpin>(reader: &mut R) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = reader.read(&mut byte).await?;
        if n == 0 {
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        out.push(byte[0]);
        if out.len() > 1024 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "IPC line too long",
            ));
        }
    }
    Ok(out)
}

/// GUI side: accept one command per connection, dispatch to the event loop,
/// then write the response. Runs until the process exits.
pub async fn serve(proxy: tao::event_loop::EventLoopProxy<UserEvent>) -> Result<(), String> {
    loop {
        let mut server = ServerOptions::new()
            .reject_remote_clients(true)
            .create(pipe_name())
            .map_err(|e| format!("pipe create failed: {e}"))?;
        if let Err(e) = server.connect().await {
            eprintln!("agent-browser IPC connect error: {e}");
            continue;
        }
        let line = match read_line(&mut server).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("agent-browser IPC read error: {e}");
                continue;
            }
        };
        let cmd: Command = match serde_json::from_slice(&line) {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("invalid command JSON: {e}");
                let _ = server.write_all(msg.as_bytes()).await;
                let _ = server.write_all(b"\n").await;
                continue;
            }
        };
        let (tx, rx) = oneshot::channel();
        if proxy.send_event(UserEvent::Ipc { cmd, reply: tx }).is_err() {
            break; // event loop gone
        }
        let reply = tokio::time::timeout(Duration::from_secs(30), rx).await;
        let response = match reply {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => Response::Error { message: "GUI dropped the request".into() },
            Err(_) => Response::Error { message: "GUI reply timed out".into() },
        };
        if let Ok(json) = serde_json::to_vec(&response) {
            let _ = server.write_all(&json).await;
            let _ = server.write_all(b"\n").await;
        }
    }
    Ok(())
}

/// MCP side: send one command to the GUI and await its response.
pub async fn call(cmd: Command) -> Result<Response, String> {
    let name = pipe_name();
    for attempt in 0..12 {
        match ClientOptions::new().open(&name) {
            Ok(mut client) => {
                let mut payload = serde_json::to_vec(&cmd)
                    .map_err(|e| format!("serialize error: {e}"))?;
                payload.push(b'\n');
                client
                    .write_all(&payload)
                    .await
                    .map_err(|e| format!("pipe write error: {e}"))?;
                let line = read_line(&mut client)
                    .await
                    .map_err(|e| format!("pipe read error: {e}"))?;
                return serde_json::from_slice(&line)
                    .map_err(|e| format!("invalid response: {e}"));
            }
            Err(e)
                if (e.raw_os_error() == Some(231) || e.raw_os_error() == Some(2))
                    && attempt < 11 =>
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(e) => {
                return Err(format!(
                    "cannot reach GUI (start `agent-browser` first): {e}"
                ))
            }
        }
    }
    unreachable!()
}
