//! The agent side: a localhost listener inside the GUI, and `scriber mcp`, the stdio relay agents start.

mod tools;

pub use tools::Server;

use crate::model::{Document, Shared};
use rmcp::ServiceExt;
use std::sync::{Arc, Mutex};

pub const DEFAULT_PORT: u16 = 47821;

pub fn port() -> u16 {
    std::env::var("SCRIBER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

/// Serve MCP on 127.0.0.1 for the GUI's document. Returns the bound port or an error to show in the status bar.
pub fn listen(doc: Shared, repaint: Arc<dyn Fn() + Send + Sync>) -> anyhow::Result<u16> {
    let port = port();
    // ponytail: plain localhost TCP, reachable by every local user; switch to a per-user socket/pipe on shared machines.
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    std::thread::Builder::new().name("mcp".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
            loop {
                let Ok((stream, _)) = listener.accept().await else { continue };
                let server = Server::new(doc.clone(), repaint.clone());
                tokio::spawn(async move {
                    if let Ok(running) = server.serve(stream).await {
                        let _ = running.waiting().await;
                    }
                });
            }
        });
    })?;
    Ok(port)
}

/// `scriber mcp`: what agents run. Relays stdio to the GUI (starting it when needed), or with `--headless` serves a GUI-less document on stdio.
pub fn relay(headless: bool) -> anyhow::Result<()> {
    if headless {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
        return rt.block_on(async {
            let doc: Shared = Arc::new(Mutex::new(Document::new(300.0, 200.0)));
            let running = Server::new(doc, Arc::new(|| {})).serve(rmcp::transport::stdio()).await?;
            running.waiting().await?;
            Ok(())
        });
    }
    let addr = ("127.0.0.1", port());
    let stream = match std::net::TcpStream::connect(addr) {
        Ok(s) => s,
        Err(_) => {
            launch_gui()?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                match std::net::TcpStream::connect(addr) {
                    Ok(s) => break s,
                    Err(e) if std::time::Instant::now() > deadline => anyhow::bail!("Scriber did not start listening on port {}: {e}", addr.1),
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
                }
            }
        }
    };
    let mut up = stream.try_clone()?;
    std::thread::spawn(move || {
        let _ = pump(&mut std::io::stdin(), &mut up);
        let _ = up.shutdown(std::net::Shutdown::Write);
    });
    let mut down = stream;
    pump(&mut down, &mut std::io::stdout())?;
    Ok(())
}

/// Forward bytes as soon as they arrive. (Not `io::copy`: its Linux fast paths can sit on data.)
fn pump(from: &mut impl std::io::Read, to: &mut impl std::io::Write) -> std::io::Result<()> {
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        to.write_all(&buf[..n])?;
        to.flush()?;
    }
}

fn launch_gui() -> anyhow::Result<()> {
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
