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
    /// Only materials that can be imported for this Scriber machine id (e.g. "xtool-m2").
    pub machine: Option<String>,
    /// Also list materials that only have settings for xTool machines not in the user's library.
    #[serde(default)]
    pub include_other_machines: bool,
    /// xTool Studio's data folder, when it is not in the usual place (or set SCRIBER_XTOOL_DIR).
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportXtool {
    /// Material ids from search_xtool_materials, e.g. "123".
    pub ids: Vec<String>,
    /// The Scriber machine to import the settings for; missing = the document's machine if it has
    /// settings for it, else the first machine that has.
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
    /// Where to write the library file: .scrlib, or .zip for a library zip (your own items only).
    pub path: PathBuf,
    /// Machine ids; with both lists missing everything is exported.
    pub machines: Option<Vec<String>>,
    /// Material ids; the machines their settings use are added.
    pub materials: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportLibrary {
    /// A .scrlib file or a library zip (.zip).
    pub path: PathBuf,
    /// For a machine or material whose id is taken by a different one.
    pub on_conflict: exchange::OnConflict,
}

fn import_all(list: &[xtool::XtoolMaterial], origin: &str, overwrite: bool, mut errors: Vec<String>) -> ToolResult {
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
    #[tool(description = "Search the materials in xTool Studio's local cache on this computer (only materials the user opened in xTool Studio's material library, or saved there from atomm.com's Materials Lab with \"Save to Studio\"; read-only, nothing is downloaded). Each material once: id (for import_xtool_materials), name, thickness mm, category, and per Scriber machine in the user's library it can be imported for (the document's first) the settings per laser module and op. By default only importable materials are listed; with include_other_machines the rest come with why_not.")]
    fn search_xtool_materials(&self, Parameters(p): Parameters<SearchXtool>) -> ToolResult {
        let list = match xtool::read_cache(p.dir.as_deref()) {
            Ok(l) => l,
            Err(e) => return fail(e),
        };
        let first = self.doc.lock().s.machine.clone();
        let lib = library::get();
        let words: Vec<String> = p.query.unwrap_or_default().to_lowercase().split_whitespace().map(str::to_string).collect();
        let hits: Vec<_> = xtool::group(list)
            .iter()
            .filter(|g| words.iter().all(|w| g.name.to_lowercase().contains(w)))
            .filter_map(|g| {
                let targets: Vec<_> = g.targets(&lib, first.as_deref()).into_iter().filter(|(m, _)| p.machine.as_deref().is_none_or(|pm| pm == *m)).collect();
                let mut entry = json!({"id": g.id, "name": g.name, "thickness": g.thickness, "category": g.category});
                if targets.is_empty() {
                    if !p.include_other_machines || p.machine.is_some() {
                        return None;
                    }
                    entry["why_not"] = json!(g.reason(&lib));
                } else {
                    entry["machines"] = targets.iter().map(|(m, x)| (m.to_string(), serde_json::Value::Object(xtool::settings_by_module(&lib, m, x)))).collect::<serde_json::Map<_, _>>().into();
                }
                Some(entry)
            })
            .collect();
        const MAX: usize = 60;
        ok(json!({"count": hits.len(), "materials": &hits[..hits.len().min(MAX)], "note": (hits.len() > MAX).then_some("more results: narrow the query")}))
    }

    #[tool(description = "Import materials from xTool Studio's cache into the user's library (ids from search_xtool_materials) for one Scriber machine: name, thickness, category and for every laser module of that machine the cut/score/engrave power, speed, passes and line interval. Image (bitmap) engraving settings are not imported. Returns the new material ids.")]
    fn import_xtool_materials(&self, Parameters(p): Parameters<ImportXtool>) -> ToolResult {
        let list = match xtool::read_cache(p.dir.as_deref()) {
            Ok(l) => l,
            Err(e) => return fail(e),
        };
        let first = p.machine.clone().or_else(|| self.doc.lock().s.machine.clone());
        let groups = xtool::group(list);
        let (mut chosen, mut errors) = (Vec::new(), Vec::new());
        {
            let lib = library::get();
            for id in &p.ids {
                // An old "<id>@<device>" key works too.
                let (material, device) = id.split_once('@').map_or((id.as_str(), None), |(m, d)| (m, Some(d)));
                let Some(g) = groups.iter().find(|g| g.id == material) else {
                    errors.push(format!("{id}: not in xTool Studio's cache; see search_xtool_materials"));
                    continue;
                };
                let targets = g.targets(&lib, first.as_deref());
                match targets.iter().find(|(m, x)| p.machine.as_deref().is_none_or(|pm| pm == *m) && device.is_none_or(|d| d == x.device)) {
                    Some((_, x)) => chosen.push((*x).clone()),
                    None if targets.is_empty() => errors.push(format!("{}: {}", g.name, g.reason(&lib))),
                    None => errors.push(format!("{}: no settings for {}", g.name, p.machine.as_deref().unwrap_or(id))),
                }
            }
        }
        import_all(&chosen, "cache", p.overwrite, errors)
    }

    #[tool(description = "Import the laser settings of an xTool Studio project (.xs) as a material named after the file (rename it with save_material). Returns the new material ids.")]
    fn import_xtool_project(&self, Parameters(p): Parameters<ImportXs>) -> ToolResult {
        match xtool::read_project(&p.path) {
            Ok(list) => import_all(&list, &format!("project {}", p.path.file_name().unwrap_or_default().to_string_lossy()), p.overwrite, Vec::new()),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Save machines and materials to a Scriber library file to share with other Scriber users. A path ending in .scrlib: one JSON file; both lists missing = the whole library, and the machines the materials' settings use are included. A path ending in .zip: a library zip (.scriber.json and .scriber/machines|materials/<id>.json, the layout of a library repository) with only the user's own machines and materials (never built-ins or sources' items; both lists missing = all of them).")]
    fn export_library(&self, Parameters(p): Parameters<ExportLibrary>) -> ToolResult {
        if p.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
            return match library::sources::export_zip(&library::get(), &p.path, p.machines.as_deref(), p.materials.as_deref()) {
                Ok((machines, materials)) => ok(json!({"path": p.path, "machines": machines, "materials": materials})),
                Err(e) => fail(e),
            };
        }
        let b = match exchange::bundle(&library::get(), p.machines.as_deref(), p.materials.as_deref()) {
            Ok(b) => b,
            Err(e) => return fail(e),
        };
        match exchange::write(&b, &p.path) {
            Ok(()) => ok(json!({"path": p.path, "machines": b.machines.len(), "materials": b.materials.len()})),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Merge a Scriber library file (.scrlib, or a library zip .zip with .scriber.json and .scriber/machines|materials/ at its root or in one top folder; invalid files in it are skipped and reported) into the user's library. on_conflict (replace / keep_both / skip) decides for ids taken by a different machine or material; keep_both gives the import a new id. Reports added, replaced, skipped and machines the settings need that the user does not have.")]
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
