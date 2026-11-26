//! The agent side: the editor's MCP endpoint (streamable HTTP on localhost) and `scriber mcp`,
//! the stdio bridge for agents that cannot speak HTTP.

mod constraint_tools;
mod http;
mod relay;
mod tools;

pub use http::url;
pub use tools::Server;

use crate::model::{Document, Shared};
use rmcp::ServiceExt;
use std::sync::{Arc, Mutex};

pub const DEFAULT_PORT: u16 = 47821;

pub fn port() -> u16 {
    std::env::var("SCRIBER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

/// Serve MCP over HTTP on 127.0.0.1 for the GUI's document. Returns the bound port or an error to show in the status bar.
pub fn listen(doc: Shared, repaint: Arc<dyn Fn() + Send + Sync>) -> anyhow::Result<u16> {
    let port = port();
    // ponytail: localhost HTTP, reachable by every local user; switch to a per-user socket/pipe on shared machines.
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    std::thread::Builder::new().name("mcp".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
            http::serve(listener, move || Server::new(doc.clone(), repaint.clone())).await;
        });
    })?;
    Ok(port)
}

/// `scriber mcp`: what stdio agents run. Bridges stdio to the editor (starting it when needed), or with `--headless` serves a GUI-less document on stdio.
pub fn relay(headless: bool) -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async {
        if headless {
            let doc: Shared = Arc::new(Mutex::new(Document::new(300.0, 200.0)));
            let running = Server::new(doc, Arc::new(|| {})).serve(rmcp::transport::stdio()).await?;
            running.waiting().await?;
            return Ok(());
        }
        relay::bridge(port(), tokio::io::BufReader::new(tokio::io::stdin()), tokio::io::stdout(), launch_gui).await
    })
}

fn launch_gui() -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        // Own process group, so the GUI survives when the agent stops this relay.
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn()?;
    Ok(())
}
