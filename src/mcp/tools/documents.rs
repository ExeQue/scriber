//! Several open documents (the GUI's tabs): which one a call works on, and the tools that list,
//! open, create, save, close and show them, plus the recent projects.
//!
//! Every document tool takes an optional `document` id; missing = the active tab. That is done once,
//! here: `call_tool` resolves the id before the tool runs (`for_call`), and `list_tools` adds the
//! parameter to every document tool's schema (`with_document`), so the tools themselves just use
//! `self.doc`.

use super::{Server, ToolResult, fail, ok};
use crate::model::{Document, Shared};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, JsonObject, ListToolsResult, Tool};
use rmcp::schemars::{self, JsonSchema};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

/// Tools that do not work on one document, so they get no `document` parameter. Tools that
/// declare `document` themselves keep their own.
const GLOBAL: &[&str] = &[
    "list_documents",
    "open",
    "import_xs",
    "new_document",
    "list_recent_projects",
    "list_fonts",
    "list_machines",
    "list_materials",
    "save_machine",
    "save_material",
    "set_material_setting",
    "import_xtool_project",
    "export_library",
    "import_library",
    "list_library_sources",
    "subscribe_library",
    "sync_library_sources",
    "set_library_source",
    "unsubscribe_library",
    "export_library_repository",
];

const DOCUMENT_HELP: &str = "The open document to work on (id from list_documents); missing = the active tab, the one the person sees.";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenProject {
    /// Path to the .scrbr file.
    pub path: String,
    /// Show it to the person (make it the active tab). Default false: leave their view alone unless they asked.
    #[serde(default)]
    pub activate: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportXsProject {
    /// Path to the xTool Studio project (.xs).
    pub path: String,
    /// Show it to the person (make it the active tab). Default false.
    #[serde(default)]
    pub activate: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NewDocument {
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// Make it the active tab. Default false.
    #[serde(default)]
    pub activate: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveDocument {
    /// Path to the .scrbr file; missing = the document's own file.
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloseDocument {
    /// The document's id (list_documents).
    pub document: u64,
    /// Close it although it has unsaved changes (they are lost).
    #[serde(default)]
    pub discard_changes: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DocumentId {
    /// The document's id (list_documents).
    pub document: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecentProjects {
    /// Also list the .scrbr files in this folder (not its subfolders).
    pub dir: Option<String>,
}

/// File name, or Untitled.
pub(super) fn name(d: &Document) -> String {
    d.path.as_ref().and_then(|p| p.file_name()).map_or_else(|| "Untitled".into(), |f| f.to_string_lossy().into_owned())
}

fn entry(id: u64, doc: &Shared, active: u64) -> Value {
    let d = doc.lock().unwrap();
    json!({"document": id, "name": name(&d), "path": d.path, "unsaved": d.dirty, "active": id == active, "items": d.s.items.len()})
}

fn project(path: &Path, open: Option<u64>) -> Value {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs());
    let name = path.file_name().map(|f| f.to_string_lossy().into_owned());
    json!({"path": path, "name": name, "modified": modified, "exists": path.is_file(), "open_as": open})
}

impl Server {
    /// The server for one call: `document` (when given) picks the open document the tool works
    /// on, else the active tab. Unknown ids fail before the tool runs.
    pub(super) fn for_call(&self, args: Option<&JsonObject>) -> Result<Server, String> {
        let mut s = self.clone();
        match args.and_then(|a| a.get("document")).filter(|v| !v.is_null()) {
            None => (s.id, s.doc) = self.ws.active(),
            Some(v) => {
                let id = v.as_u64().ok_or("document must be a document id (see list_documents)")?;
                s.doc = self.ws.get(id).ok_or_else(|| format!("no open document {id}; see list_documents"))?;
                s.id = id;
            }
        }
        Ok(s)
    }

    pub(super) async fn call(&self, request: CallToolRequestParams, context: RequestContext<RoleServer>) -> Result<CallToolResponse, McpError> {
        match self.for_call(request.arguments.as_ref()) {
            Ok(s) => s.tool_router.call(ToolCallContext::new(&s, request, context)).await,
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(e)]).into()),
        }
    }

    pub(super) fn tools(&self) -> Vec<Tool> {
        self.tool_router.list_all().into_iter().map(with_document).collect()
    }

    pub(super) fn list(&self, context: &RequestContext<RoleServer>) -> ListToolsResult {
        let cache = context.protocol_version().is_some_and(|v| v >= rmcp::model::ProtocolVersion::V_2026_07_28);
        ListToolsResult {
            result_type: Some(rmcp::model::ResultType::COMPLETE),
            tools: self.tools(),
            meta: None,
            next_cursor: None,
            ttl_ms: cache.then_some(0),
            cache_scope: cache.then_some(rmcp::model::CacheScope::Public),
        }
    }
}

/// Add the optional `document` parameter to a document tool's schema.
pub(super) fn with_document(mut t: Tool) -> Tool {
    if GLOBAL.contains(&t.name.as_ref()) {
        return t;
    }
    let mut schema = (*t.input_schema).clone();
    let props = schema.entry("properties").or_insert_with(|| json!({}));
    if let Some(props) = props.as_object_mut() {
        props.entry("document").or_insert_with(|| json!({"type": "integer", "minimum": 0, "description": DOCUMENT_HELP}));
    }
    t.input_schema = Arc::new(schema);
    t
}

#[tool_router(router = documents_router, vis = "pub(super)")]
impl Server {
    #[tool(description = "List the open documents (the tabs in Scriber's window): document id, name, path, unsaved, active (the one the person sees) and item count. Every document tool takes document: <id>; without it, it works on the active tab.")]
    fn list_documents(&self) -> ToolResult {
        let active = self.ws.active_id();
        let docs: Vec<Value> = self.ws.docs().iter().map(|(id, d)| entry(*id, d, active)).collect();
        ok(json!({"documents": docs, "active": active}))
    }

    #[tool(description = "Open a .scrbr project in a new tab, or return the tab that already has it (already_open: true). Returns its document id: pass it as document to work on it. activate: true also shows it to the person; leave it false unless they asked to see it.")]
    fn open(&self, Parameters(p): Parameters<OpenProject>) -> ToolResult {
        let r = self.ws.open(&PathBuf::from(&p.path), p.activate);
        (self.repaint)();
        match r {
            Ok((id, existed)) => {
                let mut v = entry(id, &self.ws.get(id).unwrap(), self.ws.active_id());
                v["already_open"] = json!(existed);
                ok(v)
            }
            Err(e) => fail(format!("cannot open {}: {e}", p.path)),
        }
    }

    #[tool(description = "Open an xTool Studio project (.xs) as a new, unsaved document in a new tab: its elements (paths, lines, rectangles, circles/ellipses, polygons, stars, text as its glyph outlines, images) at their positions, one layer per xTool colour layer and processing type (cutting → cut, vector engraving → score, fill engraving → engrave, bitmap → engrave with the image; elements set not to be processed → no_output) with the project's power, speed, passes and line interval, groups (the outermost), and the machine from the device code (the material is its work area, or the drawing plus a margin). Returns the document id, per layer its id, name, op and item count, skipped elements with the reason, and notes. activate: true shows it (default false). For only the laser settings use import_xtool_project.")]
    fn import_xs(&self, Parameters(p): Parameters<ImportXsProject>) -> ToolResult {
        let lib = crate::library::get().clone();
        let r = match crate::export::xs_import::read_file(Path::new(&p.path), &lib) {
            Ok(r) => r,
            Err(e) => return fail(format!("cannot import {}: {e}", p.path)),
        };
        let (w, h, machine, laser) = (r.doc.s.width, r.doc.s.height, r.doc.s.machine.clone(), r.doc.s.laser.clone());
        let id = self.ws.add(r.doc, p.activate);
        (self.repaint)();
        let mut v = entry(id, &self.ws.get(id).unwrap(), self.ws.active_id());
        v["material"] = json!([w, h]);
        v["machine"] = json!(machine);
        v["laser"] = json!(laser);
        v["layers"] = json!(r.layers);
        v["skipped"] = json!(r.skipped);
        v["notes"] = json!(r.notes);
        ok(v)
    }

    #[tool(description = "Start a new empty document in a new tab (material size in mm, default 300 × 200). Returns its document id. activate: true shows it to the person (default false).")]
    fn new_document(&self, Parameters(p): Parameters<NewDocument>) -> ToolResult {
        let id = self.ws.add(Document::new(p.width.unwrap_or(300.0), p.height.unwrap_or(200.0)), p.activate);
        (self.repaint)();
        ok(entry(id, &self.ws.get(id).unwrap(), self.ws.active_id()))
    }

    #[tool(description = "Save a document as a .scrbr file (without path: its own file).")]
    fn save(&self, Parameters(p): Parameters<SaveDocument>) -> ToolResult {
        let id = self.id;
        self.with(|d| {
            let Some(path) = p.path.map(PathBuf::from).or_else(|| d.path.clone()) else {
                return fail("the document has no file yet; give a path");
            };
            match d.save(&path) {
                Ok(()) => {
                    self.ws.remember(&path);
                    ok(json!({"document": id, "path": path}))
                }
                Err(e) => fail(e),
            }
        })
    }

    #[tool(description = "Close a document's tab. Refuses when it has unsaved changes unless discard_changes is true (ask the person first). Closing the last tab leaves a new empty document.")]
    fn close_document(&self, Parameters(p): Parameters<CloseDocument>) -> ToolResult {
        let Some(doc) = self.ws.get(p.document) else { return fail(format!("no open document {}; see list_documents", p.document)) };
        if doc.lock().unwrap().dirty && !p.discard_changes {
            return fail("the document has unsaved changes; save it first, or pass discard_changes: true to lose them");
        }
        self.ws.close(p.document);
        (self.repaint)();
        ok(json!({"closed": p.document, "active": self.ws.active_id()}))
    }

    #[tool(description = "Show a document to the person (make it the active tab). Only when they asked to see it: switching tabs changes what they are looking at.")]
    fn activate_document(&self, Parameters(p): Parameters<DocumentId>) -> ToolResult {
        if !self.ws.activate(p.document) {
            return fail(format!("no open document {}; see list_documents", p.document));
        }
        (self.repaint)();
        ok(json!({"active": p.document}))
    }

    #[tool(description = "The projects opened or saved recently in Scriber, newest first (path, name, modified as Unix seconds, exists, open_as = the document id when open now), and with dir also the .scrbr files in that folder. Open one with open.")]
    fn list_recent_projects(&self, Parameters(p): Parameters<RecentProjects>) -> ToolResult {
        let recent: Vec<Value> = self.ws.recent().iter().map(|f| project(f, self.ws.find(f))).collect();
        let mut out = json!({"recent": recent});
        if let Some(dir) = p.dir {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) => return fail(format!("cannot read {dir}: {e}")),
            };
            let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|f| f.is_file() && f.extension().is_some_and(|x| x.eq_ignore_ascii_case("scrbr"))).collect();
            files.sort();
            out["in_dir"] = json!(files.iter().map(|f| project(f, self.ws.find(f))).collect::<Vec<_>>());
        }
        ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Workspace;
    use crate::model::annotations::Author;

    fn args<T: serde::de::DeserializeOwned>(v: Value) -> Parameters<T> {
        Parameters(serde_json::from_value(v).unwrap())
    }

    fn reply(r: ToolResult) -> Value {
        let r = r.unwrap();
        let t = r.content[0].as_text().unwrap().text.clone();
        assert_ne!(r.is_error, Some(true), "{t}");
        serde_json::from_str(&t).unwrap()
    }

    fn is_error(r: ToolResult) -> String {
        let r = r.unwrap();
        assert_eq!(r.is_error, Some(true));
        r.content[0].as_text().unwrap().text.clone()
    }

    /// A server with two documents; the first is active.
    fn two() -> (Server, u64, u64) {
        let ws = Workspace::new(Document::new(100.0, 100.0));
        let a = ws.active_id();
        let s = Server::new(ws, Arc::new(|| {}));
        let b = reply(s.new_document(args(json!({}))))["document"].as_u64().unwrap();
        (s, a, b)
    }

    /// The server as `call_tool` sets it up for these arguments.
    fn on(s: &Server, v: Value) -> Server {
        s.for_call(v.as_object()).unwrap()
    }

    fn rect() -> Value {
        json!({"items": [{"shape": {"kind": "rect", "x": 1, "y": 1, "w": 5, "h": 5}}]})
    }

    fn count(s: &Server, id: u64) -> usize {
        s.ws.get(id).unwrap().lock().unwrap().s.items.len()
    }

    #[test]
    fn document_picks_the_document_and_the_default_is_the_active_tab() {
        let (s, a, b) = two();
        assert_eq!(s.ws.active_id(), a, "new_document leaves the person's tab alone");
        reply(on(&s, json!({"document": b})).add_items(args(rect())));
        assert_eq!((count(&s, a), count(&s, b)), (0, 1));
        reply(on(&s, json!({})).add_items(args(rect())));
        assert_eq!((count(&s, a), count(&s, b)), (1, 1));
        reply(on(&s, json!({"document": b})).activate_document(args(json!({"document": b}))));
        reply(on(&s, json!({})).add_items(args(rect())));
        assert_eq!((count(&s, a), count(&s, b)), (1, 2), "the default follows the active tab");
        assert!(s.for_call(json!({"document": 99}).as_object()).err().unwrap().contains("no open document 99"));

        let listed = reply(s.list_documents());
        assert_eq!(listed["active"], b);
        assert_eq!(listed["documents"][0]["items"], 1);
        assert_eq!(listed["documents"][1]["unsaved"], true);
    }

    #[test]
    fn the_selection_is_kept_per_document() {
        let (s, a, b) = two();
        let first = reply(on(&s, json!({"document": a})).add_items(args(rect())))["ids"][0].as_u64().unwrap();
        s.ws.get(a).unwrap().lock().unwrap().selection = vec![first];
        reply(on(&s, json!({"document": a})).add_items(args(rect())));
        reply(on(&s, json!({"document": b})).add_items(args(json!({"items": [{"shape": {"kind": "circle", "cx": 5, "cy": 5, "r": 2}}], "select": true}))));
        assert_eq!(s.ws.get(a).unwrap().lock().unwrap().selection, vec![first], "A's selection stays");
        assert_eq!(s.ws.get(b).unwrap().lock().unwrap().selection.len(), 1, "B selected on purpose");
    }

    #[test]
    fn open_save_close_and_recent_projects() {
        let dir = std::env::temp_dir().join(format!("scriber-mcp-docs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("lid.scrbr");
        let (s, a, b) = two();
        reply(on(&s, json!({"document": b})).add_items(args(rect())));
        assert!(is_error(s.close_document(args(json!({"document": b})))).contains("unsaved"));
        let saved = reply(on(&s, json!({"document": b})).save(args(json!({"path": file}))));
        assert_eq!(saved["document"], b);

        // Open again: the same tab; not activated unless asked.
        let opened = reply(s.open(args(json!({"path": file}))));
        assert_eq!((opened["document"].as_u64(), opened["already_open"].as_bool(), s.ws.active_id()), (Some(b), Some(true), a));

        let recent = reply(s.list_recent_projects(args(json!({"dir": dir}))));
        assert_eq!(recent["recent"][0]["name"], "lid.scrbr");
        assert_eq!((recent["recent"][0]["exists"].as_bool(), recent["recent"][0]["open_as"].as_u64()), (Some(true), Some(b)));
        assert_eq!(recent["in_dir"].as_array().unwrap().len(), 1);

        reply(s.close_document(args(json!({"document": b}))));
        assert!(s.ws.get(b).is_none());
        let reopened = reply(s.open(args(json!({"path": file, "activate": true}))));
        let c = reopened["document"].as_u64().unwrap();
        assert_eq!((reopened["already_open"].as_bool(), reopened["items"].as_u64(), s.ws.active_id()), (Some(false), Some(1), c));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn document_is_in_every_document_tools_schema() {
        let (s, _, _) = two();
        let tools = s.tools();
        let has = |name: &str| tools.iter().find(|t| t.name == name).unwrap().input_schema["properties"].get("document").cloned();
        assert!(has("add_items").is_some() && has("get_document").is_some() && has("undo").is_some());
        assert!(has("list_machines").is_none() && has("open").is_none());
        assert_eq!(has("close_document").unwrap()["type"], "integer");
        assert!(has("wait_for_annotations").unwrap()["description"].as_str().unwrap().contains("every open document"));
    }

    #[tokio::test]
    async fn wait_for_annotations_watches_every_document() {
        let (s, a, b) = two();
        let since = crate::model::annotations::revision();
        s.ws.get(b).unwrap().lock().unwrap().annotate(Author::Person, "in the other tab", Some([1.0, 1.0]), vec![]).unwrap();
        let r = reply(s.wait_for_annotations(args(json!({"since_revision": since, "timeout_s": 2}))).await);
        assert_eq!((r["annotations"][0]["document"].as_u64(), r["annotations"][0]["text"].as_str()), (Some(b), Some("in the other tab")));
        let only_a = reply(s.wait_for_annotations(args(json!({"since_revision": since, "timeout_s": 0.1, "document": a}))).await);
        assert_eq!(only_a["timed_out"], true);
    }
}
