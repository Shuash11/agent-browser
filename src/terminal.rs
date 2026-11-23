//! Embedded OpenCode terminal: PTY lifecycle and input/output bridge.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tao::event_loop::EventLoopProxy;

use crate::ipc::UserEvent;

pub type TerminalWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type TerminalMaster = Arc<Mutex<Box<dyn MasterPty + Send>>>;

#[derive(Clone)]
pub struct TerminalSession {
    pub writer: TerminalWriter,
    master: TerminalMaster,
    _child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
}

impl TerminalSession {
    /// Spawn a CLI program inside a ConPTY, cwd = repo root. Output chunks are
    /// delivered to the GUI as `UserEvent::TermOutput`.
    pub fn spawn(
        prog: &str,
        cwd: &PathBuf,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let system = native_pty_system();
        let pair = system.openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(prog);
        cmd.cwd(cwd);
        let child = pair.slave.spawn_command(cmd)?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let mut reader = pair.master.try_clone_reader()?;
        let master = Arc::new(Mutex::new(pair.master));
        let child = Arc::new(Mutex::new(child));

        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = proxy.send_event(UserEvent::TermOutput { data });
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { writer, master, _child: child })
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Ok(m) = self.master.lock() {
            let _ = m.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    /// Clone the writer handle for the WebView IPC handler.
    pub fn writer_handle(&self) -> TerminalWriter {
        self.writer.clone()
    }
}
