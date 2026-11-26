//! What tools send back about items: compact by default (no geometry), so a big result does not
//! overflow the agent's context; `verbose: true` adds the full geometry.

use crate::model::{Document, Item, Shape};
use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;
use serde_json::{Value, json};

/// get_document leaves the geometry out when the full reply would be longer than this (chars).
pub const FULL_LIMIT: usize = 20_000;

/// The `verbose` switch of tools that change items.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct Detail {
    /// Reply with each item's full geometry. Default false: id, name, layer, op, kind, bbox,
    /// rotation and (paths) contour and vertex counts; get_document with ids gives the geometry later.
    #[serde(default)]
    pub verbose: bool,
}

/// Item ids plus the verbose switch.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct IdsDetail {
    /// Item ids; empty = the user's current selection.
    #[serde(default)]
    pub ids: Vec<u64>,
    #[serde(flatten)]
    pub detail: Detail,
}

/// One item: compact, or with its geometry. Both carry op, bbox and rotation.
pub fn item(d: &Document, i: &Item, verbose: bool) -> Value {
    let mut v = if verbose { serde_json::to_value(i).unwrap() } else { json!({"id": i.id, "name": i.name, "layer": i.layer}) };
    v["op"] = json!(d.s.op(i));
    v["bbox"] = json!(i.shape.metrics().bbox);
    v["rotation"] = json!(i.rotation);
    if !verbose {
        match &i.shape {
            Shape::Path { contours } => {
                v["kind"] = "path".into();
                v["contours"] = contours.len().into();
                v["vertices"] = contours.iter().map(|c| c.vertices.len()).sum::<usize>().into();
            }
            // Text, dimensions, images and guides are small; their kind is the shape's tag.
            s => v["kind"] = serde_json::to_value(s).unwrap()["kind"].clone(),
        }
    }
    v
}

/// The items among `ids` that exist, in that order.
pub fn items(d: &Document, ids: &[u64], verbose: bool) -> Value {
    json!(ids.iter().filter_map(|id| d.item(*id)).map(|i| item(d, i, verbose)).collect::<Vec<_>>())
}

/// The usual reply of a tool that changed items: `{ids, items}`.
pub fn changed(d: &Document, ids: &[u64], verbose: bool) -> Value {
    json!({"ids": ids, "items": items(d, ids, verbose)})
}

/// What get_document shows: everything, or only `ids`; geometry when asked, for picked ids, else when it fits.
pub fn document(d: &Document, ids: &[u64], geometry: Option<bool>) -> Value {
    let pick: Vec<&Item> = d.s.items.iter().filter(|i| ids.is_empty() || ids.contains(&i.id)).collect();
    let mut out = head(d);
    out["items"] = json!(pick.iter().map(|i| item(d, i, geometry != Some(false))).collect::<Vec<_>>());
    if geometry.is_none() && ids.is_empty() && out.to_string().len() > FULL_LIMIT {
        out["items"] = json!(pick.iter().map(|i| item(d, i, false)).collect::<Vec<_>>());
        out["geometry_omitted"] = format!(
            "the document is large, so items carry no geometry; call get_document with ids: [...] for the geometry of particular items, or geometry: true for all of it (over {FULL_LIMIT} chars)"
        )
        .into();
    }
    out
}

/// Everything about the document but its items.
fn head(d: &Document) -> Value {
    let images: Vec<Value> = d.s.images.iter().map(|(id, i)| json!({"image": id, "px_width": i.w, "px_height": i.h})).collect();
    json!({
        "width": d.s.width, "height": d.s.height, "units": "mm",
        "file": d.path, "unsaved_changes": d.dirty,
        "notes": d.s.notes, "selection": d.selection,
        "layers": super::layers::layers(d), "items": [], "images": images,
        "machine": d.s.machine, "laser": d.s.laser, "material": d.s.material,
        "groups": d.s.groups,
    })
}
