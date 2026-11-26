use super::*;
use crate::model::Item;

fn import(svg: &str, opts: &ImportOptions) -> Document {
    let mut d = Document::new(300.0, 200.0);
    d.import_svg(svg.as_bytes(), opts).unwrap();
    d
}

fn near(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

fn bbox_near(a: [f64; 4], b: [f64; 4], eps: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| near(*x, y, eps))
}

#[test]
fn colours_become_laser_operations() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">
        <style>.score { stroke: #0000ff }</style>
        <rect id="plate" x="5" y="5" width="40" height="20" rx="3" fill="none" stroke="#e00"/>
        <circle class="score" cx="70" cy="15" r="8" fill="none"/>
        <path d="M5 40 L45 40" stroke="#333"/>
        <path d="M50 30 H90 V48 H50 Z M60 35 H80 V43 H60 Z" fill="#000"/>
        <g transform="translate(10 0)"><line x1="0" y1="45" x2="20" y2="45" stroke="orange"/></g>
    </svg>"##;
    let d = import(svg, &ImportOptions::default());
    let ops: Vec<(Op, bool)> = d.s.items.iter().map(|i| (i.op, i.fill)).collect();
    assert_eq!(ops, [(Op::Cut, false), (Op::Score, false), (Op::Engrave, false), (Op::NoLine, true), (Op::Cut, false)]);
    assert_eq!(d.selection.len(), 5);

    let [plate, circle, _, compound, orange] = &d.s.items[..] else { unreachable!() };
    assert_eq!(plate.name, "plate");
    assert!(bbox_near(plate.shape.metrics().bbox, [5.0, 5.0, 40.0, 20.0], 1e-4), "mm units: {:?}", plate.shape.metrics().bbox);
    let rounded = &plate.shape.contours()[0];
    assert!(rounded.closed && rounded.vertices.iter().filter(|v| v.bulge != 0.0).count() == 4, "corners are arcs: {rounded:?}");
    assert!(circle.shape.contours()[0].vertices.len() <= 4, "a circle is a few arcs");
    assert!(near(circle.shape.metrics().area.unwrap(), std::f64::consts::PI * 64.0, 0.05));
    assert_eq!(compound.shape.contours().len(), 2, "the hole stays part of the compound path");
    assert!(near(compound.shape.metrics().area.unwrap(), 40.0 * 18.0 - 20.0 * 8.0, 1e-3));
    assert!(bbox_near(orange.shape.metrics().bbox, [10.0, 45.0, 20.0, 0.0], 1e-4), "group transforms apply");
}

#[test]
fn scale_width_and_position() {
    // Plain numbers are px at 96 per inch: 96 px = 25.4 mm.
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"><rect x="10" y="10" width="96" height="48" fill="none" stroke="red"/></svg>"#;
    let b = |opts: ImportOptions| import(svg, &opts).bbox(&[1]).unwrap();
    let mm = 25.4 / 96.0;
    assert!(bbox_near(b(ImportOptions::default()), [10.0 * mm, 10.0 * mm, 25.4, 12.7], 1e-4));
    assert!(bbox_near(b(ImportOptions { scale: 96.0 / 72.0, ..Default::default() }), [10.0 / 72.0 * 25.4, 10.0 / 72.0 * 25.4, 96.0 / 72.0 * 25.4, 48.0 / 72.0 * 25.4], 1e-4));
    assert!(bbox_near(b(ImportOptions { x: Some(5.0), y: Some(7.0), width: Some(100.0), ..Default::default() }), [5.0, 7.0, 100.0, 50.0], 1e-4));

    let mut d = Document::new(100.0, 100.0);
    assert!(d.import_svg(b"<svg", &ImportOptions::default()).is_err());
    assert!(d.import_svg(svg.as_bytes(), &ImportOptions { tolerance: 0.0, ..Default::default() }).is_err());
    assert!(d.s.items.is_empty(), "nothing is added on an error");
}

#[test]
fn our_own_svg_export_round_trips() {
    let mut d = Document::new(200.0, 150.0);
    d.add("rect", Op::Cut, false, Shape::path(Contour::rect(10.0, 10.0, 30.0, 20.0, 0.0)));
    d.add("coaster", Op::Cut, true, Shape::path(Contour::rect(50.0, 10.0, 40.0, 40.0, 6.0)));
    d.add("circle", Op::Score, false, Shape::path(Contour::circle(120.0, 30.0, 15.0)));
    d.add("arc", Op::Engrave, false, Shape::path(Contour::arc(30.0, 80.0, 12.0, 30.0, 250.0)));
    let plate = Shape::Path { contours: vec![Contour::rect(60.0, 70.0, 50.0, 30.0, 0.0), Contour::circle(85.0, 85.0, 7.0)] };
    d.add("face", Op::NoLine, true, plate);
    d.add("open", Op::Cut, false, Shape::path(Contour::polyline(&[[130.0, 70.0], [150.0, 90.0], [170.0, 70.0]], false)));
    let png = crate::export::image::encode_png(&image::GrayImage::from_fn(4, 2, |x, _| image::Luma([x as u8 * 60]))).unwrap();
    let id = d.place_image(Img { png, w: 4, h: 2 }, [20.0, 110.0], Some(40.0), "photo");
    if let Shape::Image { rotation, .. } = &mut d.item_mut(id).unwrap().shape {
        *rotation = 30.0;
    }

    let back = import(&crate::export::svg(&d.s, false), &ImportOptions::default());
    assert_eq!(back.s.items.len(), d.s.items.len(), "{:#?}", back.s.items);
    for item in &d.s.items {
        let m = item.shape.metrics();
        let twin: Vec<&Item> = back.s.items.iter().filter(|i| i.op == item.op && i.fill == item.fill && bbox_near(i.shape.metrics().bbox, m.bbox, 1e-3)).collect();
        assert_eq!(twin.len(), 1, "{} came back once with the same op, fill and place", item.name);
        let twin = twin[0];
        if let (Shape::Image { x, y, w, h, rotation, .. }, Shape::Image { x: x2, y: y2, w: w2, h: h2, rotation: r2, .. }) = (&item.shape, &twin.shape) {
            assert!([(x, x2), (y, y2), (w, w2), (h, h2), (rotation, r2)].iter().all(|(a, b)| near(**a, **b, 1e-3)), "{:?}", twin.shape);
            continue;
        }
        let tm = twin.shape.metrics();
        assert!(near(tm.length, m.length, 0.01) && near(tm.area.unwrap_or(0.0), m.area.unwrap_or(0.0), 0.05), "{}: {tm:?} vs {m:?}", item.name);
        for (a, b) in item.shape.contours().iter().zip(twin.shape.contours()) {
            assert_eq!((a.closed, a.vertices.len()), (b.closed, b.vertices.len()), "{}: {b:?}", item.name);
            for (p, q) in a.vertices.iter().zip(&b.vertices) {
                assert!(distance(xy(p), xy(q)) < 1e-3 && near(p.bulge, q.bulge, 1e-3), "{}: {p:?} vs {q:?}", item.name);
            }
        }
    }
}

#[test]
fn text_becomes_filled_outlines() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="30mm" viewBox="0 0 100 30"><text x="5" y="20" font-size="12" font-family="sans-serif" fill="black">Hi</text></svg>"#;
    let mut d = Document::new(100.0, 30.0);
    // No fonts on this machine: nothing to test.
    let Ok(ids) = d.import_svg(svg.as_bytes(), &ImportOptions::default()) else { return };
    let item = d.item(ids[0]).unwrap();
    assert!(item.op == Op::NoLine && item.fill && item.shape.contours().len() >= 2, "{item:?}");
    let [x, y, w, h] = item.shape.metrics().bbox;
    assert!(x >= 5.0 && y > 5.0 && x + w < 30.0 && y + h <= 20.5, "{:?}", [x, y, w, h]);
}
