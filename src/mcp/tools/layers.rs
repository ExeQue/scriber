//! MCP tools for layers: each layer carries the laser op and the power/speed/passes of its items.

use super::{Server, ToolResult, fail, ok};
use crate::library;
use crate::model::layer::op_operation;
use crate::model::{Document, Op, Overrides};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddLayer {
    pub name: String,
    pub op: Op,
    /// Make it the layer new shapes drawn in the GUI go on.
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateLayer {
    pub id: u64,
    pub name: Option<String>,
    /// Changes what the laser does with every item on the layer.
    pub op: Option<Op>,
    /// Canvas colour as [r, g, b]; [] = the op's colour.
    pub color: Option<Vec<u8>>,
    /// Replaces the layer's own power (%), speed (mm/s) and passes; a missing one (or {}) uses the
    /// material library's preset for the layer's op.
    pub settings: Option<Overrides>,
    /// Engrave layers: the layer's own line spacing in lines per cm (10 / mm between lines; 100 = 0.1 mm);
    /// null = the material library's.
    #[serde(default, deserialize_with = "present")]
    pub lines_per_cm: Option<Option<f64>>,
    /// The material's profile for the layer's op (a setting's name, see list_materials); "" = the default one.
    pub profile: Option<String>,
    /// Engrave layers: how its images are engraved, the id of one of the machine's image modes
    /// (list_machines image_modes, e.g. "bayer"); "" = the machine's default.
    pub image_mode: Option<String>,
    /// Hidden layers are not drawn, lasered or exported.
    pub visible: Option<bool>,
    /// Items on a locked layer cannot be selected or edited.
    pub locked: Option<bool>,
    /// New position in the list (0 = first).
    pub order: Option<usize>,
    /// Make it the current layer (where shapes drawn in the GUI go).
    pub current: Option<bool>,
}

/// A field that is there (even as null) is Some, so null can clear.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteLayer {
    pub id: u64,
    /// Layer to move the items to; without it a layer that has items is not deleted.
    pub move_to: Option<u64>,
}

/// The layers with their item counts and the settings the laser uses for each.
pub(super) fn layers(d: &Document) -> Value {
    let lib = library::get();
    let list: Vec<Value> = d
        .s
        .layers
        .iter()
        .map(|l| {
            let mut v = serde_json::to_value(l).unwrap();
            v["items"] = d.s.items.iter().filter(|i| i.layer == l.id).count().into();
            v["current"] = (l.id == d.current_layer()).into();
            if let Some(i) = l.line_interval {
                v["lines_per_cm"] = (10.0 / i).into();
            }
            v["laser_setting"] = json!(op_operation(l.op).and_then(|op| l.setting(&d.s, &lib, op)));
            if l.op == Op::Engrave {
                v["image_mode_used"] = json!(l.image_mode(&d.s, &lib).map(|m| m.id));
            }
            v
        })
        .collect();
    json!(list)
}

/// Why `id` cannot be a layer's image mode: the document's machine does not offer it. A document
/// without a machine takes any id (the mode is checked when a machine is chosen, by the pre-flight check).
fn unknown_image_mode(s: &crate::model::State, id: &str) -> Option<String> {
    let machine = library::get().machine(s.machine.as_deref()?)?.clone();
    let modes = machine.image_modes();
    if id.is_empty() || modes.iter().any(|m| m.id == id) {
        return None;
    }
    Some(match modes.len() {
        0 => format!("{} has no image modes to choose from", machine.name),
        _ => format!("{} has no image mode \"{id}\"; it has: {}", machine.name, modes.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")),
    })
}

#[tool_router(router = layers_router, vis = "pub(super)")]
impl Server {
    #[tool(description = "List the layers in order: id, name, op, colour, own settings (power/speed/passes that override the material library), lines_per_cm (the layer's own engrave line spacing; line_interval is the same in mm), image_mode (the layer's own; image_mode_used on engrave layers: the one its images get, the machine's default when it has none; null when the machine has no modes), visible, locked, number of items, whether it is current, and laser_setting: the power/speed/passes the laser uses (library preset for the op with the layer's own numbers on top; null when neither gives them).")]
    fn list_layers(&self) -> ToolResult {
        ok(layers(&self.doc.lock()))
    }

    #[tool(description = "Add a layer at the end with a name and op (cut, score, engrave, no_output). A taken name gets a number. Returns the layers.")]
    fn add_layer(&self, Parameters(p): Parameters<AddLayer>) -> ToolResult {
        self.with(|d| {
            d.checkpoint();
            let id = d.add_layer(&p.name, p.op);
            if p.current {
                d.set_current_layer(id);
            }
            ok(json!({"id": id, "layers": layers(d)}))
        })
    }

    #[tool(description = "Change a layer: name, op (changes what the laser does with all its items), color, settings (the layer's own power/speed/passes; missing = material library), lines_per_cm (engrave line spacing, lines per cm = 10 / mm; null = material library), profile, image_mode (how images on an engrave layer are engraved: greyscale or a dither, an id from the machine's image_modes in list_machines; \"\" = the machine's default), visible, locked, order (position) and current. One undo step.")]
    fn update_layer(&self, Parameters(p): Parameters<UpdateLayer>) -> ToolResult {
        if let Some(e) = p.settings.as_ref().and_then(|o| o.check().err()) {
            return fail(e);
        }
        if p.lines_per_cm.flatten().is_some_and(|l| !(l > 0.0 && l.is_finite())) {
            return fail("lines_per_cm must be larger than 0");
        }
        let color = match p.color.as_deref() {
            None => None,
            Some([]) => Some(None),
            Some(&[r, g, b]) => Some(Some([r, g, b])),
            Some(_) => return fail("color is [r, g, b] or [] for the op's colour"),
        };
        self.with(|d| {
            if d.s.layer(p.id).is_none() {
                return fail(format!("no layer with id {}", p.id));
            }
            if let Some(e) = p.image_mode.as_deref().and_then(|id| unknown_image_mode(&d.s, id)) {
                return fail(e);
            }
            d.checkpoint();
            let l = d.s.layer_mut(p.id).unwrap();
            l.name = p.name.unwrap_or(l.name.clone());
            l.op = p.op.unwrap_or(l.op);
            l.color = color.unwrap_or(l.color);
            l.settings = p.settings.unwrap_or(l.settings);
            if let Some(lines) = p.lines_per_cm {
                l.line_interval = lines.map(|l| 10.0 / l);
            }
            if let Some(name) = p.profile {
                l.profile = (!name.is_empty()).then_some(name);
            }
            if let Some(id) = p.image_mode {
                l.image_mode = (!id.is_empty()).then_some(id);
            }
            l.visible = p.visible.unwrap_or(l.visible);
            l.locked = p.locked.unwrap_or(l.locked);
            if let Some(k) = p.order {
                d.move_layer(p.id, k).ok();
            }
            if p.current == Some(true) {
                d.set_current_layer(p.id);
            }
            d.touch();
            ok(layers(d))
        })
    }

    #[tool(description = "Delete a layer. Its items move to move_to; without move_to a layer with items is refused. The last layer cannot be deleted.")]
    fn delete_layer(&self, Parameters(p): Parameters<DeleteLayer>) -> ToolResult {
        self.with(|d| {
            let before = d.s.clone();
            match d.delete_layer(p.id, p.move_to) {
                Ok(()) => {
                    d.push_undo(before);
                    ok(layers(d))
                }
                Err(e) => fail(e),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::{AddItems, UpdateItem};
    use rmcp::model::CallToolResult;
    use std::sync::Arc;

    fn text(r: ToolResult) -> Value {
        let r: CallToolResult = r.unwrap();
        let t = r.content[0].as_text().unwrap().text.clone();
        assert_ne!(r.is_error, Some(true), "{t}");
        serde_json::from_str(&t).unwrap()
    }

    fn args<T: serde::de::DeserializeOwned>(v: Value) -> Parameters<T> {
        Parameters(serde_json::from_value(v).unwrap())
    }

    #[test]
    fn layer_tools_round_trip() {
        let ws = crate::model::Workspace::new(Document::new(100.0, 100.0));
        let doc = ws.active().1;
        let server = Server::new(ws, Arc::new(|| {}));
        let deep = text(server.add_layer(args(json!({"name": "Deep cut", "op": "cut"}))))["id"].as_u64().unwrap();
        // op maps to the first layer with it; layer wins over op.
        let added: Parameters<AddItems> = args(json!({"items": [
            {"shape": {"kind": "circle", "cx": 10, "cy": 10, "r": 5}, "op": "score"},
            {"shape": {"kind": "circle", "cx": 30, "cy": 10, "r": 5}, "layer": deep, "op": "engrave"}
        ]}));
        let ids = text(server.add_items(added))["ids"].clone();
        let (a, b) = (ids[0].as_u64().unwrap(), ids[1].as_u64().unwrap());
        {
            let d = doc.lock();
            assert_eq!(d.s.op(d.item(a).unwrap()), Op::Score);
            assert_eq!(d.item(b).unwrap().layer, deep);
        }
        let set = json!({"id": deep, "op": "engrave", "settings": {"passes": 3}, "color": [1, 2, 3], "order": 0});
        let list = text(server.update_layer(args(set)));
        assert_eq!((list[0]["id"].as_u64(), list[0]["op"].as_str(), list[0]["settings"]["passes"].as_u64()), (Some(deep), Some("engrave"), Some(3)));
        {
            let d = doc.lock();
            assert!(d.s.engraves_area(d.item(b).unwrap()), "the layer's op change reaches its items");
        }
        // Lines per cm in, mm stored, lines per cm out; null clears it.
        let list = text(server.update_layer(args(json!({"id": deep, "lines_per_cm": 200}))));
        assert_eq!((list[0]["line_interval"].as_f64(), list[0]["lines_per_cm"].as_f64()), (Some(0.05), Some(200.0)));
        assert_eq!(server.update_layer(args(json!({"id": deep, "lines_per_cm": 0}))).unwrap().is_error, Some(true));
        text(server.update_layer(args(json!({"id": deep, "name": "Deep"}))));
        assert_eq!(doc.lock().s.layer(deep).unwrap().line_interval, Some(0.05), "missing leaves it");
        let list = text(server.update_layer(args(json!({"id": deep, "lines_per_cm": null}))));
        assert!(list[0].get("line_interval").is_none() && list[0].get("lines_per_cm").is_none(), "{list}");
        // update_item moves an item by layer or by op.
        let moved: Parameters<UpdateItem> = args(json!({"id": a, "op": "cut"}));
        text(server.update_item(moved));
        let cut = doc.lock().layer_for(Op::Cut);
        assert_eq!(doc.lock().item(a).unwrap().layer, cut);
        // A layer with items is only deleted with somewhere to move them; undo brings it back.
        assert_eq!(server.delete_layer(args(json!({"id": deep}))).unwrap().is_error, Some(true));
        text(server.delete_layer(args(json!({"id": deep, "move_to": cut}))));
        assert_eq!(doc.lock().item(b).unwrap().layer, cut);
        assert!(doc.lock().undo());
        assert_eq!(doc.lock().item(b).unwrap().layer, deep);
        // Image modes: any id without a machine; with one, only its modes; "" goes back to its default.
        let engrave = doc.lock().layer_for(Op::Engrave);
        let mode = |list: &Value| list.as_array().unwrap().iter().find(|l| l["id"].as_u64() == Some(engrave)).unwrap().clone();
        assert!(mode(&text(server.update_layer(args(json!({"id": engrave, "image_mode": "anything"})))))["image_mode_used"].is_null(), "no machine");
        doc.lock().s.machine = Some("xtool-m2".into());
        let refused = server.update_layer(args(json!({"id": engrave, "image_mode": "nope"}))).unwrap();
        assert_eq!(refused.is_error, Some(true));
        assert!(refused.content[0].as_text().unwrap().text.contains("bayer"), "the error lists the machine's modes");
        let l = mode(&text(server.update_layer(args(json!({"id": engrave, "image_mode": "bayer"})))));
        assert_eq!((l["image_mode"].as_str(), l["image_mode_used"].as_str()), (Some("bayer"), Some("bayer")));
        let l = mode(&text(server.update_layer(args(json!({"id": engrave, "image_mode": ""})))));
        assert_eq!((l.get("image_mode"), l["image_mode_used"].as_str()), (None, Some("grayscale")));
        // Items on a locked layer cannot be edited.
        text(server.update_layer(args(json!({"id": deep, "locked": true}))));
        let edit: Parameters<UpdateItem> = args(json!({"id": b, "name": "x"}));
        assert_eq!(server.update_item(edit).unwrap().is_error, Some(true));
    }
}
