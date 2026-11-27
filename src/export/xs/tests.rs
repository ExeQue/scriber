use super::*;
use crate::library::{Material, builtin_machines, xtool::read_project};
use crate::model::layer::Overrides;
use crate::model::{Contour, Document};
use std::io::Read;

fn archive(bytes: &[u8]) -> zip::ZipArchive<std::io::Cursor<&[u8]>> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap()
}

fn read(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Value {
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    serde_json::from_str(&s).unwrap()
}

/// The world bbox of an xTool `dPath` (M L C Z, absolute), cubics sampled finely.
fn path_bbox(d: &str) -> [f64; 4] {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    let mut cur = [0.0, 0.0];
    let mut start = cur;
    let mut cmd = ' ';
    let mut nums: Vec<f64> = Vec::new();
    let flush = |cmd: char, nums: &mut Vec<f64>, cur: &mut [f64; 2], start: &mut [f64; 2], pts: &mut Vec<[f64; 2]>| {
        match cmd {
            'M' | 'L' => {
                *cur = [nums[0], nums[1]];
                if cmd == 'M' {
                    *start = *cur;
                }
                pts.push(*cur);
            }
            'C' => {
                let [p0, p1, p2, p3] = [*cur, [nums[0], nums[1]], [nums[2], nums[3]], [nums[4], nums[5]]];
                for k in 0..=200 {
                    let t = k as f64 / 200.0;
                    let u = 1.0 - t;
                    let f = |i: usize| u * u * u * p0[i] + 3.0 * u * u * t * p1[i] + 3.0 * u * t * t * p2[i] + t * t * t * p3[i];
                    pts.push([f(0), f(1)]);
                }
                *cur = p3;
            }
            'Z' => *cur = *start,
            _ => {}
        }
        nums.clear();
    };
    let spaced: String = d.chars().flat_map(|c| if c.is_ascii_alphabetic() { vec![' ', c, ' '] } else { vec![c] }).collect();
    for tok in spaced.split_whitespace() {
        match tok.parse::<f64>() {
            Ok(v) => nums.push(v),
            Err(_) => {
                flush(cmd, &mut nums, &mut cur, &mut start, &mut pts);
                cmd = tok.chars().next().unwrap();
            }
        }
    }
    flush(cmd, &mut nums, &mut cur, &mut start, &mut pts);
    geom::bbox_of(&pts)
}

fn lib() -> Library {
    let m: Material = serde_json::from_value(json!({"id": "felt", "name": "Felt", "category": "other", "thickness": 3, "settings": [
        {"machine": "xtool-m2", "laser": "diode-10w", "operation": "engrave", "power": 30, "speed": 300, "passes": 1, "line_interval": 0.05}]}))
    .unwrap();
    Library { machines: builtin_machines(), materials: vec![m], ..Default::default() }
}

#[test]
fn round_trip() {
    let mut d = Document::new(200.0, 100.0);
    (d.s.machine, d.s.laser, d.s.material) = (Some("xtool-m2".into()), Some("diode-10w".into()), Some("felt".into()));
    let cut = d.add("plate", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 50.0, 30.0, 0.0)));
    d.add("ring", Op::Score, Shape::path(Contour::circle(100.0, 50.0, 20.0)));
    d.add("square", Op::Engrave, Shape::path(Contour::rect(130.0, 10.0, 20.0, 20.0, 0.0)));
    let text = Shape::Text { x: 10.0, y: 80.0, size: 8.0, text: "Hi".into(), rotation: 0.0, font: "sans-serif".into(), bold: false, italic: false };
    let has_font = !text.laser_contours().is_empty();
    let text_box = text.metrics().bbox;
    d.add("label", Op::Engrave, text);
    let note = d.add("note", Op::NoOutput, Shape::path(Contour::line([0.0, 0.0], [5.0, 5.0])));
    d.add("dim", Op::NoOutput, Shape::Dimension { x1: 0.0, y1: 90.0, x2: 50.0, y2: 90.0, offset: 5.0 });
    let note_layer = d.item(note).unwrap().layer;
    let cut_layer = d.item(cut).unwrap().layer;
    d.s.layer_mut(cut_layer).unwrap().settings = Overrides { power: Some(100.0), speed: Some(4.0), passes: Some(2) };
    d.s.layers.iter_mut().find(|l| l.op == Op::Score).unwrap().settings = Overrides { power: Some(75.0), speed: Some(140.0), passes: Some(1) };
    d.s.layers.iter_mut().find(|l| l.op == Op::Engrave).unwrap().settings.power = Some(80.0);

    let x = xs(&d.s, &lib(), "Test", true).unwrap();
    assert_eq!(x.elements, 4 + has_font as usize, "the no-output note is in, the dimension is not");
    assert_eq!(xs(&d.s, &lib(), "Test", false).unwrap().elements, 3 + has_font as usize, "include_no_output: false leaves the note out");
    assert!(x.skipped.is_empty() && x.notes.is_empty(), "{:?} {:?}", x.skipped, x.notes);

    // Settings, through the library importer.
    let path = std::env::temp_dir().join(format!("scriber-xs-export-{}.xs", std::process::id()));
    std::fs::write(&path, &x.bytes).unwrap();
    let back = read_project(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let set = &back[0].settings;
    assert_eq!(set.iter().filter(|s| s.operation == Operation::Score).count(), 1, "the no-output patch is no material setting: {set:?}");
    let get = |op| set.iter().find(|s| s.operation == op).unwrap();
    let (c, s, e) = (get(Operation::Cut), get(Operation::Score), get(Operation::Engrave));
    assert_eq!((c.power, c.speed, c.passes, c.watts, c.source.as_str()), (100.0, 4.0, 2, 10.0, "blue"));
    assert_eq!((s.power, s.speed, s.passes), (75.0, 140.0, 1));
    assert_eq!((e.power, e.speed, e.passes, e.lines_per_cm), (80.0, 300.0, 1, Some(200.0)), "power from the layer, the rest from the material");

    // Geometry and processing type per element.
    let mut zip = archive(&x.bytes);
    let project = read(&mut zip, "project.json");
    let canvas = project["activeCanvasId"].as_str().unwrap();
    let displays = read(&mut zip, &format!("canvases/{canvas}/displays-0.json"))["displays"].as_array().unwrap().clone();
    let device = read(&mut zip, "devices/device-JS002-1.json");
    let profiles = read(&mut zip, "profiles.json")["profiles"].clone();
    let mode = &device["processing"][canvas]["modes"]["LASER_PLANE"];
    let kind_of = |id: &Value| {
        let b = mode["bindings"].as_array().unwrap().iter().find(|b| b["displayIds"].as_array().unwrap().contains(id)).unwrap();
        profiles[b["baseProfileId"].as_str().unwrap()]["processingType"].as_str().unwrap().to_string()
    };
    let close = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.01);
    let expect = [("VECTOR_CUTTING", [10.0, 10.0, 50.0, 30.0]), ("VECTOR_ENGRAVING", [80.0, 30.0, 40.0, 40.0]), ("FILL_VECTOR_ENGRAVING", [130.0, 10.0, 20.0, 20.0])];
    for (kind, bbox) in expect {
        let el = displays.iter().find(|el| kind_of(&el["id"]) == kind && close(path_bbox(el["dPath"].as_str().unwrap()), bbox));
        let el = el.unwrap_or_else(|| panic!("no {kind} element at {bbox:?}"));
        let written = ["x", "y", "width", "height"].map(|k| el[k].as_f64().unwrap());
        assert!(close(written, bbox), "{kind}: {written:?}");
        assert_eq!(el["isFill"], kind == "FILL_VECTOR_ENGRAVING");
    }
    if has_font {
        let t = displays.iter().find(|el| el["isCompoundPath"] == true).unwrap();
        assert_eq!((kind_of(&t["id"]).as_str(), &t["fillRule"]), ("FILL_VECTOR_ENGRAVING", &json!("evenodd")));
        assert!(close(path_bbox(t["dPath"].as_str().unwrap()), text_box), "{:?} vs {text_box:?}", path_bbox(t["dPath"].as_str().unwrap()));
    }
    let layer_data = read(&mut zip, &format!("canvases/{canvas}.json"))["layerData"].clone();
    assert_eq!(layer_data.as_object().unwrap().len(), x.layers);
    assert_eq!(x.layers, 4, "cut, score, engrave and the no-output layer");

    // The no-output note: on the canvas, ignored, and bound as score with a faint patch and two
    // shadows, as xTool Studio stores "no output"; on its own named layer.
    let ignored = mode["ignoredDisplayIds"].as_array().unwrap();
    assert_eq!(ignored.len(), 1);
    let el = displays.iter().find(|el| el["id"] == ignored[0]).unwrap();
    assert!(close(path_bbox(el["dPath"].as_str().unwrap()), [0.0, 0.0, 5.0, 5.0]));
    let bindings = mode["bindings"].as_array().unwrap();
    let bases: Vec<&Value> = bindings.iter().filter(|b| b["displayIds"].as_array().unwrap().contains(&ignored[0])).collect();
    assert_eq!(bases.len(), 1);
    let base = bases[0];
    assert!(base.get("shadowOf").is_none());
    assert_eq!(profiles[base["baseProfileId"].as_str().unwrap()]["processingType"], "VECTOR_ENGRAVING");
    assert_eq!(base["displayIds"].as_array().unwrap().len(), 1, "only the no-output element");
    let patch = &mode["patches"][base["patchIds"][0].as_str().unwrap()];
    assert_eq!((&patch["profileId"], &patch["overrides"]["power"], &patch["overrides"]["speed"], &patch["overrides"]["repeat"]), (&base["baseProfileId"], &json!(5.0), &json!(300.0), &json!(1)));
    let shadows: Vec<&Value> = bindings.iter().filter(|b| b["shadowOf"] == base["bindingId"]).collect();
    assert_eq!(shadows.len(), 2);
    assert!(shadows.iter().all(|b| b["displayIds"].as_array().unwrap().is_empty()));
    let name = &d.s.layers.iter().find(|l| l.id == note_layer).unwrap().name;
    assert_eq!(layer_data[el["layerTag"].as_str().unwrap()]["name"], json!(name));
}

#[test]
fn same_colour_layers_stay_apart() {
    // Three cut layers with the default cut colour: xTool snaps colours to its palette, so each must
    // get its own palette slot or xTool merges them.
    let mut d = Document::new(100.0, 100.0);
    d.add("a", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 10.0, 10.0, 0.0)));
    let second = d.add_layer("Second cut", Op::Cut);
    d.add_to(second, "b", Shape::path(Contour::rect(30.0, 10.0, 10.0, 10.0, 0.0)));
    let third = d.add_layer("Third cut", Op::Cut);
    d.add_to(third, "c", Shape::path(Contour::rect(50.0, 10.0, 10.0, 10.0, 0.0)));
    let x = xs(&d.s, &lib(), "Triplets", true).unwrap();
    assert_eq!(x.layers, 3);
    let mut zip = archive(&x.bytes);
    let canvas = read(&mut zip, "project.json")["activeCanvasId"].as_str().unwrap().to_string();
    let layer_data = read(&mut zip, &format!("canvases/{canvas}.json"))["layerData"].clone();
    let names: Vec<&str> = layer_data.as_object().unwrap().values().map(|l| l["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Second cut") && names.contains(&"Third cut") && names.len() == 3, "{names:?}");
    let displays = read(&mut zip, &format!("canvases/{canvas}/displays-0.json"))["displays"].as_array().unwrap().clone();
    let tags: Vec<&str> = displays.iter().map(|el| el["layerTag"].as_str().unwrap()).collect();
    assert_eq!(super::hex(tags[0]), Op::Cut.rgb(), "the first layer keeps its colour");
    for el in &displays {
        let tag = el["layerTag"].as_str().unwrap();
        assert!(layer_data.get(tag).is_some());
        assert_eq!(format!("#{:06x}", el["stroke"]["color"].as_u64().unwrap()), tag, "the element's colour is its layer's");
    }
    // Distinct after xTool's snap to its palette.
    let mut slots: Vec<usize> = tags.iter().map(|t| super::slot(super::hex(t))).collect();
    slots.sort();
    slots.dedup();
    assert_eq!(slots.len(), 3, "{tags:?}");
}

#[test]
fn ids_hashes_and_layout() {
    let mut d = Document::new(300.0, 300.0);
    // A long wiggly line: its path string goes to the vector store.
    let pts: Vec<[f64; 2]> = (0..3000).map(|i| [i as f64 * 0.1, 10.0 + (i % 7) as f64 * 0.123457]).collect();
    d.add("long", Op::Cut, Shape::path(Contour::polyline(&pts, false)));
    d.add("photo", Op::Engrave, Shape::Image { image: 999, x: 0.0, y: 0.0, w: 10.0, h: 10.0, rotation: 0.0 });
    let x = xs(&d.s, &lib(), "Ids", true).unwrap();
    assert_eq!(x.skipped.len(), 1, "the image is skipped");
    assert_eq!(x.notes.len(), 2, "no machine, no setting: {:?}", x.notes);

    let mut zip = archive(&x.bytes);
    let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    assert_eq!(names.last().unwrap(), ".format");
    assert!(names.contains(&"meta/".to_string()) && names.contains(&"resources/project-cover.png".to_string()));
    let project = read(&mut zip, "project.json");
    assert!(uuid::Uuid::parse_str(project["projectId"].as_str().unwrap()).is_ok());
    assert_eq!(project["projectId"], project["projectTraceID"]);
    let canvas = project["activeCanvasId"].as_str().unwrap().to_string();
    let short = |v: &Value, prefix: &str| {
        let s = v.as_str().unwrap();
        let rest = s.strip_prefix(prefix).unwrap_or_else(|| panic!("{s}"));
        assert!(rest.len() == 12 && rest.chars().all(|c| c.is_ascii_alphanumeric()), "{s}");
    };
    let profiles = read(&mut zip, "profiles.json")["profiles"].clone();
    assert_eq!(profiles.as_object().unwrap().len(), 3);
    profiles.as_object().unwrap().keys().for_each(|k| short(&json!(k), "profile:"));
    let mode = read(&mut zip, "devices/device-JS002-1.json")["processing"][&canvas]["modes"]["LASER_PLANE"].clone();
    for (k, p) in mode["patches"].as_object().unwrap() {
        short(&json!(k), "patch:");
        assert_eq!(p["source"], "custom");
    }
    let bindings = mode["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 3, "one base binding and two shadows");
    bindings.iter().for_each(|b| short(&b["bindingId"], "binding:"));

    let el = read(&mut zip, &format!("canvases/{canvas}/displays-0.json"))["displays"][0].clone();
    assert!(uuid::Uuid::parse_str(el["id"].as_str().unwrap()).is_ok());
    let hash = el["vectorRef"]["vectorHash"].as_str().unwrap().to_string();
    assert!(el.get("dPath").is_none());
    let data = read(&mut zip, "vectors/svg/data-0.json");
    let index = read(&mut zip, "vectors/svg/index.json");
    let d = data["entries"][&hash].as_str().unwrap();
    assert_eq!(super::hash(d), hash);
    assert_eq!(hash.len(), 64);
    assert_eq!(index["entries"][&hash]["size"], d.len());
    // sha256("abc"), the standard test vector.
    assert_eq!(super::hash("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn images_become_bitmaps() {
    let mut d = Document::new(100.0, 100.0);
    (d.s.machine, d.s.laser, d.s.material) = (Some("xtool-m2".into()), Some("diode-10w".into()), Some("felt".into()));
    let mut png = std::io::Cursor::new(Vec::new());
    image::GrayImage::from_fn(40, 20, |x, _| image::Luma([(x * 6) as u8])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let png = png.into_inner();
    d.place_image(crate::model::Img { png: png.clone(), w: 40, h: 20 }, [10.0, 20.0], Some(40.0), "photo");
    d.s.layers.iter_mut().find(|l| l.op == Op::Engrave).unwrap().settings = Overrides { power: Some(30.0), speed: Some(200.0), passes: Some(1) };
    let x = xs(&d.s, &lib(), "Img", true).unwrap();
    assert!(x.skipped.is_empty() && x.elements == 1, "{:?}", x.skipped);
    // An image on a no-output layer: bound to image engraving (no shadows) and ignored.
    let reference = d.place_image(crate::model::Img { png, w: 40, h: 20 }, [60.0, 20.0], Some(20.0), "reference");
    let no_output = d.s.layers.iter().find(|l| l.op == Op::NoOutput).unwrap().id;
    d.s.items.iter_mut().find(|i| i.id == reference).unwrap().layer = no_output;
    let x2 = xs(&d.s, &lib(), "Img", true).unwrap();
    let mut zip2 = archive(&x2.bytes);
    let canvas2 = read(&mut zip2, "project.json")["activeCanvasId"].as_str().unwrap().to_string();
    let mode2 = read(&mut zip2, "devices/device-JS002-1.json")["processing"][&canvas2]["modes"]["LASER_PLANE"].clone();
    let bindings2 = mode2["bindings"].as_array().unwrap();
    assert_eq!((x2.elements, bindings2.len()), (2, 2), "two bitmap bindings, no shadows");
    let ignored2 = mode2["ignoredDisplayIds"].as_array().unwrap();
    assert_eq!(ignored2.len(), 1);
    assert_eq!(bindings2.iter().filter(|b| b["displayIds"].as_array().unwrap().contains(&ignored2[0])).count(), 1);
    assert_eq!(bindings2[0]["baseProfileId"], bindings2[1]["baseProfileId"]);
    let mut zip = archive(&x.bytes);
    let project = read(&mut zip, "project.json");
    let canvas = project["activeCanvasId"].as_str().unwrap().to_string();
    let el = read(&mut zip, &format!("canvases/{canvas}/displays-0.json"))["displays"][0].clone();
    assert_eq!((el["type"].as_str(), el["originWidth"].as_u64(), el["width"].as_f64(), el["height"].as_f64()), (Some("BITMAP"), Some(40), Some(40.0), Some(20.0)));
    assert_eq!(el["transform"], json!([1.0, 0, 0, 1.0, 10.0, 20.0]));
    let resource = el["resourcePath"].as_str().unwrap().to_string();
    let mut bytes = Vec::new();
    zip.by_name(&resource).unwrap().read_to_end(&mut bytes).unwrap();
    assert_eq!(resource, format!("resources/{}.png", super::hash_bytes(&bytes)));
    assert!(matches!(image::load_from_memory(&bytes).unwrap(), image::DynamicImage::ImageRgba8(_)));
    assert_eq!(read(&mut zip, &format!("{resource}.meta.json"))["ref"], resource);
    let profiles = read(&mut zip, "profiles.json")["profiles"].clone();
    let bitmap = profiles.as_object().unwrap().values().find(|p| p["processingType"] == "BITMAP_ENGRAVING").unwrap()["id"].clone();
    let mode = read(&mut zip, "devices/device-JS002-1.json")["processing"][&canvas]["modes"]["LASER_PLANE"].clone();
    assert_eq!(mode["profileRefs"].as_array().unwrap().len(), 4);
    let bindings = mode["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1, "no shadows for images");
    assert_eq!(bindings[0]["baseProfileId"], bitmap);
    let patch = &mode["patches"][bindings[0]["patchIds"][0].as_str().unwrap()]["overrides"];
    assert_eq!((patch["power"].as_f64(), patch["speed"].as_f64(), patch["bitmapMode"].as_str(), patch["density"].as_f64()), (Some(30.0), Some(200.0), Some("grayscale"), Some(200.0)));

    // The layer's image mode goes into the patch as xTool writes it; the profile keeps greyscale.
    let bitmap_mode = |d: &Document| {
        let bytes = xs(&d.s, &lib(), "Img", true).unwrap().bytes;
        let mut zip = archive(&bytes);
        let canvas = read(&mut zip, "project.json")["activeCanvasId"].as_str().unwrap().to_string();
        let profiles = read(&mut zip, "profiles.json")["profiles"].clone();
        let profile = profiles.as_object().unwrap().values().find(|p| p["processingType"] == "BITMAP_ENGRAVING").unwrap()["values"]["bitmapMode"].clone();
        let mode = read(&mut zip, "devices/device-JS002-1.json")["processing"][&canvas]["modes"]["LASER_PLANE"].clone();
        let patch = mode["bindings"].as_array().unwrap().iter().find(|b| mode["ignoredDisplayIds"].as_array().unwrap().iter().all(|i| !b["displayIds"].as_array().unwrap().contains(i))).unwrap()["patchIds"][0].clone();
        (profile.as_str().unwrap().to_string(), mode["patches"][patch.as_str().unwrap()]["overrides"]["bitmapMode"].as_str().unwrap().to_string())
    };
    let engrave = d.s.layers.iter_mut().find(|l| l.op == Op::Engrave).unwrap();
    engrave.image_mode = Some("floyd".into());
    assert_eq!(bitmap_mode(&d), ("grayscale".into(), "Floyd".into()));
    d.s.layers.iter_mut().find(|l| l.op == Op::Engrave).unwrap().image_mode = Some("passthrough".into());
    assert_eq!(bitmap_mode(&d).1, "passthrough");
    d.s.layers.iter_mut().find(|l| l.op == Op::Engrave).unwrap().image_mode = Some("not-on-this-machine".into());
    assert_eq!(bitmap_mode(&d).1, "grayscale", "a mode the machine lacks: its default");
}

#[test]
fn arcs_become_cubics() {
    let d = path::dpath(&[Contour::circle(0.0, 0.0, 10.0)]);
    assert!(d.starts_with('M') && d.ends_with('Z') && d.contains('C') && !d.contains('A'), "{d}");
    assert!(path_bbox(&d).iter().zip([-10.0, -10.0, 20.0, 20.0]).all(|(a, b)| (a - b).abs() < 0.01), "{d}");
}

#[test]
fn images_lie_under_cut_and_score_lines() {
    // Cut comes before Engrave in the layer list, yet the picture must not cover the cut line.
    let mut d = Document::new(100.0, 100.0);
    (d.s.machine, d.s.laser) = (Some("xtool-m2".into()), Some("diode-10w".into()));
    let cut = d.add("outline", Op::Cut, Shape::path(Contour::circle(50.0, 50.0, 40.0)));
    let mut png = std::io::Cursor::new(Vec::new());
    image::GrayImage::from_pixel(10, 10, image::Luma([255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    d.place_image(crate::model::Img { png: png.into_inner(), w: 10, h: 10 }, [0.0, 0.0], Some(100.0), "fade");
    let score = d.add("score", Op::Score, Shape::path(Contour::circle(50.0, 50.0, 30.0)));
    assert!(cut < score);
    let x = xs(&d.s, &lib(), "Z", true).unwrap();
    let mut zip = archive(&x.bytes);
    let canvas = read(&mut zip, "project.json")["activeCanvasId"].as_str().unwrap().to_string();
    let displays = read(&mut zip, &format!("canvases/{canvas}/displays-0.json"))["displays"].as_array().unwrap().clone();
    let z = |ty: &str| displays.iter().filter(|d| d["type"] == ty).map(|d| d["zOrder"].as_u64().unwrap()).collect::<Vec<_>>();
    let (bitmaps, paths) = (z("BITMAP"), z("PATH"));
    assert_eq!((bitmaps.len(), paths.len()), (1, 2));
    assert!(paths.iter().all(|p| *p > bitmaps[0]), "image {bitmaps:?} under the lines {paths:?}");
}
