//! Opening an xTool Studio project (`.xs`) as a Scriber document: the active canvas's elements as
//! items, one Scriber layer per xTool colour layer and processing type, the processing settings of
//! each binding on its layer, groups, and the machine from the device code. The format and the
//! mapping are in docs/xs-format.md (section 6).

mod geometry;
#[cfg(test)]
mod tests;

use crate::library::Library;
use crate::model::{Contour, Document, Img, Op, Overrides, Shape, geom};
use geometry::{Affine, Sub, apply, parse_path, place, scales, transform};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

/// Margin in mm around the drawing when the material size comes from the drawing.
const MARGIN: f64 = 10.0;

/// The imported document and what went into it.
pub struct Imported {
    pub doc: Document,
    pub layers: Vec<LayerCount>,
    /// Elements left out, with the reason.
    pub skipped: Vec<String>,
    /// Things the user should know (machine not found, settings left to the library, …).
    pub notes: Vec<String>,
}

#[derive(Serialize)]
pub struct LayerCount {
    pub id: u64,
    pub name: String,
    pub op: Op,
    pub items: usize,
}

/// xTool's processing types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Cut,
    Line,
    Fill,
    Bitmap,
}

impl Kind {
    fn from(name: &str) -> Option<Kind> {
        Some(match name {
            "VECTOR_CUTTING" => Kind::Cut,
            "VECTOR_ENGRAVING" => Kind::Line,
            "FILL_VECTOR_ENGRAVING" => Kind::Fill,
            "BITMAP_ENGRAVING" => Kind::Bitmap,
            _ => return None,
        })
    }

    fn op(self) -> Op {
        match self {
            Kind::Cut => Op::Cut,
            Kind::Line => Op::Score,
            Kind::Fill | Kind::Bitmap => Op::Engrave,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Cut => "Cut",
            Kind::Line => "Score",
            Kind::Fill => "Engrave",
            Kind::Bitmap => "Image",
        }
    }
}

/// A layer's settings from a patch: power, speed, passes and line interval.
type Settings = Option<(Overrides, Option<f64>)>;

/// Which Scriber layer an element goes on: its xTool colour, processing type (None = no output) and settings.
#[derive(Clone, PartialEq)]
struct Key {
    tag: String,
    kind: Option<Kind>,
    settings: Settings,
}

/// One element, converted.
struct Piece {
    key: Key,
    name: String,
    shape: Shape,
    image: Option<Img>,
    /// Outermost group tag (Scriber groups are flat).
    group: Option<String>,
}

struct Zip(zip::ZipArchive<std::io::Cursor<Vec<u8>>>);

impl Zip {
    fn bytes(&mut self, name: &str) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        self.0.by_name(name).map_err(|e| format!("{name}: {e}"))?.read_to_end(&mut out).map_err(|e| format!("{name}: {e}"))?;
        Ok(out)
    }

    fn json(&mut self, name: &str) -> Result<Value, String> {
        serde_json::from_slice(&self.bytes(name)?).map_err(|e| format!("{name}: {e}"))
    }

    fn names(&self, prefix: &str) -> Vec<String> {
        let mut v: Vec<String> = self.0.file_names().filter_map(Result::ok).filter(|n| n.starts_with(prefix) && n.ends_with(".json")).map(|n| n.into_owned()).collect();
        // displays-2 before displays-10.
        v.sort_by_key(|n| (n.len(), n.clone()));
        v
    }
}

/// "#00befe" → [0, 190, 254].
fn rgb(tag: &str) -> Option<[u8; 3]> {
    let h = tag.strip_prefix('#')?;
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    (h.len() == 6).then_some([c(0)?, c(2)?, c(4)?])
}

/// The element's processing per display id: (type, settings), from the base bindings.
fn bindings(mode: &Value, profiles: &Value) -> HashMap<String, Vec<(Kind, Settings)>> {
    let mut out: HashMap<String, Vec<(Kind, Settings)>> = HashMap::new();
    for b in mode["bindings"].as_array().into_iter().flatten().filter(|b| b.get("shadowOf").is_none_or(Value::is_null)) {
        let base = b["baseProfileId"].as_str().unwrap_or_default();
        let profile = &profiles["profiles"][base];
        let Some(kind) = Kind::from(profile["processingType"].as_str().unwrap_or_default()) else { continue };
        let patch = b["patchIds"].as_array().into_iter().flatten().filter_map(|p| mode["patches"].get(p.as_str()?)).find(|p| p["profileId"] == base);
        // Without a patch the profile holds xTool's placeholders (power 1 %): leave the numbers to the library.
        let settings = patch.map(|p| {
            let mut v: Map<String, Value> = profile["values"].as_object().cloned().unwrap_or_default();
            v.extend(p["overrides"].as_object().cloned().unwrap_or_default());
            let num = |k: &str| v.get(k).and_then(Value::as_f64);
            let o = Overrides { power: num("power").map(|p| p.clamp(0.0, 100.0)), speed: num("speed").filter(|s| *s > 0.0), passes: num("repeat").map(|r| (r as u32).max(1)) };
            // density is lines per cm.
            let interval = matches!(kind, Kind::Fill | Kind::Bitmap).then(|| num("density")).flatten().filter(|d| *d > 0.0).map(|d| 10.0 / d);
            (o, interval)
        });
        for id in b["displayIds"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            out.entry(id.into()).or_default().push((kind, settings));
        }
    }
    out
}

/// A `PATH` display's outline in world mm (its own path data, or the vector store's).
fn path_contours(d: &Value, vectors: &HashMap<String, String>) -> Result<Vec<Contour>, String> {
    let data = match (d["dPath"].as_str(), d["vectorRef"]["vectorHash"].as_str()) {
        (Some(p), _) => p,
        (None, Some(h)) => vectors.get(h).ok_or("its path data is missing from vectors/svg")?,
        _ => return Err("it has no path data".into()),
    };
    Ok(place(&parse_path(data)?, &transform(d)))
}

/// What one display becomes: a shape and maybe the picture it shows.
fn convert(d: &Value, zip: &mut Zip, vectors: &HashMap<String, String>) -> Result<(Shape, Option<Img>), String> {
    let t: Affine = transform(d);
    let (sx, sy) = scales(&t);
    if sx < 1e-12 || sy < 1e-12 {
        return Err("its transform has no size".into());
    }
    let f = |k: &str| d[k].as_f64().unwrap_or(0.0);
    // width/height (and oWidth/oHeight) are world sizes; the outline is built in local units.
    let (w, h) = (f("width") / sx, f("height") / sy);
    let one = |s: Sub| Ok((Shape::Path { contours: place(&[s], &t) }, None));
    match d["type"].as_str().unwrap_or_default() {
        "PATH" => Ok((Shape::Path { contours: path_contours(d, vectors)? }, None)),
        "LINE" => {
            let end = [d["endPoint"]["x"].as_f64().unwrap_or(0.0), d["endPoint"]["y"].as_f64().unwrap_or(0.0)];
            Ok((Shape::path(Contour::line(apply(&t, [0.0, 0.0]), apply(&t, end))), None))
        }
        "RECT" => {
            let corner = d["primitiveCornerData"].as_str().and_then(|s| serde_json::from_str::<Value>(s).ok());
            let r = d["radius"].as_f64().or_else(|| corner?["defaultCorner"]["desiredRadius"].as_f64()).unwrap_or(0.0);
            one(geometry::rect(w, h, r / sx))
        }
        "CIRCLE" => one(geometry::ellipse(w, h)),
        kind @ ("REGULAR_POLYGON" | "STAR") => {
            let count = d["count"].as_u64().filter(|n| *n >= 3).ok_or("it has fewer than 3 corners")? as usize;
            let inner = (kind == "STAR").then(|| d["starInnerScale"].as_f64().unwrap_or(0.382));
            let (ow, oh) = (d["oWidth"].as_f64().map_or(w, |v| v / sx), d["oHeight"].as_f64().map_or(h, |v| v / sy));
            one(geometry::polygon(count, inner, ow, oh, [w, h]))
        }
        "TEXT" => {
            let chars = d["charJSONs"].as_array().filter(|c| !c.is_empty()).ok_or("text without glyph outlines (charJSONs)")?;
            let mut contours = Vec::new();
            for c in chars {
                contours.extend(path_contours(c, vectors)?);
            }
            Ok((Shape::Path { contours }, None))
        }
        "BITMAP" => {
            let res = d["resourcePath"].as_str().ok_or("image without resourcePath")?;
            let img = crate::export::image::to_grey(&zip.bytes(res)?).map_err(|e| format!("cannot read {res}: {e}"))?;
            // The picture is a unit-pixel raster under the transform.
            let (pw, ph) = (d["originWidth"].as_f64().unwrap_or(img.w as f64), d["originHeight"].as_f64().unwrap_or(img.h as f64));
            let (o, right, down) = (apply(&t, [0.0, 0.0]), apply(&t, [pw, 0.0]), apply(&t, [0.0, ph]));
            let (iw, ih) = (geom::distance(o, right), geom::distance(o, down));
            let centre = [(right[0] + down[0]) / 2.0, (right[1] + down[1]) / 2.0];
            let rotation = (right[1] - o[1]).atan2(right[0] - o[0]).to_degrees();
            let shape = Shape::Image { image: 0, x: centre[0] - iw / 2.0, y: centre[1] - ih / 2.0, w: iw, h: ih, rotation };
            Ok((shape, Some(img)))
        }
        other => Err(format!("{other} elements are not supported")),
    }
}

fn type_name(d: &Value) -> String {
    match d["type"].as_str().unwrap_or("element") {
        "REGULAR_POLYGON" => "Polygon".into(),
        "BITMAP" => "Image".into(),
        t => t[..1].to_uppercase() + &t[1..].to_lowercase(),
    }
}

/// Read an `.xs` project into a new document (machine and laser from `lib` by the device code).
pub fn read(bytes: Vec<u8>, lib: &Library) -> Result<Imported, String> {
    let mut zip = Zip(zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not an xTool Studio project: {e}"))?);
    let (mut skipped, mut notes) = (Vec::new(), Vec::new());
    let project = zip.json("project.json")?;
    let profiles = zip.json("profiles.json").unwrap_or_default();
    let canvases = project["modules"]["canvases"].as_array().map_or(0, Vec::len);
    let canvas_id = project["activeCanvasId"].as_str().or_else(|| project["modules"]["canvases"][0].as_str()).ok_or("project.json names no canvas")?.to_string();
    if canvases > 1 {
        notes.push(format!("The project has {canvases} canvases; only the active one was imported."));
    }
    let canvas = zip.json(&format!("canvases/{canvas_id}.json"))?;
    let device_file = project["activeDeviceId"].as_str().map(|d| format!("devices/device-{d}.json")).filter(|n| zip.0.index_for_name(n).is_some()).or_else(|| zip.names("devices/").into_iter().next());
    let device = device_file.map(|n| zip.json(&n)).transpose()?.unwrap_or_default();
    let mode = &device["processing"][&canvas_id]["modes"][device["processing"][&canvas_id]["activeMode"].as_str().unwrap_or("LASER_PLANE")];
    let bound = bindings(mode, &profiles);
    let ignored: HashSet<&str> = mode["ignoredDisplayIds"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let mut vectors = HashMap::new();
    for name in zip.names("vectors/svg/data-") {
        if let Some(entries) = zip.json(&name)?["entries"].as_object() {
            vectors.extend(entries.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))));
        }
    }
    let mut displays: Vec<Value> = Vec::new();
    for name in zip.names(&format!("canvases/{canvas_id}/displays-")) {
        displays.extend(zip.json(&name)?["displays"].as_array().cloned().unwrap_or_default());
    }
    // Drawn bottom to top.
    displays.sort_by(|a, b| a["zOrder"].as_f64().unwrap_or(0.0).total_cmp(&b["zOrder"].as_f64().unwrap_or(0.0)));

    let mut pieces = Vec::new();
    let mut unbound = 0;
    for d in &displays {
        let id = d["id"].as_str().unwrap_or_default();
        let ty = type_name(d);
        let label = match d["type"].as_str() {
            Some("TEXT") => d["text"].as_str().unwrap_or_default().to_string(),
            _ => d["name"].as_str().filter(|n| !n.is_empty()).map_or(ty.clone(), String::from),
        };
        let (shape, image) = match convert(d, &mut zip, &vectors) {
            Ok(s) => s,
            Err(e) => {
                skipped.push(format!("{ty} {id}: {e}"));
                continue;
            }
        };
        if shape.contours().iter().all(|c| c.vertices.len() < 2) && image.is_none() {
            skipped.push(format!("{ty} {id}: empty outline"));
            continue;
        }
        let is_image = image.is_some();
        // An image is processed by its bitmap binding, a vector by its first other one.
        let proc = bound.get(id).and_then(|v| v.iter().find(|(k, _)| (*k == Kind::Bitmap) == is_image)).copied();
        if proc.is_none() && !ignored.contains(id) {
            unbound += 1;
        }
        let (kind, settings) = match proc {
            Some(p) if !ignored.contains(id) => (Some(p.0), p.1),
            _ => (None, None),
        };
        let tag = d["layerTag"].as_str().or(d["layerColor"].as_str()).unwrap_or("#000000").to_lowercase();
        let group = d["groupTags"].as_array().and_then(|g| g.last()).and_then(Value::as_str).map(String::from);
        pieces.push(Piece { key: Key { tag, kind, settings }, name: label, shape, image, group });
    }
    if unbound > 0 {
        notes.push(format!("{unbound} elements had no processing type in the project; they are on no-output layers."));
    }
    if pieces.is_empty() {
        return Err(format!("nothing to import{}", if skipped.is_empty() { String::new() } else { format!(": {}", skipped.join("; ")) }));
    }

    // Layers: in xTool's layer order, then by type (no output last), then first use.
    let layer_data = &canvas["layerData"];
    let order = |tag: &str| layer_data[tag]["order"].as_f64().unwrap_or(f64::MAX);
    let mut keys: Vec<Key> = Vec::new();
    for p in &pieces {
        if !keys.contains(&p.key) {
            keys.push(p.key.clone());
        }
    }
    keys.sort_by(|a, b| order(&a.tag).total_cmp(&order(&b.tag)).then((a.kind.is_none(), a.kind).cmp(&(b.kind.is_none(), b.kind))));

    let mut doc = Document::new(300.0, 200.0);
    doc.s.layers.clear();
    let mut layer_ids = Vec::new();
    for k in &keys {
        let base = layer_data[&k.tag]["name"].as_str().filter(|n| !n.trim().is_empty()).map_or_else(|| k.tag.to_uppercase(), String::from);
        let shared = keys.iter().filter(|o| o.tag == k.tag).count() > 1;
        let name = if shared { format!("{base} ({})", k.kind.map_or("No output", Kind::label)) } else { base };
        let id = doc.add_layer(&name, k.kind.map_or(Op::NoOutput, Kind::op));
        let l = doc.s.layer_mut(id).unwrap();
        l.color = rgb(&k.tag);
        l.visible = layer_data[&k.tag]["visible"].as_bool().unwrap_or(true);
        if let Some((o, interval)) = k.settings {
            (l.settings, l.line_interval) = (o, interval);
        }
        layer_ids.push(id);
    }

    let mut groups: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for p in pieces {
        let layer = layer_ids[keys.iter().position(|k| *k == p.key).unwrap()];
        let shape = match (p.image, p.shape) {
            (Some(img), Shape::Image { x, y, w, h, rotation, .. }) => {
                let image = doc.new_id();
                doc.s.images.insert(image, Arc::new(img));
                Shape::Image { image, x, y, w, h, rotation }
            }
            (_, shape) => shape,
        };
        let id = doc.add_to(layer, &p.name, shape);
        if let Some(g) = p.group {
            groups.entry(g).or_default().push(id);
        }
    }
    doc.s.groups = groups.into_values().filter(|g| g.len() > 1).collect();

    // Machine, laser module and material size.
    let code = device["deviceCode"].as_str().unwrap_or_default();
    let module = profiles["profiles"].as_object().into_iter().flat_map(|p| p.values()).find_map(|p| p["values"]["laser"].as_str());
    let watts = device["power"][0].as_f64();
    let all: Vec<u64> = doc.s.items.iter().map(|i| i.id).collect();
    let bbox = doc.bbox(&all).unwrap_or([0.0; 4]);
    let mut area = None;
    match lib.machines.iter().find(|m| !code.is_empty() && m.xtool_device() == Some(code)) {
        Some(m) => {
            let laser = m.lasers.iter().find(|l| module.is_some_and(|s| l.xtool_module() == s)).or_else(|| m.lasers.iter().find(|l| Some(l.power_w) == watts)).or(m.lasers.first());
            doc.s.machine = Some(m.id.clone());
            doc.s.laser = laser.map(|l| l.id.clone());
            area = Some(m.area(doc.s.laser.as_deref()));
        }
        None if code.is_empty() => notes.push("The project names no machine.".into()),
        None => notes.push(format!("No machine in the library has the xTool device code {code}; set it in Machines & materials, then pick the machine for this document.")),
    }
    let content = [bbox[0] + bbox[2] + MARGIN, bbox[1] + bbox[3] + MARGIN];
    let [w, h] = match area {
        Some([w, h]) if content[0] <= w && content[1] <= h => [w, h],
        Some([w, h]) => {
            notes.push(format!("The drawing reaches beyond the machine's {w} × {h} mm work area; the material was made larger to hold it."));
            [w.max(content[0]), h.max(content[1])]
        }
        None => content.map(|v| (v.max(10.0) * 10.0).ceil() / 10.0),
    };
    (doc.s.width, doc.s.height) = (w, h);
    if bbox[0] < 0.0 || bbox[1] < 0.0 {
        notes.push("Part of the drawing lies left of or above the material's origin.".into());
    }
    if mode["data"]["material"].as_f64().is_some_and(|m| m > 0.0) {
        notes.push("The project's xTool material is not linked; import its settings with \"Laser settings from an xTool project\" in Machines & materials if needed.".into());
    }
    doc.dirty = true;
    doc.selection.clear();

    let layers = doc.s.layers.iter().map(|l| LayerCount { id: l.id, name: l.name.clone(), op: l.op, items: doc.s.items.iter().filter(|i| i.layer == l.id).count() }).collect();
    Ok(Imported { doc, layers, skipped, notes })
}

/// `read` from a file.
pub fn read_file(path: &std::path::Path, lib: &Library) -> Result<Imported, String> {
    read(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?, lib)
}
