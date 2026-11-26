//! xTool Studio project (`.xs`) export: one canvas of `PATH` elements, one colour layer per Scriber
//! layer, and per layer and processing type a binding with the layer's power, speed and passes.
//! The format is described in docs/xs-format.md.

mod files;
mod path;
#[cfg(test)]
mod tests;

use crate::library::{Library, Operation, Setting, xtool};
use crate::model::layer::op_operation;
use crate::model::{Op, Shape, State, geom};
use anyhow::{Context, bail};
use files::{Kind, Outline};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;

/// Longer path strings go to the `vectors/svg` store, as xTool does with long ones (it moved a 63 KB
/// path; its threshold is unknown, short ones stay inline).
const INLINE_MAX: usize = 32 * 1024;

/// The finished project and what went into it.
pub struct Xs {
    pub bytes: Vec<u8>,
    pub elements: usize,
    /// xTool colour layers.
    pub layers: usize,
    /// Items left out that would be lasered (rotated or unreadable images).
    pub skipped: Vec<String>,
    /// Things the user should know (defaults used, …).
    pub notes: Vec<String>,
}

pub fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `<prefix>:` + 12 random letters and digits, as xTool's profile, patch and binding ids.
fn short_id(prefix: &str) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let bytes = uuid::Uuid::new_v4().into_bytes();
    format!("{prefix}:{}", bytes[..12].iter().map(|b| ABC[*b as usize % ABC.len()] as char).collect::<String>())
}

/// The key of a path in the vector store: sha256 of the string, lowercase hex.
fn hash(s: &str) -> String {
    hash_bytes(s.as_bytes())
}

/// sha256 as lowercase hex (also the name of an image resource).
fn hash_bytes(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|b| format!("{b:02x}")).collect()
}

/// The picture as an 8-bit RGBA PNG (the only kind seen in xTool projects) and its size in pixels.
fn rgba_png(png: &[u8]) -> Option<(Vec<u8>, [u32; 2])> {
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some((out.into_inner(), [img.width(), img.height()]))
}

/// One binding: the elements of one Scriber layer that get one processing type, and their numbers.
struct Group {
    layer: u64,
    kind: Kind,
    displays: Vec<Value>,
    setting: Option<Setting>,
}

/// The xTool device code, its short name, the module string ("laser_blue_10W") and watts.
fn machine(s: &State, lib: &Library, notes: &mut Vec<String>) -> anyhow::Result<(&'static str, String, String, f64)> {
    let Some(id) = s.machine.as_deref() else {
        notes.push("No machine chosen: written for the xTool M2 with the 10 W blue laser.".into());
        return Ok(("JS002", "M2".into(), "laser_blue_10W".into(), 10.0));
    };
    let code = xtool::device_for(id).with_context(|| format!("xTool Studio export needs an xTool machine; {id} is not one (supported: xtool-m2)"))?;
    let m = lib.machine(id).with_context(|| format!("unknown machine {id}"))?;
    let laser = s.laser.as_deref().and_then(|l| m.laser(l)).or(m.lasers.first()).with_context(|| format!("{} has no laser module", m.name))?;
    let w = laser.power_w;
    Ok((code, m.model.clone(), format!("laser_{}_{w}W", xtool::source_for(laser.kind)), w))
}

/// The patch overrides: xTool's defaults for the type with the layer's numbers on top.
fn overrides(kind: Kind, laser: &str, s: Option<&Setting>) -> Map<String, Value> {
    let mut v = kind.values(laser);
    if let Some(s) = s {
        v.insert("power".into(), json!(s.power));
        v.insert("speed".into(), json!(s.speed));
        v.insert("repeat".into(), json!(s.passes));
        if let (Kind::Fill | Kind::Bitmap, Some(i)) = (kind, s.line_interval.filter(|i| *i > 0.0)) {
            // density is lines per cm; dpi = density × 2.54 (xTool's 100 goes with 254).
            let density = (10.0 / i * 1000.0).round() / 1000.0;
            v.insert("density".into(), json!(density));
            v.insert("dpiX".into(), json!((density * 2.54).round()));
            v.insert("dpiY".into(), json!((density * 2.54).round()));
        }
    }
    v
}

/// The document as an xTool Studio project. Hidden and no-output layers, guides and dimensions are
/// left out; text goes out as its glyph outlines; images are engraved in greyscale (rotated ones are
/// skipped and listed in `skipped`).
pub fn xs(s: &State, lib: &Library, name: &str) -> anyhow::Result<Xs> {
    let mut notes = Vec::new();
    let mut skipped = Vec::new();
    let (code, ext_name, laser, watts) = machine(s, lib, &mut notes)?;
    let canvas_id = uuid();
    let mut groups: Vec<Group> = Vec::new();
    let mut layer_data = Map::new();
    let mut vectors = Map::new();
    let mut resources = std::collections::BTreeMap::new();
    let mut z = 0;
    for layer in s.layers.iter().filter(|l| l.visible && l.op != Op::NoOutput) {
        let [r, g, b] = layer.rgb();
        let tag = format!("#{r:02x}{g:02x}{b:02x}");
        for item in s.items.iter().filter(|i| i.layer == layer.id) {
            let name = if item.name.is_empty() { item.id.to_string() } else { format!("{} ({})", item.name, item.id) };
            let (kind, display) = if let Shape::Image { image, x, y, w, h, rotation } = item.shape {
                // ponytail: rotated images are skipped; bake the rotation into the raster when needed.
                if rotation.abs() > 1e-9 {
                    skipped.push(format!("image {name} on layer {}: rotated images are not exported yet", layer.name));
                    continue;
                }
                let Some(png) = s.images.get(&image).and_then(|img| rgba_png(&img.png)) else {
                    skipped.push(format!("image {name} on layer {}: its picture is missing or unreadable", layer.name));
                    continue;
                };
                let file = format!("{}.png", hash_bytes(&png.0));
                z += 1;
                let display = files::bitmap(&uuid(), &tag, [r, g, b], z, &format!("resources/{file}"), png.1, [x, y, w, h]);
                resources.insert(file, png.0);
                (Kind::Bitmap, display)
            } else {
                let contours = item.shape.laser_contours();
                if contours.iter().all(|c| c.vertices.len() < 2) {
                    continue;
                }
                let area = s.engraves_area(item);
                let kind = match (layer.op.on(&item.shape), area) {
                    (Op::Cut, _) => Kind::Cut,
                    (_, true) => Kind::Fill,
                    _ => Kind::Line,
                };
                let d = path::dpath(&contours);
                let path = if d.len() > INLINE_MAX {
                    let h = hash(&d);
                    vectors.insert(h.clone(), Value::String(d));
                    ("vectorRef", json!({"vectorHash": h, "bucketType": "svg", "originalField": "dPath"}))
                } else {
                    ("dPath", Value::String(d))
                };
                let boxes: Vec<_> = contours.iter().filter(|c| !c.vertices.is_empty()).map(|c| c.bbox()).collect();
                let outline = Outline { path, bbox: geom::union(&boxes).unwrap_or_default(), closed: contours.iter().all(|c| c.closed), compound: contours.len() > 1 };
                z += 1;
                (kind, files::display(&uuid(), &tag, [r, g, b], z, area, outline))
            };
            let n = layer_data.len() + 1;
            layer_data.entry(tag.clone()).or_insert_with(|| json!({"name": tag.to_uppercase(), "order": n, "visible": true}));
            match groups.iter_mut().find(|g| g.layer == layer.id && g.kind == kind) {
                Some(g) => g.displays.push(display),
                None => {
                    let operation = match kind {
                        Kind::Line => Operation::Score,
                        // An image is engraved, whatever its layer.
                        Kind::Bitmap => Operation::Engrave,
                        _ => op_operation(layer.op).unwrap(),
                    };
                    let setting = layer.setting(s, lib, operation);
                    if setting.is_none() {
                        notes.push(format!("Layer {}: no power, speed and passes for {} (none on the layer or in the material library); xTool's defaults are used.", layer.name, operation.label().to_lowercase()));
                    }
                    groups.push(Group { layer: layer.id, kind, displays: vec![display], setting });
                }
            }
        }
    }

    // xTool writes the bitmap profile only when the project has an image.
    let kinds: Vec<Kind> = Kind::ALL.into_iter().chain(groups.iter().any(|g| g.kind == Kind::Bitmap).then_some(Kind::Bitmap)).collect();
    let profile_ids: Vec<String> = kinds.iter().map(|_| short_id("profile")).collect();
    let profile_of = |k: Kind| profile_ids[kinds.iter().position(|x| *x == k).unwrap()].clone();
    let mut patches = Map::new();
    let mut bindings = Vec::new();
    for g in &groups {
        let (binding, patch) = (short_id("binding"), short_id("patch"));
        let profile = profile_of(g.kind);
        patches.insert(patch.clone(), json!({"id": patch, "profileId": profile, "source": "custom", "overrides": overrides(g.kind, &laser, g.setting.as_ref())}));
        let ids: Vec<&Value> = g.displays.iter().map(|d| &d["id"]).collect();
        bindings.push(json!({"bindingId": binding, "canvasId": canvas_id, "mode": "LASER_PLANE", "baseProfileId": profile, "displayIds": ids, "patchIds": [patch]}));
        // Shadow bindings keep the other two types for the same elements, as xTool writes them (not for images).
        for k in Kind::ALL.into_iter().filter(|k| *k != g.kind && g.kind != Kind::Bitmap) {
            bindings.push(json!({"bindingId": short_id("binding"), "canvasId": canvas_id, "mode": "LASER_PLANE", "baseProfileId": profile_of(k), "displayIds": [], "shadowOf": binding}));
        }
    }
    let displays: Vec<Value> = groups.into_iter().flat_map(|g| g.displays).collect();
    let elements = displays.len();
    let layers = layer_data.len();
    let profiles: Map<String, Value> = kinds
        .iter()
        .map(|k| (profile_of(*k), json!({"id": profile_of(*k), "processingType": k.name(), "values": k.values(&laser)})))
        .collect();

    // A small cover for xTool's project list (whether it is required is unknown).
    let inches = s.width.max(s.height) / 25.4;
    let cover = super::preview_png(s, 256.0 / inches).ok();
    let project_id = uuid();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let device_id = format!("{code}-1");

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let file = |zip: &mut zip::ZipWriter<_>, name: &str, data: &[u8]| -> anyhow::Result<()> {
        zip.start_file(name, opts)?;
        zip.write_all(data)?;
        Ok(())
    };
    let canvas_dir = format!("canvases/{canvas_id}");
    zip.add_directory("meta/", opts)?;
    file(&mut zip, "meta/persistence-meta.json", br#"{"schemaVersion":"2.0.0","protocol":"xcs-workspace-v2"}"#)?;
    file(&mut zip, "project.json", files::project(&project_id, &canvas_id, &device_id, name, now, cover.is_some()).to_string().as_bytes())?;
    file(&mut zip, "profiles.json", json!({"profiles": profiles}).to_string().as_bytes())?;
    zip.add_directory("canvases/", opts)?;
    file(&mut zip, &format!("{canvas_dir}.json"), files::canvas(&canvas_id, layer_data, elements).to_string().as_bytes())?;
    zip.add_directory(format!("{canvas_dir}/"), opts)?;
    file(&mut zip, &format!("{canvas_dir}/displays-0.json"), json!({"canvasId": canvas_id, "chunkIndex": 0, "displays": displays}).to_string().as_bytes())?;
    zip.add_directory("devices/", opts)?;
    let device = files::device(code, &ext_name, watts, &canvas_id, &profile_ids, patches, bindings);
    file(&mut zip, &format!("devices/device-{device_id}.json"), device.to_string().as_bytes())?;
    if cover.is_some() || !resources.is_empty() {
        zip.add_directory("resources/", opts)?;
    }
    if let Some(png) = &cover {
        file(&mut zip, "resources/project-cover.png.meta.json", files::COVER_META.as_bytes())?;
        file(&mut zip, "resources/project-cover.png", png)?;
    }
    for (name, png) in &resources {
        file(&mut zip, &format!("resources/{name}.meta.json"), files::resource_meta(name).to_string().as_bytes())?;
        file(&mut zip, &format!("resources/{name}"), png)?;
    }
    if !vectors.is_empty() {
        let index: Map<String, Value> = vectors.iter().map(|(h, d)| (h.clone(), json!({"hash": h, "size": d.as_str().unwrap().len()}))).collect();
        zip.add_directory("vectors/", opts)?;
        zip.add_directory("vectors/svg/", opts)?;
        file(&mut zip, "vectors/svg/index.json", json!({"bucketType": "svg", "version": "1.0", "entryCount": index.len(), "entries": index}).to_string().as_bytes())?;
        file(&mut zip, "vectors/svg/data-0.json", json!({"bucketType": "svg", "chunkIndex": 0, "entries": vectors}).to_string().as_bytes())?;
    }
    file(&mut zip, ".format", b"v2")?;
    let bytes = zip.finish()?.into_inner();
    if elements == 0 && skipped.is_empty() {
        bail!("nothing to export: no items on shown cut, score or engrave layers");
    }
    Ok(Xs { bytes, elements, layers, skipped, notes })
}
