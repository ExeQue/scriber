//! `scriber mcp`: a stdio↔HTTP bridge for agents without HTTP support (and WSL without localhost).
//! Each JSON-RPC line from stdin is POSTed to the editor's /mcp; the replies (JSON or SSE events)
//! go to stdout. The server is stateless, so when the editor is gone the bridge (re)starts it and
//! retries the request.

use anyhow::{Result, bail};
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

struct Bridge {
    port: u16,
    launch: Box<dyn Fn() -> std::io::Result<()> + Send + Sync>,
    /// One caller at a time waits for the editor, so it is started once.
    starting: tokio::sync::Mutex<()>,
    /// Negotiated in `initialize`; sent as `MCP-Protocol-Version` afterwards.
    version: Mutex<Option<String>>,
}

/// The request never reached the editor (nothing listens): safe to retry.
struct Down;

pub(super) async fn bridge(
    port: u16,
    input: impl AsyncBufRead + Unpin,
    mut output: impl AsyncWrite + Unpin + Send + 'static,
    launch: impl Fn() -> std::io::Result<()> + Send + Sync + 'static,
) -> Result<()> {
    let b = Arc::new(Bridge { port, launch: Box::new(launch), starting: Default::default(), version: Mutex::new(None) });
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            output.write_all(line.as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
        std::io::Result::Ok(())
    });
    let mut lines = input.lines();
    let mut tasks = tokio::task::JoinSet::new();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let (b, tx) = (b.clone(), tx.clone());
        tasks.spawn(async move { b.handle(line, &tx).await });
    }
    while tasks.join_next().await.is_some() {}
    drop(tx);
    writer.await??;
    Ok(())
}

impl Bridge {
    async fn handle(&self, line: String, out: &mpsc::UnboundedSender<String>) {
        let id = serde_json::from_str::<Value>(&line).ok().and_then(|v| v.get("id").cloned());
        let err = match self.send(&line, out).await {
            Ok(()) => return,
            Err(e) => e,
        };
        // Notifications have no id and get no reply.
        if let Some(id) = id {
            let msg = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": format!("Scriber: {err:#}")}});
            let _ = out.send(msg.to_string());
        }
    }

    async fn send(&self, line: &str, out: &mpsc::UnboundedSender<String>) -> Result<()> {
        for _ in 0..3 {
            match self.post(line, out).await {
                Err(Ok(Down)) => self.wait_up().await?,
                Err(Err(e)) => return Err(e),
                Ok(()) => return Ok(()),
            }
        }
        bail!("Scriber keeps going away (port {})", self.port)
    }

    /// Start the editor unless it already answers, then wait until it listens.
    async fn wait_up(&self) -> Result<()> {
        let _one = self.starting.lock().await;
        let addr = ("127.0.0.1", self.port);
        if TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        (self.launch)()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            match TcpStream::connect(addr).await {
                Ok(_) => return Ok(()),
                Err(e) if Instant::now() > deadline => bail!("Scriber did not start listening on port {}: {e}", self.port),
                Err(_) => {}
            }
        }
    }

    /// POST one message and forward the replies as they arrive.
    async fn post(&self, line: &str, out: &mpsc::UnboundedSender<String>) -> Result<(), Result<Down>> {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).await.map_err(|_| Ok(Down))?;
        let (mut send, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.map_err(|e| Err(e.into()))?;
        tokio::spawn(conn);
        let mut req = Request::post("/mcp")
            .header("host", format!("127.0.0.1:{}", self.port))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(v) = self.version.lock().unwrap().as_deref() {
            req = req.header("mcp-protocol-version", v);
        }
        let req = req.body(Full::new(Bytes::from(line.to_owned()))).map_err(|e| Err(e.into()))?;
        let resp = send.send_request(req).await.map_err(|e| Err(e.into()))?;
        let status = resp.status();
        let sse = resp.headers().get("content-type").is_some_and(|c| c.as_bytes().starts_with(b"text/event-stream"));
        let mut body = resp.into_body();
        let mut buf = String::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|e| Err(e.into()))?;
            let Some(data) = frame.data_ref() else { continue };
            buf.push_str(&String::from_utf8_lossy(data));
            if sse {
                // One event per blank line; its `data:` lines are the message.
                while let Some(end) = buf.find("\n\n") {
                    let event: String = buf.drain(..end + 2).collect();
                    let data: Vec<&str> = event.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim_start).collect();
                    if !data.is_empty() && !data.concat().is_empty() {
                        self.forward(data.join("\n"), out);
                    }
                }
            }
        }
        if !status.is_success() {
            return Err(Err(anyhow::anyhow!("HTTP {status}: {}", buf.trim())));
        }
        if !sse && !buf.trim().is_empty() {
            self.forward(buf.trim().to_owned(), out);
        }
        Ok(())
    }

    fn forward(&self, msg: String, out: &mpsc::UnboundedSender<String>) {
        if let Some(v) = serde_json::from_str::<Value>(&msg).ok().and_then(|v| v["result"]["protocolVersion"].as_str().map(str::to_owned)) {
            *self.version.lock().unwrap() = Some(v);
        }
        let _ = out.send(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::http::tests::spawn;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[tokio::test]
    async fn round_trip_and_survives_a_restart() {
        let (port, server) = spawn(0);
        let launched = Arc::new(Mutex::new(0));
        let count = launched.clone();
        let (mut stdin, input) = tokio::io::duplex(1 << 16);
        let (output, stdout) = tokio::io::duplex(1 << 20);
        tokio::spawn(bridge(port, BufReader::new(input), output, move || {
            *count.lock().unwrap() += 1;
            spawn(port);
            Ok(())
        }));
        let mut replies = BufReader::new(stdout).lines();
        macro_rules! call {
            ($msg:expr) => {{
                stdin.write_all(format!("{}\n", $msg).as_bytes()).await.unwrap();
                serde_json::from_str::<Value>(&replies.next_line().await.unwrap().unwrap()).unwrap()
            }};
        }

        let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}});
        assert_eq!(call!(init)["result"]["protocolVersion"], "2025-06-18");
        stdin.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n").await.unwrap();
        let r = call!(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
        assert!(r.to_string().contains("get_document"), "{r}");
        let get = |id: u32| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"get_document","arguments":{}}});
        let r = call!(get(3));
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("\"width\":300"), "{r}");
        assert_eq!(*launched.lock().unwrap(), 0);

        server.abort();
        let _ = server.await;
        let r = call!(get(4));
        assert_eq!(r["id"], 4);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("\"width\":300"), "{r}");
        assert_eq!(*launched.lock().unwrap(), 1);
    }
}
