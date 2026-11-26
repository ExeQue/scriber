//! The `.scrbr` project file: a zip with `document.json` and the images as `images/<id>.png`.

use super::{Document, Img, State};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

/// Bump when the JSON changes in a way older versions cannot read.
pub const FORMAT_VERSION: u32 = 1;

impl Document {
    pub fn save(&mut self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("scrbr.tmp");
        write_archive(&self.s, &tmp)?;
        // Write-then-rename, so a crash never leaves a half-written project.
        std::fs::rename(&tmp, path)?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.rev += 1;
        Ok(())
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        Ok(Self { s: read_archive(path)?, path: Some(path.to_path_buf()), ..Self::default() })
    }
}

fn write_archive(s: &State, path: &Path) -> anyhow::Result<()> {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path)?);
    let opts = zip::write::SimpleFileOptions::default();
    let mut json = serde_json::to_value(s)?;
    json["format"] = FORMAT_VERSION.into();
    json["app"] = "scriber".into();
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

fn read_archive(path: &Path) -> anyhow::Result<State> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let mut json = String::new();
    zip.by_name("document.json")?.read_to_string(&mut json)?;
    let value: serde_json::Value = serde_json::from_str(&json)?;
    let format = value["format"].as_u64().unwrap_or(0);
    anyhow::ensure!(format <= FORMAT_VERSION as u64, "this file was made by a newer Scriber (format {format}); please update");
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
    Ok(s)
}

#[cfg(test)]
mod tests {
    use crate::model::{Contour, Document, Op, Shape};

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("scriber-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.scrbr");
        let mut d = Document::new(120.0, 80.0);
        d.s.notes = "hej".into();
        let img = crate::export::image::to_grey(&crate::export::image::encode_png(&image::GrayImage::new(3, 2)).unwrap()).unwrap();
        d.place_image(img, [1.0, 2.0], Some(30.0), "pic");
        d.add("c", Op::Cut, true, Shape::path(Contour::circle(50.0, 40.0, 10.0)));
        d.save(&path).unwrap();
        let e = Document::open(&path).unwrap();
        assert_eq!(e.s.items, d.s.items);
        assert_eq!(e.s.notes, "hej");
        assert_eq!(e.s.next_id, d.s.next_id);
        assert_eq!(e.s.images.values().next().unwrap().w, 3);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn old_files_load_without_machine_and_material() {
        let s: crate::model::State = serde_json::from_str(r#"{"width": 10, "height": 20, "items": [], "next_id": 1}"#).unwrap();
        assert_eq!((s.machine, s.laser, s.material), (None, None, None));
    }
}
