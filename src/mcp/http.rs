//! The editor's MCP endpoint: streamable HTTP at `http://127.0.0.1:<port>/mcp`.
//!
//! Stateless: no session ids, every request stands on its own, so agents keep working after the
//! editor restarts. Only loopback `Host`s and localhost `Origin`s get in (DNS rebinding, CSRF).

use super::Server;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use std::sync::Arc;
use tower_service::Service;

pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

/// Accept connections until the listener's task is dropped. `make` builds the server for one request.
pub(super) async fn serve(listener: tokio::net::TcpListener, make: impl Fn() -> Server + Send + Sync + 'static) {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or_default();
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_allowed_hosts([format!("127.0.0.1:{port}"), format!("localhost:{port}")])
        .with_allowed_origins(["http://localhost:*", "http://127.0.0.1:*", "https://localhost:*", "https://127.0.0.1:*"]);
    let mcp = StreamableHttpService::new(move || Ok(make()), Arc::new(NeverSessionManager::default()), config);
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let mcp = mcp.clone();
        let handle = hyper::service::service_fn(move |req: Request<Incoming>| {
            let mut mcp = mcp.clone();
            async move {
                if req.uri().path() != "/mcp" {
                    let body = Full::new(Bytes::from_static(b"Scriber's MCP endpoint is /mcp")).boxed();
                    return Ok(Response::builder().status(StatusCode::NOT_FOUND).body(body).unwrap());
                }
                mcp.call(req).await
            }
        });
        tokio::spawn(hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), handle));
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::model::Document;

    /// Serve a fresh headless document on `port` (0 = any); returns the port and the server task.
    pub fn spawn(port: u16) -> (u16, tokio::task::JoinHandle<()>) {
        let l = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        let ws = crate::model::Workspace::new(Document::new(300.0, 200.0));
        let task = tokio::spawn(serve(tokio::net::TcpListener::from_std(l).unwrap(), move || Server::new(ws.clone(), Arc::new(|| {}))));
        (port, task)
    }

    async fn post(port: u16, host: &str, origin: Option<&str>) -> (StatusCode, String) {
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (mut send, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.unwrap();
        tokio::spawn(conn);
        let mut req = Request::post("/mcp")
            .header("host", host)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(o) = origin {
            req = req.header("origin", o);
        }
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let resp = send.send_request(req.body(Full::new(Bytes::from(body))).unwrap()).await.unwrap();
        let status = resp.status();
        (status, String::from_utf8_lossy(&resp.into_body().collect().await.unwrap().to_bytes()).into_owned())
    }

    #[tokio::test]
    async fn rejects_foreign_host_and_origin() {
        let (port, _task) = spawn(0);
        let local = format!("127.0.0.1:{port}");
        let (ok, body) = post(port, &local, None).await;
        assert_eq!(ok, StatusCode::OK, "{body}");
        assert!(body.contains("get_document"), "{body}");
        assert_eq!(post(port, &format!("localhost:{port}"), Some("http://localhost:5173")).await.0, StatusCode::OK);
        assert_eq!(post(port, &format!("evil.example:{port}"), None).await.0, StatusCode::FORBIDDEN);
        assert_eq!(post(port, "127.0.0.1:1", None).await.0, StatusCode::FORBIDDEN);
        assert_eq!(post(port, &local, Some("https://evil.example")).await.0, StatusCode::FORBIDDEN);
    }
}
