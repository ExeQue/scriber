//! MCP tools for library sources: machine and material libraries published in GitHub repositories.

use super::{Server, ToolResult, fail, ok};
use crate::library::sources::{self, Net, Track};
use crate::library::{self, Origin};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Subscribe {
    /// owner/repo, owner/repo@ref (a tag when the repository has one by that name, else a branch) or https://github.com/owner/repo[/tree/ref].
    pub url: String,
    /// What to follow: {"branch": "main"} ("" = the default branch), {"tag": "v1.2"}, {"release": "latest"} or {"commit": "<sha>"}; missing = from the url, else the default branch.
    pub track: Option<Track>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SourceId {
    /// Source id from list_library_sources.
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Sync {
    /// Source id; missing = every enabled source.
    pub id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetSource {
    pub id: String,
    /// Follow something else (syncs right away): {"branch": ""}, {"tag": "v1.2"}, {"release": "latest"} or {"commit": "<sha>"}.
    pub track: Option<Track>,
    /// Use the source's machines and materials (false hides them without unsubscribing).
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportRepository {
    /// The repository's folder (its root): a new folder, or an existing (git) repository to update in place.
    pub dir: PathBuf,
    /// Machine ids of the user's own; both lists missing = all the user's own machines and materials.
    pub machines: Option<Vec<String>>,
    /// Material ids of the user's own.
    pub materials: Option<Vec<String>>,
    /// Only for a new repository (no .scriber.json yet): its name (default: the folder name) and description.
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Network work off the async runtime's threads, then the library re-reads the sources.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let out = tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())?;
    library::reload_sources();
    out
}

/// How many of the list's items come from this source.
fn count<T>(list: &[(Origin, T)], id: &str) -> usize {
    list.iter().filter(|(o, _)| matches!(o, Origin::Source { id: s, .. } if s == id)).count()
}

fn reply<T: serde::Serialize>(r: Result<T, String>) -> ToolResult {
    match r {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}

#[tool_router(router = sources_router, vis = "pub(super)")]
impl Server {
    #[tool(description = "List the library sources (GitHub repositories the user subscribed to for machines and materials): id, url, track (what it follows), commit in use, release, name, description, author, license, enabled, how many machines and materials it gives, machines its materials need that nobody has (settings kept), last sync summary and error, and problems (skipped files, id clashes: the first source in the list wins).")]
    fn list_library_sources(&self) -> ToolResult {
        let dir = sources::config_dir();
        let lib = library::get();
        let list: Vec<Value> = sources::list(&dir)
            .into_iter()
            .map(|s| {
                let manifest = sources::read_manifest(&s.folder(&dir)).ok();
                json!({
                    "id": s.id, "url": s.url, "track": s.track, "commit": s.pinned_commit, "release": s.release, "name": s.name,
                    "description": manifest.as_ref().map(|m| &m.description), "author": manifest.as_ref().map(|m| &m.author),
                    "license": manifest.as_ref().map(|m| &m.license), "homepage": manifest.as_ref().and_then(|m| m.homepage.as_ref()),
                    "library_version": manifest.as_ref().and_then(|m| m.library_version.as_ref()),
                    "enabled": s.enabled, "machines": count(&lib.base.machines, &s.id), "materials": count(&lib.base.materials, &s.id),
                    "missing_machines": sources::missing_machines(&lib, &s.id),
                    "last_checked": s.last_checked, "last_sync": s.last_sync, "last_error": s.last_error,
                })
            })
            .collect();
        ok(json!({"sources": list, "problems": lib.base.problems}))
    }

    #[tool(description = "Subscribe to a machine and material library published in a public GitHub repository (.scriber.json at its root, files in .scriber/machines and .scriber/materials). Downloads it now; its machines and materials appear in the library, read-only (save_machine/save_material with the same id makes the user's own copy, which wins). Returns the source and what it added.")]
    pub(super) async fn subscribe_library(&self, Parameters(p): Parameters<Subscribe>) -> ToolResult {
        reply(blocking(move || sources::subscribe(&sources::config_dir(), &Net, &p.url, p.track).map(|(s, diff)| json!({"source": s, "summary": diff.summary(), "diff": diff}))).await)
    }

    #[tool(description = "Sync library sources now: resolve what each follows to its current commit (one small request) and, when it changed, download and use it (a broken new version is not used; the old one stays). Reply per source: changed, commit, summary (e.g. \"+2 materials, 1 changed, 0 removed\") and the diff (ids added, changed, removed).")]
    pub(super) async fn sync_library_sources(&self, Parameters(p): Parameters<Sync>) -> ToolResult {
        let out = blocking(move || {
            let dir = sources::config_dir();
            let ids: Vec<String> = match p.id {
                Some(id) => vec![id],
                None => sources::list(&dir).into_iter().filter(|s| s.enabled).map(|s| s.id).collect(),
            };
            Ok(ids.iter().map(|id| sources::sync(&dir, &Net, id, true).map_or_else(|e| json!({"id": id, "error": e}), |r| json!(r))).collect::<Vec<_>>())
        })
        .await;
        reply(out)
    }

    #[tool(description = "Change a library source: track (what it follows; syncs right away: {\"branch\": \"\"} = default branch, {\"tag\": \"v1.2\"} locked to a tag, {\"release\": \"latest\"} = GitHub's latest release, {\"commit\": \"<sha>\"} fully locked) and/or enabled.")]
    pub(super) async fn set_library_source(&self, Parameters(p): Parameters<SetSource>) -> ToolResult {
        let out = blocking(move || {
            let dir = sources::config_dir();
            if let Some(on) = p.enabled {
                sources::set_enabled(&dir, &p.id, on)?;
            }
            match p.track {
                Some(t) => sources::set_track(&dir, &Net, &p.id, t).map(|r| json!(r)),
                None => Ok(json!({"id": p.id, "enabled": p.enabled})),
            }
        })
        .await;
        reply(out)
    }

    #[tool(description = "Unsubscribe from a library source: its machines and materials leave the library and its downloaded files are deleted. Confirm with the person first.")]
    fn unsubscribe_library(&self, Parameters(p): Parameters<SourceId>) -> ToolResult {
        let r = sources::unsubscribe(&sources::config_dir(), &p.id);
        library::reload_sources();
        reply(r.map(|_| json!({"removed": p.id})))
    }

    #[tool(description = "Write the user's own machines and materials (ones they made: never built-ins, sources' items or edits of those) as a library repository so the person can publish it on GitHub for others to subscribe to. dir is a new folder, or an existing (git) repository that is updated in place: only .scriber.json and .scriber/machines|materials/<id>.json are written, files there for items the user no longer has are removed, everything else (.git, README, …) stays; an existing .scriber.json keeps its name, description, author, license and homepage (library_version, when present, is bumped). name/description are used only for a new repository. Scriber does no git: the person commits and pushes. Reply: created (or updated), added, changed and removed files.")]
    fn export_library_repository(&self, Parameters(p): Parameters<ExportRepository>) -> ToolResult {
        let r = sources::export_repository(&library::get(), &p.dir, p.machines.as_deref(), p.materials.as_deref(), p.name.as_deref(), p.description.as_deref());
        reply(r.map(|e| json!({"dir": p.dir, "summary": e.summary(), "result": e})))
    }
}
