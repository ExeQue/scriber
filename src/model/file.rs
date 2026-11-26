//! The `.scrbr` project file: a zip with `document.json` and the images as `images/<id>.png`.

use super::annotations::Annotations;
use super::{Document, Img, Layer, Op, State, layer};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

/// Bump when the JSON changes in a way older versions cannot read.
pub const FORMAT_VERSION: u32 = 2;

impl Document {
    pub fn save(&mut self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("scrbr.tmp");
        write_archive(&self.s, &self.annotations, &tmp)?;
        // Write-then-rename, so a crash never leaves a half-written project.
        std::fs::rename(&tmp, path)?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.rev += 1;
        Ok(())
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let (s, annotations) = read_archive(path)?;
        Ok(Self { s, annotations: annotations.loaded(), path: Some(path.to_path_buf()), ..Self::default() })
    }
}

fn write_archive(s: &State, annotations: &Annotations, path: &Path) -> anyhow::Result<()> {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path)?);
    let opts = zip::write::SimpleFileOptions::default();
    let mut json = serde_json::to_value(s)?;
    json["format"] = FORMAT_VERSION.into();
    json["app"] = "scriber".into();
    if !annotations.list.is_empty() {
        json["annotations"] = serde_json::to_value(annotations)?;
    }
    zip.start_file("document.json", opts)?;
    zip.write_all(serde_json::to_string_pretty(&json)?.as_bytes())?;
    // PNGs are already compressed.
    let stored = opts.compression_method(zip::CompressionMethod::Stored);
    for (id, img) in &s.images {
        zip.start_file(format!("images/{id}.png"), stored)?;
        zip.write_all(&img.png)?;
    }
    zip.finish()?;
    Ok(())
}

fn read_archive(path: &Path) -> anyhow::Result<(State, Annotations)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let mut json = String::new();
    zip.by_name("document.json")?.read_to_string(&mut json)?;
    let mut value: serde_json::Value = serde_json::from_str(&json)?;
    let format = value["format"].as_u64().unwrap_or(0);
    anyhow::ensure!(format <= FORMAT_VERSION as u64, "this file was made by a newer Scriber (format {format}); please update");
    let annotations = value.as_object_mut().and_then(|o| o.remove("annotations")).map(serde_json::from_value).transpose()?.unwrap_or_default();
    migrate_fill(&mut value);
    migrate_layers(&mut value);
    let mut s: State = serde_json::from_value(value)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name()?.to_string();
        let Some(id) = name.strip_prefix("images/").and_then(|n| n.strip_suffix(".png")) else { continue };
        let id: u64 = id.parse()?;
        let mut png = Vec::new();
        entry.read_to_end(&mut png)?;
        let (w, h) = image::ImageReader::new(std::io::Cursor::new(&png)).with_guessed_format()?.into_dimensions()?;
        s.images.insert(id, Arc::new(Img { png, w, h }));
    }
    Ok((s, annotations))
}

/// Files from before the xTool operations had a line op (`cut`, `score`, `engrave`, `none`, `annotate`)
/// plus a `fill` switch. Now Engrave on a closed shape engraves its area, so:
/// - cut/score with fill: keep it, and add an engrave copy right before it (nothing is lost);
/// - none/engrave with fill: engrave;
/// - none without fill, and annotate: no_output.
///
/// Constraints keep pointing at the original item, not the copy.
fn migrate_fill(doc: &mut serde_json::Value) {
    let stored = doc["next_id"].as_u64().unwrap_or(1);
    let Some(items) = doc.get_mut("items").and_then(|i| i.as_array_mut()) else { return };
    let mut next_id = items.iter().filter_map(|i| i["id"].as_u64()).max().map_or(stored, |m| stored.max(m + 1));
    let mut out = Vec::with_capacity(items.len());
    for mut item in items.drain(..) {
        let fill = item.as_object_mut().and_then(|o| o.remove("fill")).and_then(|f| f.as_bool()).unwrap_or(false);
        let op = item["op"].as_str().unwrap_or("cut").to_owned();
        match (op.as_str(), fill) {
            ("cut" | "score", true) => {
                let mut copy = item.clone();
                copy["id"] = next_id.into();
                copy["op"] = "engrave".into();
                next_id += 1;
                out.push(copy);
            }
            ("none" | "engrave", true) => item["op"] = "engrave".into(),
            ("none" | "annotate", _) => item["op"] = "no_output".into(),
            _ => {}
        }
        out.push(item);
    }
    *items = out;
    doc["next_id"] = next_id.into();
}

/// Files from before layers (format 1) have the op on each item. They get one layer per op used,
/// in the usual order, and each item goes on the layer of its op. A file without items gets the
/// default layers.
fn migrate_layers(doc: &mut serde_json::Value) {
    if doc.get("layers").is_some() {
        return;
    }
    let mut next_id = doc["next_id"].as_u64().unwrap_or(1);
    let mut items = doc.get_mut("items").and_then(|i| i.as_array_mut()).map(std::mem::take).unwrap_or_default();
    let op_of = |item: &mut serde_json::Value| -> Op {
        let op = item.as_object_mut().and_then(|o| o.remove("op")).unwrap_or_default();
        serde_json::from_value(op).unwrap_or_default()
    };
    let ops: Vec<Op> = items.iter_mut().map(op_of).collect();
    let used: Vec<Op> = Op::ALL.into_iter().filter(|op| ops.contains(op)).collect();
    let layers: Vec<Layer> = if used.is_empty() {
        let layers = layer::defaults();
        next_id = next_id.max(layers.len() as u64 + 1);
        layers
    } else {
        used.iter()
            .map(|op| {
                next_id += 1;
                Layer::new(next_id - 1, op.label(), *op)
            })
            .collect()
    };
    for (item, op) in items.iter_mut().zip(ops) {
        item["layer"] = layers.iter().find(|l| l.op == op).map_or(0, |l| l.id).into();
    }
    doc["items"] = items.into();
    doc["layers"] = serde_json::to_value(layers).unwrap();
    doc["next_id"] = next_id.into();
}

#[cfg(test)]
mod tests {
    use crate::model::{Contour, Document, Op, Shape};

    #[test]
    fn old_files_get_one_layer_per_op() {
        let mut v = serde_json::json!({"width": 10, "height": 10, "next_id": 5, "items": [
            {"id": 1, "op": "score", "shape": {"kind": "path", "contours": []}},
            {"id": 2, "op": "cut", "shape": {"kind": "path", "contours": []}},
            {"id": 3, "op": "score", "shape": {"kind": "path", "contours": []}}
        ]});
        super::migrate_layers(&mut v);
        let s: crate::model::State = serde_json::from_value(v).unwrap();
        let layers: Vec<(u64, &str, Op)> = s.layers.iter().map(|l| (l.id, l.name.as_str(), l.op)).collect();
        assert_eq!(layers, [(5, "Cut", Op::Cut), (6, "Score", Op::Score)]);
        assert_eq!(s.items.iter().map(|i| i.layer).collect::<Vec<_>>(), [6, 5, 6]);
        assert_eq!(s.next_id, 7);
        let mut empty = serde_json::json!({"width": 10, "height": 10, "next_id": 1, "items": []});
        super::migrate_layers(&mut empty);
        let s: crate::model::State = serde_json::from_value(empty).unwrap();
        assert_eq!((s.layers.len(), s.next_id), (4, 5));
    }

    #[test]
    fn old_fill_becomes_engrave() {
        let mut v = serde_json::json!({"width": 10, "height": 10, "next_id": 5, "items": [
            {"id": 1, "op": "cut", "fill": true, "shape": {"kind": "path", "contours": []}},
            {"id": 2, "op": "none", "fill": true, "shape": {"kind": "path", "contours": []}},
            {"id": 3, "op": "none", "fill": false, "shape": {"kind": "path", "contours": []}},
            {"id": 4, "op": "annotate", "shape": {"kind": "path", "contours": []}},
            {"id": 6, "op": "score", "fill": false, "shape": {"kind": "path", "contours": []}}
        ]});
        super::migrate_fill(&mut v);
        super::migrate_layers(&mut v);
        let s: crate::model::State = serde_json::from_value(v).unwrap();
        let got: Vec<(u64, Op)> = s.items.iter().map(|i| (i.id, s.op(i))).collect();
        assert_eq!(got, [(7, Op::Engrave), (1, Op::Cut), (2, Op::Engrave), (3, Op::NoOutput), (4, Op::NoOutput), (6, Op::Score)]);
        assert_eq!(got[0].0, 7, "the copy gets a fresh id");
        assert_eq!(s.next_id, 12, "8 to 11 went to the four layers");
    }

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("scriber-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.scrbr");
        let mut d = Document::new(120.0, 80.0);
        d.s.notes = "hej".into();
        let img = crate::export::image::to_grey(&crate::export::image::encode_png(&image::GrayImage::new(3, 2)).unwrap()).unwrap();
        d.place_image(img, [1.0, 2.0], Some(30.0), "pic");
        d.add("c", Op::Cut, Shape::path(Contour::circle(50.0, 40.0, 10.0)));
        d.save(&path).unwrap();
        let e = Document::open(&path).unwrap();
        assert_eq!(e.s.items, d.s.items);
        assert_eq!(e.s.layers, d.s.layers);
        assert_eq!(e.s.notes, "hej");
        assert_eq!(e.s.next_id, d.s.next_id);
        assert_eq!(e.s.images.values().next().unwrap().w, 3);
        assert!(e.annotations.list.is_empty(), "a file without annotations loads with none");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn annotations_round_trip() {
        use crate::model::annotations::{Author, Status, revision};
        let dir = std::env::temp_dir().join(format!("scriber-ann-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.scrbr");
        let mut d = Document::new(120.0, 80.0);
        let id = d.annotate(Author::Person, "rounder", Some([5.0, 6.0]), vec![]).unwrap();
        d.reply_annotation(id, Author::Agent("bot".into()), "ok", Some(true)).unwrap();
        d.save(&path).unwrap();
        let before = revision();
        let mut e = Document::open(&path).unwrap();
        let (a, b) = (&d.annotations.list[0], &e.annotations.list[0]);
        assert_eq!((b.id, b.at, &b.text, b.status, &b.replies, &b.changed_by), (a.id, a.at, &a.text, Status::Resolved, &a.replies, &a.changed_by));
        assert!(b.revision > before, "loaded annotations get fresh revisions");
        assert_eq!(e.annotate(Author::Person, "next", Some([0.0, 0.0]), vec![]).unwrap(), id + 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn old_files_load_without_machine_and_material() {
        let mut v = serde_json::json!({"width": 10, "height": 20, "items": [], "next_id": 1});
        super::migrate_layers(&mut v);
        let s: crate::model::State = serde_json::from_value(v).unwrap();
        assert_eq!((s.machine, s.laser, s.material), (None, None, None));
    }
}
