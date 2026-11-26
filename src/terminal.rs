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

struct ChildGuard {
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

#[derive(Clone)]
pub struct TerminalSession {
    pub writer: TerminalWriter,
    master: TerminalMaster,
    guard: Arc<ChildGuard>,
}

impl TerminalSession {
    /// Spawn a CLI program inside a ConPTY, cwd = runtime app root. Output
    /// chunks are delivered to the GUI as `UserEvent::TermOutput`.
    pub fn spawn(
        prog: &str,
        args: &[&str],
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
        for arg in args {
            cmd.arg(arg);
        }
        cmd.cwd(cwd);
        let child = pair.slave.spawn_command(cmd)?;
        // Own the child immediately so any later setup failure still kills it.
        let guard = Arc::new(ChildGuard { child: Mutex::new(child) });
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let mut reader = pair.master.try_clone_reader()?;
        let master = Arc::new(Mutex::new(pair.master));

        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut pending: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&buf[..n]);
                        match String::from_utf8(pending.clone()) {
                            Ok(data) => {
                                pending.clear();
                                let _ = proxy.send_event(UserEvent::TermOutput { data });
                            }
                            Err(e) => {
                                let valid = e.utf8_error().valid_up_to();
                                let data = String::from_utf8_lossy(&pending[..valid]).into_owned();
                                if !data.is_empty() {
                                    let _ = proxy.send_event(UserEvent::TermOutput { data });
                                }
                                pending.drain(..valid);
                                if pending.len() > 3 { pending.clear(); }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            if !pending.is_empty() {
                let data = String::from_utf8_lossy(&pending).into_owned();
                let _ = proxy.send_event(UserEvent::TermOutput { data });
            }
        });

        Ok(Self { writer, master, guard })
    }

    pub fn terminate(&self) {
        if let Ok(mut child) = self.guard.child.lock() {
            let _ = child.kill();
        }
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

    pub fn writer_handle(&self) -> TerminalWriter {
        self.writer.clone()
    }
}
