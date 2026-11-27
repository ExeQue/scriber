//! xTool Studio project (`.xs`) export: one canvas of `PATH` elements, one colour layer per Scriber
//! layer, and per layer and processing type a binding with the layer's power, speed and passes.
//! The format is described in docs/xs-format.md.

mod files;
mod path;
#[cfg(test)]
mod tests;

use crate::library::{Library, Operation, Setting};
use crate::model::layer::op_operation;
use crate::model::{Op, Shape, State, geom};
use anyhow::{Context, bail};
use files::{Kind, Outline};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;

/// Longer path strings go to the `vectors/svg` store, as xTool does with long ones (on saving it moved
/// a 14 KB path and kept 2.5 KB ones inline; its exact threshold is unknown).
const INLINE_MAX: usize = 8 * 1024;

/// The finished project and what went into it.
pub struct Xs {
    pub bytes: Vec<u8>,
    /// Elements written, no-output reference ones included.
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
    /// Images: the layer's image mode as xTool writes it (`bitmapMode`, e.g. "Bayer"); None = xTool's
    /// default, greyscale.
    bitmap_mode: Option<String>,
    /// A no-output layer: bound like any layer but listed in `ignoredDisplayIds`.
    ignored: bool,
}

/// The patch of a no-output layer, as xTool Studio keeps one when output is switched off: a faint,
/// fast single pass (5 %, 300 mm/s, as in a project xTool re-saved), harmless if someone turns
/// output back on in xTool.
fn no_output_setting(kind: Kind) -> Setting {
    let op = if kind == Kind::Bitmap { Operation::Engrave } else { Operation::Score };
    Setting { power: 5.0, speed: 300.0, passes: 1, ..Setting::new("", "", op) }
}

/// The xTool device code, its short name, the module string ("laser_blue_10W") and watts.
fn machine(s: &State, lib: &Library, notes: &mut Vec<String>) -> anyhow::Result<(String, String, String, f64)> {
    let Some(id) = s.machine.as_deref() else {
        notes.push("No machine chosen: written for the xTool M2 with the 10 W blue laser.".into());
        return Ok(("JS002".into(), "M2".into(), "laser_blue_10W".into(), 10.0));
    };
    let m = lib.machine(id).with_context(|| format!("unknown machine {id}"))?;
    let code = m.xtool_device().with_context(|| format!("{} has no xTool device code; set the xTool device code for this machine in Machines & materials", m.name))?;
    let laser = s.laser.as_deref().and_then(|l| m.laser(l)).or(m.lasers.first()).with_context(|| format!("{} has no laser module", m.name))?;
    let name = m.xtool.as_ref().map(|x| x.name.trim()).filter(|n| !n.is_empty()).unwrap_or(&m.model);
    Ok((code.to_string(), name.to_string(), laser.xtool_module(), laser.power_w))
}

/// The patch overrides: xTool's defaults for the type with the layer's numbers (and image mode) on top.
fn overrides(kind: Kind, laser: &str, s: Option<&Setting>, bitmap_mode: Option<&str>) -> Map<String, Value> {
    let mut v = kind.values(laser);
    if let (Kind::Bitmap, Some(mode)) = (kind, bitmap_mode) {
        v.insert("bitmapMode".into(), json!(mode));
    }
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

/// xTool Studio's layer palette, in its order (`router.*.js` in xTool Studio 1.9.11: the map of
/// layer colours to index 1-16). On open xTool snaps every layer colour to the nearest of these
/// (squared RGB distance, first wins on a tie), so two near colours become one layer.
const PALETTE: [[u8; 3]; 16] = [
    [0xeb, 0x3d, 0xba], [0xfe, 0x00, 0x02], [0xff, 0x7f, 0x56], [0xe1, 0xc0, 0x00],
    [0xc2, 0x99, 0x00], [0x96, 0xd7, 0x1d], [0x00, 0xc7, 0x15], [0x00, 0x89, 0x7b],
    [0x23, 0x66, 0xff], [0x00, 0xbe, 0xfe], [0x81, 0x70, 0xef], [0xa9, 0x58, 0xff],
    [0x58, 0x2f, 0xa8], [0xd9, 0xd9, 0xd9], [0x84, 0x8b, 0x96], [0x00, 0x00, 0x00],
];

fn hex(tag: &str) -> [u8; 3] {
    let v = u32::from_str_radix(&tag[1..], 16).unwrap_or(0);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    (0..3).map(|i| (a[i] as i32 - b[i] as i32).pow(2)).sum()
}

/// The palette slot xTool snaps `rgb` to.
fn slot(rgb: [u8; 3]) -> usize {
    (0..PALETTE.len()).min_by_key(|&i| distance(rgb, PALETTE[i])).unwrap()
}

/// The colour a layer is written with: its own when no earlier layer snaps to the same palette slot,
/// else the nearest palette colour no earlier layer holds. None when all 16 slots are taken.
fn layer_colour(rgb: [u8; 3], taken: &[usize]) -> Option<[u8; 3]> {
    if !taken.contains(&slot(rgb)) {
        return Some(rgb);
    }
    (0..PALETTE.len()).filter(|i| !taken.contains(i)).min_by_key(|&i| distance(rgb, PALETTE[i])).map(|i| PALETTE[i])
}

/// The document as an xTool Studio project. Every shown layer becomes its own xTool colour layer
/// (xTool snaps layer colours to its 16-colour palette, so a layer whose colour lands on an
/// earlier layer's palette slot gets the nearest free palette colour). No-output layers go in
/// as reference unless `include_no_output` is false: bound as score (images as image engraving) with
/// a faint patch and listed in `ignoredDisplayIds`, as xTool Studio stores "no output";
/// hidden layers, guides and dimensions are left out; text goes out as its glyph outlines; images
/// are engraved with their layer's image mode (the machine's default, greyscale for xTool, when it
/// has none; rotated ones are skipped and listed in `skipped`).
pub fn xs(s: &State, lib: &Library, name: &str, include_no_output: bool) -> anyhow::Result<Xs> {
    let mut notes = Vec::new();
    let mut skipped = Vec::new();
    let (code, ext_name, laser, watts) = machine(s, lib, &mut notes)?;
    let canvas_id = uuid();
    let mut groups: Vec<Group> = Vec::new();
    let mut layer_data = Map::new();
    let mut vectors = Map::new();
    let mut resources = std::collections::BTreeMap::new();
    // Images go under every vector: an opaque picture (white where nothing burns) would hide the cut
    // and score lines in xTool Studio's canvas, which draws higher zOrder on top. Within each kind the
    // layer order stays.
    let mut z_image = 0;
    let mut z = s.items.iter().filter(|i| matches!(i.shape, Shape::Image { .. })).count();
    for layer in s.layers.iter().filter(|l| l.visible && (include_no_output || l.op != Op::NoOutput)) {
        let taken: Vec<usize> = layer_data.keys().map(|t| slot(hex(t))).collect();
        let [r, g, b] = layer_colour(layer.rgb(), &taken).unwrap_or_else(|| {
            notes.push(format!("Layer {}: xTool Studio has 16 layer colours, all in use; it shares one with another layer.", layer.name));
            layer.rgb()
        });
        let tag = format!("#{r:02x}{g:02x}{b:02x}");
        let lasered = layer.op != Op::NoOutput;
        for item in s.items.iter().filter(|i| i.layer == layer.id) {
            let name = if item.name.is_empty() { item.id.to_string() } else { format!("{} ({})", item.name, item.id) };
            let (kind, display) = if let Shape::Image { image, x, y, w, h, rotation } = item.shape {
                if rotation.abs() > 1e-9 {
                    if lasered {
                        skipped.push(format!("image {name} on layer {}: rotated images are not exported yet", layer.name));
                    }
                    continue;
                }
                let Some(png) = s.images.get(&image).and_then(|img| rgba_png(&img.png)) else {
                    if lasered {
                        skipped.push(format!("image {name} on layer {}: its picture is missing or unreadable", layer.name));
                    }
                    continue;
                };
                let file = format!("{}.png", hash_bytes(&png.0));
                z_image += 1;
                let display = files::bitmap(&uuid(), &tag, [r, g, b], z_image, &format!("resources/{file}"), png.1, [x, y, w, h]);
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
                    // No output too is bound as score, as xTool does.
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
            layer_data.entry(tag.clone()).or_insert_with(|| json!({"name": layer.name, "order": n, "visible": true}));
            match groups.iter_mut().find(|g| g.layer == layer.id && g.kind == kind) {
                Some(g) => g.displays.push(display),
                None if !lasered => groups.push(Group { layer: layer.id, kind, displays: vec![display], setting: Some(no_output_setting(kind)), bitmap_mode: None, ignored: true }),
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
                    let bitmap_mode = (kind == Kind::Bitmap).then(|| layer.image_mode(s, lib)).flatten().map(|m| m.value);
                    groups.push(Group { layer: layer.id, kind, displays: vec![display], setting, bitmap_mode, ignored: false });
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
        patches.insert(patch.clone(), json!({"id": patch, "profileId": profile, "source": "custom", "overrides": overrides(g.kind, &laser, g.setting.as_ref(), g.bitmap_mode.as_deref())}));
        let ids: Vec<&Value> = g.displays.iter().map(|d| &d["id"]).collect();
        bindings.push(json!({"bindingId": binding, "canvasId": canvas_id, "mode": "LASER_PLANE", "baseProfileId": profile, "displayIds": ids, "patchIds": [patch]}));
        // Shadow bindings keep the other two types for the same elements, as xTool writes them (not for images).
        for k in Kind::ALL.into_iter().filter(|k| *k != g.kind && g.kind != Kind::Bitmap) {
            bindings.push(json!({"bindingId": short_id("binding"), "canvasId": canvas_id, "mode": "LASER_PLANE", "baseProfileId": profile_of(k), "displayIds": [], "shadowOf": binding}));
        }
    }
    let processed = groups.iter().filter(|g| !g.ignored).count();
    let ignored: Vec<Value> = groups.iter().filter(|g| g.ignored).flat_map(|g| g.displays.iter().map(|d| d["id"].clone())).collect();
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
    let device = files::device(&code, &ext_name, watts, &canvas_id, &profile_ids, ignored, patches, bindings);
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
    if processed == 0 && skipped.is_empty() {
        bail!("nothing to export: no items on shown cut, score or engrave layers");
    }
    Ok(Xs { bytes, elements, layers, skipped, notes })
}
