//! MCP tools to import materials from xTool Studio and to share library files between Scriber users.

use super::{Server, ToolResult, fail, ok};
use crate::library::{self, exchange, xtool};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_router};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchXtool {
    /// Words that must all appear in the name (case-insensitive); missing = all.
    pub query: Option<String>,
    /// Only materials for this Scriber machine id (e.g. "xtool-m2").
    pub machine: Option<String>,
    /// xTool Studio's data folder, when it is not in the usual place (or set SCRIBER_XTOOL_DIR).
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportXtool {
    /// Keys from search_xtool_materials, e.g. "123@JS002".
    pub ids: Vec<String>,
    /// Only these keys' settings for this machine id; missing = any.
    pub machine: Option<String>,
    /// Replace materials imported earlier from the same xTool entry instead of adding copies.
    #[serde(default)]
    pub overwrite: bool,
    /// xTool Studio's data folder, when it is not in the usual place.
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportXs {
    /// Path to an xTool Studio project (.xs).
    pub path: PathBuf,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportLibrary {
    /// Where to write the library file (.scrlib).
    pub path: PathBuf,
    /// Machine ids; with both lists missing everything is exported.
    pub machines: Option<Vec<String>>,
    /// Material ids; the machines their settings use are added.
    pub materials: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportLibrary {
    pub path: PathBuf,
    /// For a machine or material whose id is taken by a different one.
    pub on_conflict: exchange::OnConflict,
}

fn import_all(list: &[xtool::XtoolMaterial], origin: &str, overwrite: bool) -> ToolResult {
    let mut errors = Vec::new();
    let r = library::update(|lib| {
        let mut ids = Vec::new();
        for x in list {
            match xtool::import(lib, x, origin, overwrite) {
                Ok(id) => ids.push(id),
                Err(e) => errors.push(e),
            }
        }
        Ok(ids)
    });
    match r {
        Ok(ids) if ids.is_empty() => fail(format!("nothing imported: {}", errors.join("; "))),
        Ok(ids) => ok(json!({"created": ids, "errors": errors})),
        Err(e) => fail(e),
    }
}

#[tool_router(router = xtool_router, vis = "pub(super)")]
impl Server {
    #[tool(description = "Search the materials in xTool Studio's local cache on this computer (only materials the user opened in xTool Studio's material library, or saved there from atomm.com's Materials Lab with \"Save to Studio\"; read-only, nothing is downloaded). Each: key (for import_xtool_materials), name, thickness mm, category, xTool device, Scriber machine (null = no profile yet, cannot import) and which ops each laser module has.")]
    fn search_xtool_materials(&self, Parameters(p): Parameters<SearchXtool>) -> ToolResult {
        let list = match xtool::read_cache(p.dir.as_deref()) {
            Ok(l) => l,
            Err(e) => return fail(e),
        };
        let lib = library::get();
        let words: Vec<String> = p.query.unwrap_or_default().to_lowercase().split_whitespace().map(str::to_string).collect();
        let hits: Vec<_> = list
            .iter()
            .filter(|x| words.iter().all(|w| x.name.to_lowercase().contains(w)))
            .filter(|x| p.machine.as_deref().is_none_or(|m| xtool::machine_for(&x.device) == Some(m)))
            .map(|x| json!({"key": x.key, "name": x.name, "thickness": x.thickness, "category": x.category, "device": x.device, "machine": xtool::machine_for(&x.device), "ops": xtool::ops_by_module(&lib, x)}))
            .collect();
        const MAX: usize = 60;
        ok(json!({"count": hits.len(), "materials": &hits[..hits.len().min(MAX)], "note": (hits.len() > MAX).then_some("more results: narrow the query")}))
    }

    #[tool(description = "Import materials from xTool Studio's cache into the user's library (keys from search_xtool_materials): name, thickness, category and per laser module the cut/score/engrave power, speed, passes and line interval. Image (bitmap) engraving settings are not imported. Returns the new material ids.")]
    fn import_xtool_materials(&self, Parameters(p): Parameters<ImportXtool>) -> ToolResult {
        let list = match xtool::read_cache(p.dir.as_deref()) {
            Ok(l) => l,
            Err(e) => return fail(e),
        };
        let chosen: Vec<_> = list.into_iter().filter(|x| p.ids.contains(&x.key) && p.machine.as_deref().is_none_or(|m| xtool::machine_for(&x.device) == Some(m))).collect();
        if chosen.is_empty() {
            return fail("none of those keys are in xTool Studio's cache; see search_xtool_materials");
        }
        import_all(&chosen, "cache", p.overwrite)
    }

    #[tool(description = "Import the laser settings of an xTool Studio project (.xs) as a material named after the file (rename it with save_material). Returns the new material ids.")]
    fn import_xtool_project(&self, Parameters(p): Parameters<ImportXs>) -> ToolResult {
        match xtool::read_project(&p.path) {
            Ok(list) => import_all(&list, &format!("project {}", p.path.file_name().unwrap_or_default().to_string_lossy()), p.overwrite),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Save machines and materials to a Scriber library file (.scrlib) to share with other Scriber users. Both lists missing = the whole library; the machines the materials' settings use are included.")]
    fn export_library(&self, Parameters(p): Parameters<ExportLibrary>) -> ToolResult {
        let b = match exchange::bundle(&library::get(), p.machines.as_deref(), p.materials.as_deref()) {
            Ok(b) => b,
            Err(e) => return fail(e),
        };
        match exchange::write(&b, &p.path) {
            Ok(()) => ok(json!({"path": p.path, "machines": b.machines.len(), "materials": b.materials.len()})),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Merge a Scriber library file (.scrlib) into the user's library. on_conflict (replace / keep_both / skip) decides for ids taken by a different machine or material; keep_both gives the import a new id. Reports added, replaced, skipped and machines the settings need that the user does not have.")]
    fn import_library(&self, Parameters(p): Parameters<ImportLibrary>) -> ToolResult {
        let b = match exchange::read(&p.path) {
            Ok(b) => b,
            Err(e) => return fail(e),
        };
        match library::update(|lib| Ok(exchange::import(lib, b, p.on_conflict))) {
            Ok(r) => ok(json!(r)),
            Err(e) => fail(e),
        }
    }
}
