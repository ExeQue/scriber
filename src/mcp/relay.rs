//! `scriber mcp`: a stdio↔HTTP bridge for agents without HTTP support (and WSL without localhost).
//! Each JSON-RPC line from stdin is POSTed to the editor's /mcp; the replies (JSON or SSE events)
//! go to stdout. The server is stateless, so when the editor is gone the bridge (re)starts it and
//! retries the request.
//!
//! The bridge is the one long-lived piece, so it also tells the agent when the editor's tools change
//! (a rebuilt editor restarted under a connected agent): it polls `tools/list` itself and sends
//! `notifications/tools/list_changed` when the list differs from the one the agent last got.
//! The editor has no resources or prompts, so only tools are watched.

use anyhow::{Result, bail};
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// Fingerprint of the tool list the agent last got (or the bridge last polled).
    listed: Mutex<Option<u64>>,
    polls: AtomicU64,
}

/// How often the bridge checks the editor's tool list; faster while the editor is down, so a
/// restarted editor is noticed right away.
const POLL: Duration = if cfg!(test) { Duration::from_millis(50) } else { Duration::from_secs(5) };
const POLL_DOWN: Duration = if cfg!(test) { Duration::from_millis(20) } else { Duration::from_secs(1) };
/// The bridge's own request ids; their replies never reach the agent.
const POLL_ID: &str = "scriber-relay-poll-";

/// The request never reached the editor (nothing listens): safe to retry.
struct Down;

pub(super) async fn bridge(
    port: u16,
    input: impl AsyncBufRead + Unpin,
    mut output: impl AsyncWrite + Unpin + Send + 'static,
    launch: impl Fn() -> std::io::Result<()> + Send + Sync + 'static,
) -> Result<()> {
    let b = Arc::new(Bridge {
        port,
        launch: Box::new(launch),
        starting: Default::default(),
        version: Mutex::new(None),
        listed: Mutex::new(None),
        polls: AtomicU64::new(0),
    });
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
    let mut poller = None;
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        // The agent may only get notifications once it has said `initialized`.
        if poller.is_none() && serde_json::from_str::<Value>(&line).is_ok_and(|v| v["method"] == "notifications/initialized") {
            poller = Some(tokio::spawn(b.clone().poll(tx.clone())));
        }
        let (b, tx) = (b.clone(), tx.clone());
        tasks.spawn(async move { b.handle(line, &tx).await });
    }
    while tasks.join_next().await.is_some() {}
    if let Some(p) = poller {
        p.abort();
        let _ = p.await;
    }
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

    /// Ask the editor for its tools every few seconds; `forward` compares the answers.
    async fn poll(self: Arc<Self>, out: mpsc::UnboundedSender<String>) {
        let mut wait = POLL;
        loop {
            tokio::time::sleep(wait).await;
            let n = self.polls.fetch_add(1, Ordering::Relaxed);
            let msg = json!({"jsonrpc": "2.0", "id": format!("{POLL_ID}{n}"), "method": "tools/list"}).to_string();
            // Never starts the editor: a closed editor just means "look again soon".
            wait = if matches!(self.post(&msg, &out).await, Err(Ok(Down))) { POLL_DOWN } else { POLL };
        }
    }

    fn forward(&self, msg: String, out: &mpsc::UnboundedSender<String>) {
        let Ok(mut v) = serde_json::from_str::<Value>(&msg) else {
            let _ = out.send(msg);
            return;
        };
        if v["id"].as_str().is_some_and(|id| id.starts_with(POLL_ID)) {
            if let Some(tools) = v["result"].get("tools") {
                let fp = fingerprint(tools);
                if self.listed.lock().unwrap().replace(fp).is_some_and(|old| old != fp) {
                    let _ = out.send(r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#.to_owned());
                }
            }
            return;
        }
        if let Some(tools) = v["result"].get("tools") {
            *self.listed.lock().unwrap() = Some(fingerprint(tools));
        }
        if let Some(version) = v["result"]["protocolVersion"].as_str() {
            // The initialize reply: the stateless editor cannot announce tool changes, the bridge can.
            *self.version.lock().unwrap() = Some(version.to_owned());
            v["result"]["capabilities"]["tools"]["listChanged"] = json!(true);
            let _ = out.send(v.to_string());
            return;
        }
        let _ = out.send(msg);
    }
}

fn fingerprint(tools: &Value) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    tools.to_string().hash(&mut h);
    h.finish()
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

    /// A fake editor whose tool list is `tools`; counts the bridge's own `tools/list` polls.
    fn fake(tools: Arc<Mutex<Vec<&'static str>>>, polls: Arc<AtomicU64>) -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        let l = tokio::net::TcpListener::from_std(l).unwrap();
        tokio::spawn(async move {
            loop {
                let (stream, _) = l.accept().await.unwrap();
                let (tools, polls) = (tools.clone(), polls.clone());
                let handle = hyper::service::service_fn(move |req: Request<hyper::body::Incoming>| {
                    let (tools, polls) = (tools.clone(), polls.clone());
                    async move {
                        let v: Value = serde_json::from_slice(&req.into_body().collect().await?.to_bytes()).unwrap();
                        let result = match v["method"].as_str() {
                            Some("initialize") => json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}),
                            Some("tools/list") => {
                                if v["id"].is_string() {
                                    polls.fetch_add(1, Ordering::Relaxed);
                                }
                                let list: Vec<Value> = tools.lock().unwrap().iter().map(|n| json!({"name": n, "inputSchema": {"type": "object"}})).collect();
                                json!({"tools": list})
                            }
                            _ => return Ok::<_, hyper::Error>(hyper::Response::builder().status(202).body(Full::new(Bytes::new())).unwrap()),
                        };
                        let body = json!({"jsonrpc":"2.0","id":v["id"],"result":result}).to_string();
                        Ok(hyper::Response::builder().header("content-type", "application/json").body(Full::new(Bytes::from(body))).unwrap())
                    }
                });
                tokio::spawn(hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), handle));
            }
        });
        port
    }

    #[tokio::test]
    async fn tells_the_agent_once_when_the_tools_change() {
        let tools = Arc::new(Mutex::new(vec!["a"]));
        let polls = Arc::new(AtomicU64::new(0));
        let port = fake(tools.clone(), polls.clone());
        let (mut stdin, input) = tokio::io::duplex(1 << 16);
        let (output, stdout) = tokio::io::duplex(1 << 20);
        tokio::spawn(bridge(port, BufReader::new(input), output, || Ok(())));
        let mut replies = BufReader::new(stdout).lines();
        macro_rules! send {
            ($msg:expr) => {
                stdin.write_all(format!("{}\n", $msg).as_bytes()).await.unwrap()
            };
        }
        macro_rules! next {
            () => {
                serde_json::from_str::<Value>(&replies.next_line().await.unwrap().unwrap()).unwrap()
            };
        }
        let pause = || tokio::time::sleep(POLL * 6);

        send!(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}));
        let r = next!();
        assert_eq!(r["result"]["capabilities"]["tools"]["listChanged"], true, "{r}");
        send!(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
        assert_eq!(next!()["result"]["tools"][0]["name"], "a");

        // Not initialized yet: no polls, so no notification.
        tools.lock().unwrap().push("b");
        pause().await;
        assert_eq!(polls.load(Ordering::Relaxed), 0);

        send!(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        assert_eq!(next!(), json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"}));
        pause().await;
        assert!(polls.load(Ordering::Relaxed) > 2);
        // Exactly one notification and no poll replies: the next line is the agent's own reply.
        send!(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}));
        let r = next!();
        assert_eq!(r["id"], 3, "{r}");
        assert_eq!(r["result"]["tools"][1]["name"], "b");

        tools.lock().unwrap().push("c");
        assert_eq!(next!(), json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"}));
    }
}
