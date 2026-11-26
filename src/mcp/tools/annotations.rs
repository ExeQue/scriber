//! MCP tools for review comments (see `model::annotations`): list, add, reply, resolve, and
//! `wait_for_annotations`, the long-poll that lets an agent react when the person comments.
//!
//! Why a long-poll and not an MCP resource subscription (`resources/subscribe` with
//! `notifications/resources/updated`): the HTTP endpoint is stateless (no sessions, JSON
//! responses), so the server has no channel to push a notification on, and `scriber mcp` relays
//! stdio to that same endpoint. A tool call that blocks until something changes works with every
//! client and survives editor restarts.

use super::{Server, ToolResult, fail, ok};
use crate::model::annotations::{self, Annotation, Author, Status};
use crate::model::{Document, Point};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListAnnotations {
    /// Only open or only resolved ones.
    pub status: Option<Status>,
    /// Only those changed after this revision (from an earlier reply).
    pub since_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddAnnotation {
    pub text: String,
    /// Where the pin goes (mm); default the centre of items.
    pub at: Option<Point>,
    /// Item ids it is about.
    #[serde(default)]
    pub items: Vec<u64>,
    /// Your name, shown on the comment (default "agent").
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReplyAnnotation {
    pub id: u64,
    pub text: String,
    /// true resolves it with the reply, false reopens it.
    pub resolve: Option<bool>,
    /// Your name (default "agent").
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnnotationId {
    pub id: u64,
    /// Your name (default "agent").
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WaitForAnnotations {
    /// The revision from the last list_annotations / wait_for_annotations reply; missing = now.
    pub since_revision: Option<u64>,
    /// Seconds to wait (default 60, at most 300).
    pub timeout_s: Option<f64>,
}

fn agent(name: Option<String>) -> Author {
    Author::Agent(name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "agent".into()))
}

/// One annotation as agents see it: the shapes it names that still exist, the last reply.
fn compact(d: &Document, a: &Annotation) -> Value {
    let items: Vec<u64> = a.items.iter().copied().filter(|id| d.item(*id).is_some()).collect();
    let mut v = json!({"id": a.id, "text": a.text, "author": a.author, "status": a.status, "at": a.at, "items": items, "replies": a.replies.len(), "revision": a.revision});
    if let Some(r) = a.replies.last() {
        v["last_reply"] = json!({"author": r.author, "text": r.text});
    }
    v
}

fn list(d: &Document, keep: impl Fn(&Annotation) -> bool) -> Vec<Value> {
    d.annotations.list.iter().filter(|a| keep(a)).map(|a| compact(d, a)).collect()
}

/// The person's comments, replies and status changes after `since`.
fn from_person(d: &Document, since: u64) -> Vec<Value> {
    list(d, |a| a.revision > since && a.changed_by == Author::Person)
}

#[tool_router(router = annotations_router, vis = "pub(super)")]
impl Server {
    #[tool(description = "List the review comments pinned on the canvas (id = the number on its pin, text, author, status open/resolved, at, items it points at, reply count, last_reply, revision). Filter by status and/or since_revision. Returns them and the current revision.")]
    fn list_annotations(&self, Parameters(p): Parameters<ListAnnotations>) -> ToolResult {
        let d = self.doc.lock().unwrap();
        let since = p.since_revision.unwrap_or(0);
        let found = list(&d, |a| a.revision > since && p.status.is_none_or(|s| a.status == s));
        ok(json!({"revision": annotations::revision(), "annotations": found}))
    }

    #[tool(description = "Pin a comment on the canvas for the person (at a point, or on items). Not lasered or exported. Returns its id.")]
    fn add_annotation(&self, Parameters(p): Parameters<AddAnnotation>) -> ToolResult {
        self.with(|d| match d.annotate(agent(p.author), &p.text, p.at, p.items) {
            Ok(id) => ok(json!({"id": id, "revision": annotations::revision()})),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Reply to a comment; resolve: true also resolves it (say what you changed), false reopens it.")]
    fn reply_annotation(&self, Parameters(p): Parameters<ReplyAnnotation>) -> ToolResult {
        self.with(|d| match d.reply_annotation(p.id, agent(p.author), &p.text, p.resolve) {
            Ok(()) => ok(json!({"id": p.id, "revision": annotations::revision()})),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Mark a comment resolved (prefer reply_annotation with resolve: true, so the person sees what was done).")]
    fn resolve_annotation(&self, Parameters(p): Parameters<AnnotationId>) -> ToolResult {
        self.with(|d| match d.set_annotation_status(p.id, agent(p.author), Status::Resolved) {
            Ok(()) => ok(json!({"id": p.id, "revision": annotations::revision()})),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Wait until the person adds, replies to, resolves or reopens a comment after since_revision (missing = now), or timeout_s passes (default 60, max 300). Returns at once when there already is one. Reply: annotations (the changed ones, as list_annotations) and revision; pass that revision to the next call. timed_out: true when nothing came. Loop on it to follow the person's review.")]
    async fn wait_for_annotations(&self, Parameters(p): Parameters<WaitForAnnotations>) -> ToolResult {
        // Subscribe before looking, so a change between the look and the wait still wakes us.
        let mut changes = annotations::subscribe();
        let since = p.since_revision.unwrap_or_else(annotations::revision);
        let timeout = Duration::from_secs_f64(p.timeout_s.unwrap_or(60.0).clamp(0.0, 300.0));
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let found = from_person(&self.doc.lock().unwrap(), since);
            if !found.is_empty() {
                return ok(json!({"revision": annotations::revision(), "annotations": found}));
            }
            // Any document's change wakes us; the look above decides whether it is ours.
            if tokio::time::timeout_at(deadline, changes.changed()).await.is_err() {
                return ok(json!({"revision": annotations::revision().max(since), "annotations": [], "timed_out": true}));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn server() -> (Server, crate::model::Shared) {
        let doc: crate::model::Shared = Arc::new(Mutex::new(Document::new(100.0, 100.0)));
        (Server::new(doc.clone(), Arc::new(|| {})), doc)
    }

    fn args<T: serde::de::DeserializeOwned>(v: Value) -> Parameters<T> {
        Parameters(serde_json::from_value(v).unwrap())
    }

    fn reply(r: ToolResult) -> Value {
        let r = r.unwrap();
        let t = r.content[0].as_text().unwrap().text.clone();
        assert_ne!(r.is_error, Some(true), "{t}");
        serde_json::from_str(&t).unwrap()
    }

    #[tokio::test]
    async fn wait_returns_at_once_when_something_is_newer() {
        let (s, doc) = server();
        let since = annotations::revision();
        doc.lock().unwrap().annotate(Author::Person, "bigger", Some([1.0, 2.0]), vec![]).unwrap();
        let t = std::time::Instant::now();
        let r = reply(s.wait_for_annotations(args(json!({"since_revision": since, "timeout_s": 5}))).await);
        assert!(t.elapsed() < Duration::from_secs(1));
        assert_eq!(r["annotations"][0]["text"], "bigger");
        let rev = r["revision"].as_u64().unwrap();

        // The agent's own reply does not wake it.
        reply(s.reply_annotation(args(json!({"id": 1, "text": "done", "resolve": true}))));
        let r = reply(s.wait_for_annotations(args(json!({"since_revision": rev, "timeout_s": 0.2}))).await);
        assert_eq!((r["timed_out"].as_bool(), r["annotations"].as_array().unwrap().len()), (Some(true), 0));
        let listed = reply(s.list_annotations(args(json!({"status": "resolved"}))));
        assert_eq!(listed["annotations"][0]["last_reply"]["text"], "done");
        assert_eq!(listed["annotations"][0]["last_reply"]["author"]["agent"], "agent");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wait_wakes_when_the_person_comments() {
        let (s, doc) = server();
        let since = annotations::revision();
        let person = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            doc.lock().unwrap().annotate(Author::Person, "move it left", Some([3.0, 4.0]), vec![]).unwrap();
        });
        let t = std::time::Instant::now();
        let r = reply(s.wait_for_annotations(args(json!({"since_revision": since, "timeout_s": 10}))).await);
        person.join().unwrap();
        assert!(t.elapsed() < Duration::from_secs(5), "woke early, not at the timeout");
        assert_eq!(r["annotations"][0]["text"], "move it left");
        assert!(r.get("timed_out").is_none());
    }
}
