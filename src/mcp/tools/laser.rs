//! MCP tools for machines, the material library and the document's laser setup.

use super::{Server, ToolResult, fail, ok};
use crate::library::{self, Category, Machine, Material, Operation, Setting};
use crate::model::State;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListMaterials {
    /// Only materials with settings for this machine id (and only those settings).
    pub machine: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LaserSetup {
    /// Machine id; "" = none; missing = unchanged.
    pub machine: Option<String>,
    /// Laser module id of the machine; missing = unchanged, or the machine's first module when the machine changes.
    pub laser: Option<String>,
    /// Material id; "" = none; missing = unchanged.
    pub material: Option<String>,
    /// Also set the material (canvas) size to the machine's work area.
    #[serde(default)]
    pub use_work_area: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveMachine {
    /// The whole machine; replaces the one with the same id (built-ins too: the user's copy wins).
    pub machine: Machine,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveMaterial {
    /// Existing material to change; missing = add a new one (name and thickness needed).
    pub id: Option<String>,
    pub name: Option<String>,
    /// mm
    pub thickness: Option<f64>,
    /// What kind of material (for filtering); a new material without one is "other".
    pub category: Option<Category>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetMaterialSetting {
    /// Material id.
    pub material: String,
    /// Replaces the setting for the same machine, laser and operation.
    #[serde(flatten)]
    pub setting: Setting,
}

/// The settings that apply to the document, per operation.
fn setup(s: &State) -> Value {
    let lib = library::get();
    let machine = s.machine.as_deref().and_then(|id| lib.machine(id));
    let settings: serde_json::Map<String, Value> = Operation::ALL
        .iter()
        .map(|op| {
            let setting = match (&s.machine, &s.material) {
                (Some(m), Some(mat)) => lib.setting(m, s.laser.as_deref(), mat, *op),
                _ => None,
            };
            (serde_json::to_value(op).unwrap().as_str().unwrap().to_string(), json!(setting))
        })
        .collect();
    json!({
        "machine": machine, "laser": s.laser,
        "material": s.material.as_deref().and_then(|id| lib.material(id)).map(|m| json!({"id": m.id, "name": m.name, "thickness": m.thickness, "notes": m.notes})),
        "settings": settings,
        "layers": s.layers.iter().map(|l| json!({
            "id": l.id, "name": l.name, "op": l.op, "visible": l.visible,
            "setting": crate::model::layer::op_operation(l.op).and_then(|op| l.setting(s, &lib, op)),
        })).collect::<Vec<_>>(),
        "material_size": [s.width, s.height],
        "work_area": machine.map(|m| m.area(s.laser.as_deref())),
    })
}

#[tool_router(router = laser_router, vis = "pub(super)")]
impl Server {
    #[tool(description = "List the machines (laser cutters) the user has: work area in mm, laser modules with type, power and what each can do, notes and the source of the numbers.")]
    fn list_machines(&self) -> ToolResult {
        ok(json!(library::get().machines))
    }

    #[tool(description = "List the material library: each material with its thickness and laser settings per machine, laser module and operation (cut, score, engrave): power %, speed mm/s, passes, and for engrave line_interval (mm between lines; tell people lines per cm = 10 / line_interval) and bidirectional.")]
    fn list_materials(&self, Parameters(p): Parameters<ListMaterials>) -> ToolResult {
        let lib = library::get();
        let Some(machine) = p.machine else { return ok(json!(lib.materials)) };
        let list: Vec<Material> = lib
            .materials
            .iter()
            .filter_map(|m| {
                let settings: Vec<Setting> = m.settings.iter().filter(|s| s.machine == machine).cloned().collect();
                (!settings.is_empty()).then(|| Material { settings, ..m.clone() })
            })
            .collect();
        ok(json!(list))
    }

    #[tool(description = "Pre-flight check before lasering: open shapes set to cut, lines on top of each other, shapes outside the material or the machine's work area, details smaller than the beam, text too small to read, and operations without settings for the chosen machine and material. Each warning has a severity (error/warning), a plain text and the item ids involved.")]
    fn preflight_check(&self) -> ToolResult {
        let warnings = crate::model::preflight::check(&self.doc.lock().unwrap().s, &library::get());
        ok(json!({"count": warnings.len(), "warnings": warnings}))
    }

    #[tool(description = "The document's machine, laser module and material, the material library's settings per operation (null where the library has none), and per layer the setting the laser uses: the preset for the layer's op with the layer's own power/speed/passes on top (null when neither gives them).")]
    fn get_laser_setup(&self) -> ToolResult {
        ok(setup(&self.doc.lock().unwrap().s))
    }

    #[tool(description = "Choose the document's machine, laser module and/or material (ids from list_machines/list_materials). use_work_area also sets the material size to the machine's work area. One undo step.")]
    fn set_laser_setup(&self, Parameters(p): Parameters<LaserSetup>) -> ToolResult {
        let lib = library::get().clone();
        self.with(|d| {
            let mut s = d.s.clone();
            if let Some(m) = p.machine {
                s.laser = None;
                s.machine = (!m.is_empty()).then_some(m);
            }
            let machine = match s.machine.as_deref() {
                Some(id) => match lib.machine(id) {
                    Some(m) => Some(m),
                    None => return fail(format!("no machine with id {id}; see list_machines")),
                },
                None => None,
            };
            if let Some(l) = p.laser {
                s.laser = (!l.is_empty()).then_some(l);
            }
            if let Some(m) = machine {
                match &s.laser {
                    Some(l) if m.laser(l).is_none() => return fail(format!("{} has no laser module {l}", m.name)),
                    None => s.laser = m.lasers.first().map(|l| l.id.clone()),
                    _ => {}
                }
            }
            if let Some(m) = p.material {
                s.material = (!m.is_empty()).then_some(m);
            }
            if let Some(id) = s.material.as_deref().filter(|id| lib.material(id).is_none()) {
                return fail(format!("no material with id {id}; see list_materials"));
            }
            if p.use_work_area {
                let Some(m) = machine else { return fail("use_work_area needs a machine") };
                [s.width, s.height] = m.area(s.laser.as_deref());
            }
            d.checkpoint();
            d.s = s;
            ok(setup(&d.s))
        })
    }

    #[tool(description = "Add a machine or replace the one with the same id. Saved in the user's library.")]
    fn save_machine(&self, Parameters(p): Parameters<SaveMachine>) -> ToolResult {
        let m = p.machine;
        match library::update(|lib| {
            lib.put_machine(m.clone());
            Ok(())
        }) {
            Ok(()) => ok(json!(m)),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Add a material (no id: name and thickness needed) or change an existing one's name, thickness or notes. Returns it with its id.")]
    fn save_material(&self, Parameters(p): Parameters<SaveMaterial>) -> ToolResult {
        let r = library::update(|lib| {
            let mut m = match &p.id {
                Some(id) => lib.material(id).cloned().ok_or(format!("no material with id {id}"))?,
                None => {
                    let name = p.name.clone().ok_or("a new material needs a name")?;
                    let thickness = p.thickness.ok_or("a new material needs a thickness")?;
                    Material { id: lib.new_id(&name), name, category: Category::Other, thickness, notes: String::new(), source: String::new(), settings: Vec::new() }
                }
            };
            m.name = p.name.unwrap_or(m.name);
            m.thickness = p.thickness.unwrap_or(m.thickness);
            m.category = p.category.unwrap_or(m.category);
            m.notes = p.notes.unwrap_or(m.notes);
            lib.put_material(m.clone());
            Ok(m)
        });
        match r {
            Ok(m) => ok(json!(m)),
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Add or replace a material's setting for one machine, laser module and operation (cut, score, engrave; the old \"fill\" means engrave): power 0-100 %, speed mm/s, passes; for engrave also line_interval and bidirectional. line_interval is the distance between engraved lines in mm; people think in lines per cm, so convert: line_interval = 10 / lines per cm (100 lines/cm = 0.1 mm), and report it back as lines per cm.")]
    fn set_material_setting(&self, Parameters(p): Parameters<SetMaterialSetting>) -> ToolResult {
        let r = library::update(|lib| {
            let machine = lib.machine(&p.setting.machine).ok_or(format!("no machine with id {}", p.setting.machine))?;
            if machine.laser(&p.setting.laser).is_none() {
                return Err(format!("{} has no laser module {}", machine.name, p.setting.laser));
            }
            let m = lib.material_mut(&p.material).ok_or(format!("no material with id {}", p.material))?;
            m.put(p.setting);
            Ok(m.clone())
        });
        match r {
            Ok(m) => ok(json!(m)),
            Err(e) => fail(e),
        }
    }
}
