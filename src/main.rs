//! agent-browser — local browser panel controlled by OpenCode over MCP.

mod control;
mod gui;
mod ipc;
mod mcp;
mod protocol;
mod terminal;
mod updater;

fn main() {
    let subcommand = std::env::args().nth(1);
    match subcommand.as_deref() {
        Some("update-check") => match updater::check_latest() {
            Ok(Some(info)) => println!("update available: {} ({})", info.version, info.asset_name),
            Ok(None) => println!("up to date ({})", env!("CARGO_PKG_VERSION")),
            Err(err) => {
                eprintln!("update check error: {err}");
                std::process::exit(1);
            }
        },
        Some("update-download") => match updater::check_latest() {
            Ok(Some(info)) => {
                let path = std::env::temp_dir().join(&info.asset_name);
                match updater::download_verified(&info, &path) {
                    Ok(()) => println!("verified: {}", path.display()),
                    Err(err) => {
                        eprintln!("download verification failed: {err}");
                        std::process::exit(1);
                    }
                }
            }
            Ok(None) => println!("no update to download"),
            Err(err) => {
                eprintln!("update check error: {err}");
                std::process::exit(1);
            }
        },
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
