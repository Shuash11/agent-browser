//! agent-browser — local browser panel controlled by OpenCode over MCP.

mod control;
mod gui;
mod ipc;
mod mcp;
mod protocol;
mod terminal;

fn main() {
    let subcommand = std::env::args().nth(1);
    match subcommand.as_deref() {
        Some("mcp") => {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("agent-browser MCP runtime error: {e}");
                    std::process::exit(1);
                }
            };
            if let Err(e) = rt.block_on(mcp::run()) {
                eprintln!("agent-browser MCP error: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            if let Err(err) = gui::run_gui() {
                eprintln!("agent-browser GUI error: {err}");
                std::process::exit(1);
            }
        }
    }
}
