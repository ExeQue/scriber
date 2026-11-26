use super::import::{Dxf, read};
use super::*;
use crate::model::geom::{Point, segment_distance};
use crate::model::{Contour, Document, Shape};
use std::collections::BTreeMap;

const H: f64 = 300.0;

fn near(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

fn import(d: &mut Document, dxf: &Dxf) -> Vec<u64> {
    d.import_dxf(dxf, &BTreeMap::new(), None).unwrap()
}

/// The items on the layer called `name`, and that layer's op.
fn on_layer<'a>(d: &'a Document, name: &str) -> (Op, Vec<&'a Shape>) {
    let l = d.s.layers.iter().find(|l| l.name == name).unwrap_or_else(|| panic!("no layer {name}: {:?}", d.s.layers));
    (l.op, d.s.items.iter().filter(|i| i.layer == l.id).map(|i| &i.shape).collect())
}

/// Largest distance from the curve `f` (sampled over [t0, t1]) to the contour.
fn deviation(c: &Contour, f: impl Fn(f64) -> Point, t0: f64, t1: f64) -> f64 {
    let poly = c.flatten(1e-5);
    let segs: Vec<_> = if c.closed { poly.iter().chain(poly.first()).copied().collect() } else { poly };
    (0..=500).map(|i| f(t0 + (t1 - t0) * i as f64 / 500.0)).map(|q| segs.windows(2).map(|w| segment_distance(q, w[0], w[1])).fold(f64::MAX, f64::min)).fold(0.0, f64::max)
}

#[test]
fn round_trip_keeps_shapes_and_layers() {
    let mut d = Document::new(200.0, H);
    d.add("rect", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0)));
    d.add("hole", Op::Cut, Shape::path(Contour::circle(80.0, 30.0, 10.0)));
    let arc = Contour::arc(140.0, 50.0, 15.0, 30.0, 300.0);
    d.add("arc", Op::Score, Shape::path(arc.clone()));
    let plate = Shape::Path { contours: vec![Contour::rect(10.0, 50.0, 40.0, 40.0, 5.0), Contour::circle(30.0, 70.0, 8.0)] };
    d.add("plate", Op::Engrave, plate.clone());
    d.add("note", Op::NoOutput, Shape::path(Contour::line([0.0, 0.0], [10.0, 0.0])));
    let out = dxf(&d.s, false).unwrap();
    assert_eq!((out.layers, out.entities), (3, 5));
    let text = String::from_utf8(out.bytes.clone()).unwrap();
    assert!(text.contains("AC1018") && text.contains("$INSUNITS") && text.contains("LWPOLYLINE") && text.contains("CIRCLE"), "{text}");
    assert!(dxf(&d.s, true).unwrap().layers == 4);

    let dx = read(&out.bytes, H, 0.01).unwrap();
    assert_eq!(dx.units, "mm");
    let mut e = Document::new(200.0, H);
    import(&mut e, &dx);
    let (op, cut) = on_layer(&e, "Cut");
    assert_eq!((op, cut.len()), (Op::Cut, 2));
    let bboxes: Vec<_> = cut.iter().map(|s| s.metrics().bbox).collect();
    assert!(bboxes.iter().any(|b| b.iter().zip([10.0, 10.0, 40.0, 20.0]).all(|(a, b)| near(*a, b, 1e-9))), "{bboxes:?}");
    let circle = cut.iter().find_map(|s| s.contours()[0].as_circle()).expect("a circle stays a circle");
    assert!(near(circle.0[0], 80.0, 1e-9) && near(circle.0[1], 30.0, 1e-9) && near(circle.1, 10.0, 1e-9));
    // The 270° arc turns the same way round after both y flips.
    let (op, score) = on_layer(&e, "Score");
    assert_eq!((op, score.len()), (Op::Score, 1));
    let back = &score[0].contours()[0];
    assert!(!back.closed && near(back.length(), arc.length(), 1e-9));
    assert!(deviation(back, |t| [140.0 + 15.0 * t.to_radians().cos(), 50.0 + 15.0 * t.to_radians().sin()], 30.0, 300.0) < 1e-4);
    // The plate and its hole come back as one compound path.
    let (op, engrave) = on_layer(&e, "Engrave");
    assert_eq!((op, engrave.len()), (Op::Engrave, 1));
    assert_eq!(engrave[0].contours().len(), 2);
    assert!(near(engrave[0].metrics().area.unwrap(), plate.metrics().area.unwrap(), 1e-6));
    assert!(e.s.layers.iter().all(|l| l.name != "No output" || e.s.items.iter().all(|i| i.layer != l.id)));
}

#[test]
fn hidden_layers_and_images_stay_out() {
    let mut d = Document::new(100.0, 100.0);
    d.add("a", Op::Cut, Shape::path(Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0)));
    let cut = d.layer_for(Op::Cut);
    d.s.layer_mut(cut).unwrap().visible = false;
    assert!(dxf(&d.s, true).is_ok_and(|o| o.entities == 0));
}

#[test]
fn colours_names_and_skipping() {
    assert_eq!(nearest_aci([255, 0, 0]), 1);
    assert_eq!(nearest_aci([0, 0, 255]), 5);
    assert_eq!(nearest_aci([0, 0, 0]), 7);
    assert_eq!(aci_rgb(10), [255, 0, 0]);
    assert_eq!(import::guess("0", aci_rgb(1)), Op::Cut);
    assert_eq!(import::guess("0", aci_rgb(5)), Op::Score);
    assert_eq!(import::guess("0", aci_rgb(7)), Op::Cut);
    assert_eq!(import::guess("0", aci_rgb(8)), Op::NoOutput);
    assert_eq!(import::guess("ENGRAVE", aci_rgb(1)), Op::Engrave);
    let mut d = Document::new(100.0, 100.0);
    d.add("a", Op::Cut, Shape::path(Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0)));
    d.add("b", Op::Score, Shape::path(Contour::line([0.0, 20.0], [10.0, 20.0])));
    let dx = read(&dxf(&d.s, false).unwrap().bytes, 100.0, 0.01).unwrap();
    let mut e = Document::new(100.0, 100.0);
    let skip = BTreeMap::from([("Score".to_string(), None), ("Cut".to_string(), Some(Op::Engrave))]);
    let ids = e.import_dxf(&dx, &skip, Some([50.0, 50.0])).unwrap();
    assert_eq!(ids.len(), 1);
    let (op, items) = on_layer(&e, "Cut 2");
    assert_eq!((op, items[0].metrics().bbox), (Op::Engrave, [50.0, 50.0, 10.0, 10.0]));
    let all = BTreeMap::from([("Score".to_string(), None), ("Cut".to_string(), None)]);
    assert!(e.import_dxf(&dx, &all, None).is_err());
}

/// LINE + ARC chain, a nested INSERT, inch units, an ELLIPSE, a SPLINE and TEXT, written by hand.
const FIXTURE: &str = include_str!("fixture.dxf");

#[test]
fn hand_written_fixture() {
    let dx = read(FIXTURE.as_bytes(), H, 0.01).unwrap();
    assert_eq!(dx.units, "inch");
    let names: Vec<_> = dx.groups.iter().map(|g| (g.name.as_str(), g.guess)).collect();
    assert_eq!(names, [("ELL", Some(Op::Cut)), ("Engrave text", Some(Op::Engrave)), ("MARK", Some(Op::Score)), ("PARTS", Some(Op::Cut)), ("SPL", None)]);
    let mut d = Document::new(400.0, H);
    let opts = BTreeMap::from([("SPL".to_string(), Some(Op::Cut))]);
    d.import_dxf(&dx, &opts, None).unwrap();
    let mm = |x: f64, y: f64| [x * 25.4, H - y * 25.4];

    // Two lines, an arc and a line meet end to end: one closed contour, 2 in square plus a half circle.
    let (_, parts) = on_layer(&d, "PARTS");
    assert_eq!(parts.len(), 1);
    let c = &parts[0].contours()[0];
    assert!(c.closed && c.vertices.len() == 4, "{c:?}");
    let in2 = 25.4 * 25.4;
    assert!(near(c.area().abs(), (4.0 + std::f64::consts::PI / 2.0) * in2, 1e-6), "{}", c.area());
    assert!(near(parts[0].metrics().bbox[2], 3.0 * 25.4, 1e-9));

    // INNER's line (0,0)-(1,0) at (1,0) in OUTER, OUTER at (5,0) turned 90° and scaled 2: (5,2)-(5,4).
    let (_, mark) = on_layer(&d, "MARK");
    let v = &mark[0].contours()[0].vertices;
    let ends = [[v[0].x, v[0].y], [v[1].x, v[1].y]];
    assert!(ends.iter().zip([mm(5.0, 2.0), mm(5.0, 4.0)]).all(|(a, b)| distance(*a, b) < 1e-9), "{ends:?}");

    // Ellipse at (10,5), axes 2 and 1 inch, within 0.01 mm.
    let (_, ell) = on_layer(&d, "ELL");
    let c = &ell[0].contours()[0];
    assert!(c.closed && c.vertices.iter().any(|v| v.bulge != 0.0));
    let err = deviation(c, |t| mm(10.0 + 2.0 * t.cos(), 5.0 + t.sin()), 0.0, std::f64::consts::TAU);
    assert!(err <= 0.01, "ellipse off by {err} mm");

    // Spline: one cubic Bézier span.
    let (_, spl) = on_layer(&d, "SPL");
    let p = [[0.0, 6.0], [1.0, 8.0], [3.0, 4.0], [4.0, 6.0]];
    let bez = |t: f64| {
        let u = 1.0 - t;
        let k = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        mm((0..4).map(|i| k[i] * p[i][0]).sum(), (0..4).map(|i| k[i] * p[i][1]).sum())
    };
    let err = deviation(&spl[0].contours()[0], bez, 0.0, 1.0);
    assert!(err <= 0.01, "spline off by {err} mm");

    // TEXT: a text item at its baseline, its cap height 0.25 in.
    let (op, text) = on_layer(&d, "Engrave text");
    assert_eq!(op, Op::Engrave);
    let Shape::Text { x, y, size, text, rotation, .. } = text[0] else { panic!("{:?}", text[0]) };
    assert_eq!(text, "Ø 10°");
    assert!(near(*x, 0.0, 1e-9) && near(*y, H - 8.0 * 25.4, 1e-9) && near(*size, 0.25 * 25.4 / 0.7, 1e-9) && near(*rotation, -30.0, 1e-9));
}

fn distance(a: Point, b: Point) -> f64 {
    crate::model::geom::distance(a, b)
}
