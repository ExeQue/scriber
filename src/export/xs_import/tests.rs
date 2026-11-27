use super::*;
use crate::library::store;
use crate::model::Item;
use serde_json::json;
use std::io::Write;

fn lib() -> Library {
    store::merged(store::LibraryFile::default())
}

/// A display with the fields the importer reads.
fn disp(id: &str, ty: &str, tag: &str, t: [f64; 6], extra: Value) -> Value {
    let mut d = json!({"id": id, "type": ty, "layerTag": tag, "transform": t, "zOrder": 0, "groupTags": []});
    d.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    d
}

/// A project zip: displays on canvas C, device JS002 with `mode` (bindings, patches, ignored), extra files.
fn project(displays: Vec<Value>, layers: Value, mode: Value, extra: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    let profiles = json!({"profiles": {
        "p:cut": {"processingType": "VECTOR_CUTTING", "values": {"power": 1, "speed": 16, "repeat": 1, "laser": "laser_blue_10W"}},
        "p:line": {"processingType": "VECTOR_ENGRAVING", "values": {"power": 1, "speed": 20, "repeat": 1, "laser": "laser_blue_10W"}},
        "p:fill": {"processingType": "FILL_VECTOR_ENGRAVING", "values": {"power": 1, "speed": 80, "repeat": 1, "density": 100, "laser": "laser_blue_10W"}},
        "p:img": {"processingType": "BITMAP_ENGRAVING", "values": {"power": 1, "speed": 80, "repeat": 1, "density": 100, "laser": "laser_blue_10W"}}}});
    let files: Vec<(String, Vec<u8>)> = vec![
        ("project.json".into(), json!({"activeCanvasId": "C", "activeDeviceId": "JS002-1", "modules": {"canvases": ["C"]}}).to_string().into_bytes()),
        ("profiles.json".into(), profiles.to_string().into_bytes()),
        ("canvases/C.json".into(), json!({"id": "C", "layerData": layers}).to_string().into_bytes()),
        ("canvases/C/displays-0.json".into(), json!({"displays": displays}).to_string().into_bytes()),
        ("devices/device-JS002-1.json".into(), json!({"deviceCode": "JS002", "power": [10], "processing": {"C": {"activeMode": "LASER_PLANE", "modes": {"LASER_PLANE": mode}}}}).to_string().into_bytes()),
    ];
    for (name, data) in files.into_iter().chain(extra.into_iter().map(|(n, d)| (n.to_string(), d))) {
        zip.start_file(name, opts).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn near(a: [f64; 4], b: [f64; 4], tol: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

fn by_name<'a>(doc: &'a Document, name: &str) -> &'a Item {
    doc.s.items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("no item {name}"))
}

fn bbox(i: &Item) -> [f64; 4] {
    i.shape.metrics().bbox
}

const ID: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[test]
fn every_element_type_lands_in_place() {
    let png = {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(4, 2, image::Rgba([10, 20, 30, 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    let long = "M0 0L50 0L50 5L0 5Z";
    let vectors = json!({"entries": {"h1": long}}).to_string().into_bytes();
    let (s45, c45) = (std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2);
    let g = |inner: &[&str]| json!({"groupTags": inner});
    let displays = vec![
        disp("path", "PATH", "#fe0002", [1.0, 0.0, 0.0, 1.0, 5.0, 5.0], json!({"name": "path", "dPath": "M0 0L10 0L10 10Z"})),
        // Land.xs style: world dPath under a scale and shift.
        disp("scaled", "PATH", "#fe0002", [0.5, 0.0, 0.0, 1.0, 10.0, -3.0], json!({"name": "scaled", "dPath": "M100 100L120 100"})),
        disp("long", "PATH", "#fe0002", [1.0, 0.0, 0.0, 1.0, 0.0, 200.0], json!({"name": "long", "vectorRef": {"vectorHash": "h1"}})),
        disp("line", "LINE", "#fe0002", [0.5, 0.0, 0.0, 1.0, 0.0, 50.0], json!({"name": "line", "endPoint": {"x": 20, "y": 0}})),
        // angle -90: the box goes up and right from (100, 100).
        disp("rot", "RECT", "#fe0002", [0.0, -1.0, 1.0, 0.0, 100.0, 100.0], json!({"name": "rot", "width": 40, "height": 20, "radius": 0})),
        disp("round", "RECT", "#fe0002", [1.0, 0.0, 0.0, 1.0, 10.0, 100.0], json!({"name": "round", "width": 30, "height": 20, "radius": 5})),
        disp("cornerdata", "RECT", "#fe0002", [1.0, 0.0, 0.0, 1.0, 50.0, 100.0], json!({"name": "cornerdata", "width": 30, "height": 20,
            "primitiveCornerData": "{\"defaultCorner\":{\"desiredRadius\":10,\"style\":\"round\"}}"})),
        // The CIRCLE oddity: scale 0.01 in the transform, width/height in world mm, turned -45°.
        disp("ellipse", "CIRCLE", "#fe0002", [0.01 * c45, -0.01 * s45, 0.01 * s45, 0.01 * c45, 200.0, 50.0], json!({"name": "ellipse", "width": 32, "height": 44})),
        // Turned 90° clockwise about its top-left.
        disp("tri", "REGULAR_POLYGON", "#fe0002", [0.0, 1.0, -1.0, 0.0, 300.0, 10.0],
            json!({"name": "tri", "count": 3, "oWidth": 65.588406, "oHeight": 75.462329, "width": 56.801226, "height": 56.596747})),
        disp("star", "STAR", "#fe0002", [1.0, 0.0, 0.0, 1.0, 241.253742, 90.10047],
            json!({"name": "star", "count": 5, "starInnerScale": 0.382, "oWidth": 70.723258, "oHeight": 75.357199, "width": 67.261816, "height": 68.161227})),
        disp("text", "TEXT", "#00befe", [2.0, 0.0, 0.0, 2.0, 10.0, 150.0], json!({"text": "Hi", "charJSONs": [
            disp("c1", "PATH", "", [2.0, 0.0, 0.0, 2.0, 10.0, 150.0], json!({"dPath": "M0 0L0 -10L5 -10L5 0Z"})),
            disp("c2", "PATH", "", [2.0, 0.0, 0.0, 2.0, 22.0, 150.0], json!({"dPath": "M0 0L0 -10L2 -10L2 0Z"}))]})),
        disp("img", "BITMAP", "#00befe", [0.5, 0.0, 0.0, 0.5, 10.0, 20.0], json!({"resourcePath": "resources/a.png", "originWidth": 4, "originHeight": 2})),
        disp("fill", "RECT", "#00befe", ID, json!({"name": "fill", "width": 10, "height": 10, "offsetX": 0})),
        disp("ref", "LINE", "#00befe", [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], json!({"name": "ref", "endPoint": {"x": 5, "y": 5}})),
        disp("nested1", "RECT", "#fe0002", [1.0, 0.0, 0.0, 1.0, 0.0, 250.0], {
            let mut v = g(&["g-in", "g-out"]);
            v["name"] = json!("nested1");
            v["width"] = json!(5);
            v["height"] = json!(5);
            v
        }),
        disp("nested2", "RECT", "#fe0002", [1.0, 0.0, 0.0, 1.0, 10.0, 250.0], json!({"name": "nested2", "width": 5, "height": 5, "groupTags": ["g-in", "g-out"]})),
        disp("outer", "RECT", "#fe0002", [1.0, 0.0, 0.0, 1.0, 20.0, 250.0], json!({"name": "outer", "width": 5, "height": 5, "groupTags": ["g-out"]})),
        disp("weird", "QR", "#fe0002", ID, json!({})),
    ];
    let mode = json!({
        "ignoredDisplayIds": ["ref"],
        "patches": {
            "pc": {"profileId": "p:cut", "overrides": {"power": 80, "speed": 5, "repeat": 3}},
            "pf": {"profileId": "p:fill", "overrides": {"power": 40, "speed": 200, "density": 50}}},
        "bindings": [
            {"bindingId": "b1", "baseProfileId": "p:cut", "patchIds": ["pc"], "displayIds": ["path", "scaled", "long", "line", "rot", "round", "cornerdata", "ellipse", "tri", "star", "nested1", "nested2", "outer", "weird"]},
            {"bindingId": "s1", "baseProfileId": "p:line", "shadowOf": "b1", "displayIds": ["path"]},
            // An image also listed in a vector binding: its bitmap binding wins.
            {"bindingId": "b2", "baseProfileId": "p:line", "displayIds": ["text", "img", "ref"]},
            {"bindingId": "b3", "baseProfileId": "p:img", "displayIds": ["img"]},
            {"bindingId": "b4", "baseProfileId": "p:fill", "patchIds": ["pf"], "displayIds": ["fill"]}]});
    let layers = json!({"#fe0002": {"name": "Red", "order": 2}, "#00befe": {"name": "#00BEFE", "order": 1, "visible": true}});
    let bytes = project(displays, layers, mode, vec![("resources/a.png", png), ("vectors/svg/data-0.json", vectors)]);
    let r = read(bytes, &lib()).unwrap();
    let d = &r.doc;

    assert!(near(bbox(by_name(d, "path")), [5.0, 5.0, 10.0, 10.0], 1e-9));
    assert!(near(bbox(by_name(d, "scaled")), [60.0, 97.0, 10.0, 0.0], 1e-9));
    assert!(near(bbox(by_name(d, "long")), [0.0, 200.0, 50.0, 5.0], 1e-9));
    assert!(near(bbox(by_name(d, "line")), [0.0, 50.0, 10.0, 0.0], 1e-9));
    assert!(near(bbox(by_name(d, "rot")), [100.0, 60.0, 20.0, 40.0], 1e-9), "{:?}", bbox(by_name(d, "rot")));
    let round = by_name(d, "round").shape.metrics();
    assert!(near(round.bbox, [10.0, 100.0, 30.0, 20.0], 1e-3));
    let perimeter = 2.0 * (30.0 + 20.0) - 8.0 * 5.0 + std::f64::consts::TAU * 5.0;
    assert!((round.length - perimeter).abs() < 0.01, "{}", round.length);
    assert!(by_name(d, "round").shape.contours()[0].vertices.iter().any(|v| v.bulge != 0.0), "corners are arcs");
    assert!(by_name(d, "cornerdata").shape.metrics().length < 99.0, "radius from primitiveCornerData");
    // Rotated ellipse: half extents sqrt((16² + 22²) / 2) about the turned centre.
    let e = bbox(by_name(d, "ellipse"));
    let half = ((16.0f64.powi(2) + 22.0f64.powi(2)) / 2.0).sqrt();
    let centre = geometry::apply(&[c45, -s45, s45, c45, 200.0, 50.0], [16.0, 22.0]);
    assert!(near(e, [centre[0] - half, centre[1] - half, 2.0 * half, 2.0 * half], 0.005), "{e:?}");
    // The triangle's tight box (centred in the nominal box, at its top) turned 90° about (300, 10).
    let (ow, w, h) = (65.588406, 56.801226, 56.596747);
    assert!(near(bbox(by_name(d, "tri")), [300.0 - h, 10.0 + (ow - w) / 2.0, h, w], 1e-3), "{:?}", bbox(by_name(d, "tri")));
    assert_eq!(by_name(d, "tri").shape.contours()[0].vertices.len(), 3);
    assert!(near(bbox(by_name(d, "star")), [242.984463, 90.10047, 67.261816, 68.161227], 1e-3));
    assert_eq!(by_name(d, "star").shape.contours()[0].vertices.len(), 10);

    let text = by_name(d, "Hi");
    assert_eq!(text.shape.contours().len(), 2);
    assert!(near(bbox(text), [10.0, 130.0, 16.0, 20.0], 1e-9), "{:?}", bbox(text));
    let img = d.s.items.iter().find(|i| matches!(i.shape, Shape::Image { .. })).unwrap();
    let Shape::Image { image, x, y, w, h, rotation } = img.shape else { unreachable!() };
    assert!(near([x, y, w, h], [10.0, 20.0, 2.0, 1.0], 1e-9) && rotation == 0.0);
    assert_eq!((d.s.images[&image].w, d.s.images[&image].h), (4, 2));

    // Layers: blue (order 1) before red; the blue colour splits by type, the ignored line has its own.
    let layer = |name: &str| d.s.layers.iter().find(|l| l.name == name).unwrap_or_else(|| panic!("no layer {name} in {:?}", d.s.layers.iter().map(|l| &l.name).collect::<Vec<_>>()));
    let names: Vec<&str> = d.s.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["#00BEFE (Score)", "#00BEFE (Engrave)", "#00BEFE (Image)", "#00BEFE (No output)", "Red"]);
    let red = layer("Red");
    assert_eq!((red.op, red.color, red.settings), (Op::Cut, Some([0xfe, 0, 2]), Overrides { power: Some(80.0), speed: Some(5.0), passes: Some(3) }));
    let fill = layer("#00BEFE (Engrave)");
    assert_eq!((fill.op, fill.settings.power, fill.line_interval), (Op::Engrave, Some(40.0), Some(0.2)));
    assert_eq!(layer("#00BEFE (Image)").op, Op::Engrave);
    assert!(layer("#00BEFE (Score)").settings.is_empty(), "no patch: the library's preset");
    assert_eq!(layer("#00BEFE (No output)").op, Op::NoOutput);
    assert_eq!(by_name(d, "ref").layer, layer("#00BEFE (No output)").id);
    assert_eq!(r.layers.iter().map(|l| l.items).sum::<usize>(), d.s.items.len());

    // Nested groups become the outermost one.
    let ids: Vec<u64> = ["nested1", "nested2", "outer"].iter().map(|n| by_name(d, n).id).collect();
    assert_eq!(d.s.groups, vec![ids]);
    assert_eq!(r.skipped.len(), 1, "{:?}", r.skipped);
    assert!(r.skipped[0].contains("QR"));
    // The M2 from the device code; the material is its work area.
    assert_eq!((d.s.machine.as_deref(), d.s.laser.as_deref()), (Some("xtool-m2"), Some("diode-10w")));
    let m2 = lib().machine("xtool-m2").unwrap().area(Some("diode-10w"));
    assert!(d.s.width >= m2[0] && d.s.height >= m2[1] && d.s.width >= 320.0, "{} × {} for {m2:?}", d.s.width, d.s.height);
}

#[test]
fn unknown_device_sizes_the_material_to_the_drawing() {
    let mut bytes = project(vec![disp("a", "RECT", "#fe0002", [1.0, 0.0, 0.0, 1.0, 20.0, 30.0], json!({"width": 40, "height": 10}))], json!({}), json!({}), vec![]);
    // Rewrite the device code to one no machine has.
    let r = {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            let name = f.name().unwrap().to_string();
            out.start_file(name.clone(), zip::write::SimpleFileOptions::default()).unwrap();
            out.write_all(&if name.starts_with("devices/") { String::from_utf8(data).unwrap().replace("JS002", "ZZ999").into_bytes() } else { data }).unwrap();
        }
        bytes = out.finish().unwrap().into_inner();
        read(bytes, &lib()).unwrap()
    };
    assert_eq!((r.doc.s.width, r.doc.s.height), (70.0, 50.0));
    assert!(r.doc.s.machine.is_none() && r.notes.iter().any(|n| n.contains("ZZ999")));
    // Not bound to any processing: no output, and said so.
    assert_eq!(r.doc.s.layers[0].op, Op::NoOutput);
    assert!(r.notes.iter().any(|n| n.contains("no processing type")));
    assert!(read(b"not a zip".to_vec(), &lib()).is_err());
}

#[test]
fn round_trip_through_the_exporter() {
    let lib = lib();
    let mut d = Document::new(400.0, 300.0);
    let (cut, score, engrave) = (d.layer_for(Op::Cut), d.layer_for(Op::Score), d.layer_for(Op::Engrave));
    d.s.layer_mut(cut).unwrap().settings = Overrides { power: Some(100.0), speed: Some(4.0), passes: Some(2) };
    d.s.layer_mut(score).unwrap().settings = Overrides { power: Some(75.0), speed: Some(140.0), passes: Some(1) };
    let e = d.s.layer_mut(engrave).unwrap();
    (e.settings, e.line_interval) = (Overrides { power: Some(30.0), speed: Some(200.0), passes: Some(1) }, Some(0.05));
    d.add_to(cut, "rect", Shape::path(Contour::rect(10.0, 10.0, 80.0, 50.0, 6.0)));
    d.add_to(cut, "circle", Shape::path(Contour::circle(150.0, 60.0, 37.5)));
    d.add_to(score, "hexagon", Shape::path(Contour::polygon(250.0, 60.0, 30.0, 6, 17.0)));
    d.add_to(score, "arc", Shape::path(Contour::arc(80.0, 200.0, 40.0, 10.0, 250.0)));
    d.add_to(engrave, "plate", Shape::Path { contours: vec![Contour::rect(200.0, 150.0, 100.0, 80.0, 0.0), Contour::circle(250.0, 190.0, 20.0)] });
    let png = {
        let mut out = std::io::Cursor::new(Vec::new());
        image::GrayImage::from_pixel(30, 20, image::Luma([128])).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    let id = d.place_image(crate::export::image::to_grey(&png).unwrap(), [20.0, 120.0], Some(60.0), "photo");
    d.move_to_layer(&[id], engrave).unwrap();

    let xs = crate::export::xs(&d.s, &lib, "rt", true).unwrap();
    let r = read(xs.bytes, &lib).unwrap();
    let key = |s: &crate::model::State, i: &Item| {
        let l = s.layer(i.layer).unwrap();
        (l.op, l.settings, l.line_interval, i.shape.metrics().bbox)
    };
    let mut a: Vec<_> = d.s.items.iter().map(|i| key(&d.s, i)).collect();
    let mut b: Vec<_> = r.doc.s.items.iter().map(|i| key(&r.doc.s, i)).collect();
    a.sort_by(|x, y| x.3[0].total_cmp(&y.3[0]));
    b.sort_by(|x, y| x.3[0].total_cmp(&y.3[0]));
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!((x.0, x.1), (y.0, y.1));
        assert!(near(x.3, y.3, 0.01), "{:?} vs {:?}", x.3, y.3);
        if let Some(i) = x.2 {
            assert!((y.2.unwrap() - i).abs() < 1e-3, "{x:?} {y:?}");
        }
    }
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
}
