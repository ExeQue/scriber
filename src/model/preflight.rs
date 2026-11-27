//! The pre-flight check: what would go wrong on the laser, in plain words, with the items involved.
//! Pure: it reads the document and the material library and changes nothing.

use super::geom::{BBox, Point, distance};
use super::{Item, Op, Shape, State};
use crate::library::{Library, Operation};
use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

/// The smallest detail the beam can make, in mm (a typical diode or CO2 spot is 0.08–0.1 mm).
pub const BEAM: f64 = 0.1;
/// Text below this font size (mm) is hard to read when engraved and falls apart when cut.
pub const MIN_TEXT_SIZE: f64 = 2.0;
/// Lines closer than this (mm) count as lying on top of each other.
pub const DOUBLE_LINE_TOL: f64 = 0.05;
/// How much line (mm) must lie on top of another before it is reported.
pub const MIN_DOUBLE_LENGTH: f64 = 0.5;
/// Ends of open cut lines closer than this (mm) are joined.
pub const JOIN_TOL: f64 = 0.01;
/// Slack when comparing a shape's box with the material or work area, in mm.
const EDGE_TOL: f64 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// The job will go wrong or cannot run as drawn.
    Error,
    /// Probably a mistake, but it can be on purpose.
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    OpenCut,
    DoubleLine,
    OutsideMaterial,
    OutsideWorkArea,
    TooSmall,
    SmallText,
    NoSetting,
}

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Warning {
    pub severity: Severity,
    pub kind: Kind,
    pub text: String,
    /// The items involved, to select and zoom to.
    pub items: Vec<u64>,
}

fn warn(severity: Severity, kind: Kind, text: String, items: Vec<u64>) -> Option<Warning> {
    (!items.is_empty()).then_some(Warning { severity, kind, text, items })
}

fn shapes(n: usize) -> String {
    if n == 1 { "1 shape".into() } else { format!("{n} shapes") }
}

/// Every warning for the document, errors first.
pub fn check(s: &State, lib: &Library) -> Vec<Warning> {
    // Only what reaches the laser; guides, no-output items and hidden layers are never sent.
    let lasered: Vec<&Item> = s.shown().filter(|i| s.op(i) != Op::NoOutput && !matches!(i.shape, Shape::Guide(_) | Shape::Dimension { .. })).collect();
    let mut out = no_settings(s, lib, &lasered);
    out.extend(outside_work_area(s, lib, &lasered));
    out.extend(outside_material(s, &lasered));
    out.extend(double_lines(s, &lasered));
    out.extend(open_cuts(s, &lasered));
    out.extend(too_small(&lasered));
    out.extend(small_text(&lasered));
    out.sort_by_key(|w| w.severity != Severity::Error);
    out
}

fn open_cuts(s: &State, items: &[&Item]) -> Option<Warning> {
    let cuts: Vec<&Item> = items.iter().copied().filter(|i| s.effective_op(i) == Op::Cut).collect();
    // Ends of every open cut line; a chain of lines that closes (four lines as a rectangle) has none left over.
    let ends: Vec<(u64, Point)> = cuts
        .iter()
        .flat_map(|i| i.shape.contours().iter().filter(|c| !c.closed).map(move |c| (i.id, c)))
        .flat_map(|(id, c)| {
            let p = c.flatten(1.0);
            [(id, p[0]), (id, *p.last().unwrap())]
        })
        .collect();
    let loose = |k: usize| !ends.iter().enumerate().any(|(j, e)| j != k && distance(e.1, ends[k].1) <= JOIN_TOL);
    let mut ids: Vec<u64> = (0..ends.len()).filter(|k| loose(*k)).map(|k| ends[k].0).collect();
    ids.dedup();
    let n = ids.len();
    warn(Severity::Warning, Kind::OpenCut, format!("{} set to Cut {} not closed, so nothing is cut free.", shapes(n), if n == 1 { "is" } else { "are" }), ids)
}

/// One straight piece of a laser line.
struct Seg {
    item: u64,
    a: Point,
    b: Point,
}

/// Lines (cut or scored) on top of each other: the laser goes over them twice and burns the edge.
fn double_lines(s: &State, items: &[&Item]) -> Vec<Warning> {
    let mut segs: Vec<Seg> = items
        .iter()
        .filter(|i| matches!(s.effective_op(i), Op::Cut | Op::Score))
        .flat_map(|i| i.shape.laser_contours().iter().map(|c| c.flatten(DOUBLE_LINE_TOL / 4.0)).collect::<Vec<_>>().into_iter().map(move |p| (i.id, p)))
        .flat_map(|(item, p)| p.windows(2).map(|w| Seg { item, a: w[0], b: w[1] }).collect::<Vec<_>>())
        .filter(|s| distance(s.a, s.b) > 0.0)
        .collect();
    segs.sort_by(|s, t| s.a[0].min(s.b[0]).total_cmp(&t.a[0].min(t.b[0])));
    // Overlapping length per pair of items (the same item twice when a shape runs over itself).
    let mut pairs: Vec<((u64, u64), f64)> = Vec::new();
    // Sweep on x only: O(n²) when many lines share one column.
    for (k, s) in segs.iter().enumerate() {
        let right = s.a[0].max(s.b[0]) + DOUBLE_LINE_TOL;
        for t in segs[k + 1..].iter().take_while(|t| t.a[0].min(t.b[0]) <= right) {
            let len = overlap(s, t);
            if len > 0.0 {
                let key = (s.item.min(t.item), s.item.max(t.item));
                match pairs.iter_mut().find(|p| p.0 == key) {
                    Some(p) => p.1 += len,
                    None => pairs.push((key, len)),
                }
            }
        }
    }
    pairs
        .into_iter()
        .filter(|p| p.1 >= MIN_DOUBLE_LENGTH)
        .filter_map(|((a, b), len)| {
            let ids = if a == b { vec![a] } else { vec![a, b] };
            let text = format!("Lines lie on top of each other ({len:.1} mm): the laser goes over them twice and burns the edge.");
            warn(Severity::Warning, Kind::DoubleLine, text, ids)
        })
        .collect()
}

/// How long `t` runs along `s` within the tolerance (0 when they only touch or cross).
fn overlap(s: &Seg, t: &Seg) -> f64 {
    let len = distance(s.a, s.b);
    let dir = [(s.b[0] - s.a[0]) / len, (s.b[1] - s.a[1]) / len];
    let off_line = |p: Point| ((p[0] - s.a[0]) * dir[1] - (p[1] - s.a[1]) * dir[0]).abs() > DOUBLE_LINE_TOL;
    if off_line(t.a) || off_line(t.b) {
        return 0.0;
    }
    let along = |p: Point| ((p[0] - s.a[0]) * dir[0] + (p[1] - s.a[1]) * dir[1]).clamp(0.0, len);
    (along(t.a) - along(t.b)).abs()
}

fn inside(b: BBox, w: f64, h: f64) -> bool {
    b[0] >= -EDGE_TOL && b[1] >= -EDGE_TOL && b[0] + b[2] <= w + EDGE_TOL && b[1] + b[3] <= h + EDGE_TOL
}

fn outside(items: &[&Item], w: f64, h: f64) -> Vec<u64> {
    items.iter().filter(|i| !inside(i.shape.metrics().bbox, w, h)).map(|i| i.id).collect()
}

fn outside_material(s: &State, items: &[&Item]) -> Option<Warning> {
    let ids = outside(items, s.width, s.height);
    warn(Severity::Warning, Kind::OutsideMaterial, format!("Outside the material: {}.", shapes(ids.len())), ids)
}

fn outside_work_area(s: &State, lib: &Library, items: &[&Item]) -> Option<Warning> {
    let m = lib.machine(s.machine.as_deref()?)?;
    let [w, h] = m.area(s.laser.as_deref());
    let ids = outside(items, w, h);
    warn(Severity::Error, Kind::OutsideWorkArea, format!("Outside the work area of the {} ({w} × {h} mm): {}. The laser cannot reach there.", m.name, shapes(ids.len())), ids)
}

/// Lines and holes smaller than the beam: the laser cannot make them.
fn too_small(items: &[&Item]) -> Option<Warning> {
    let tiny = |i: &&&Item| {
        i.shape.laser_contours().iter().any(|c| {
            let b = c.bbox();
            if c.closed { b[2].max(b[3]) < BEAM } else { c.length() < BEAM }
        })
    };
    let ids: Vec<u64> = items.iter().filter(tiny).map(|i| i.id).collect();
    let text = format!("{} {} lines or holes smaller than the laser beam ({BEAM} mm).", shapes(ids.len()), if ids.len() == 1 { "has" } else { "have" });
    warn(Severity::Warning, Kind::TooSmall, text, ids)
}

fn small_text(items: &[&Item]) -> Option<Warning> {
    let ids: Vec<u64> = items.iter().filter(|i| matches!(i.shape, Shape::Text { size, .. } if size < MIN_TEXT_SIZE)).map(|i| i.id).collect();
    let text = format!("Text smaller than {MIN_TEXT_SIZE} mm is hard to read once lasered ({}).", shapes(ids.len()));
    warn(Severity::Warning, Kind::SmallText, text, ids)
}

/// Operations in use without power and speed for this material on this machine.
fn no_settings(s: &State, lib: &Library, items: &[&Item]) -> Vec<Warning> {
    let (Some(machine), Some(material)) = (s.machine.as_deref(), s.material.as_deref()) else { return Vec::new() };
    let names = |id: &str, found: Option<&String>| found.cloned().unwrap_or_else(|| id.to_string());
    let machine_name = names(machine, lib.machine(machine).map(|m| &m.name));
    let material_name = names(material, lib.material(material).map(|m| &m.name));
    s.layers
        .iter()
        .flat_map(|layer| Operation::ALL.into_iter().map(move |op| (layer, op)))
        .filter(|(layer, op)| layer.setting(s, lib, *op).is_none())
        .filter_map(|(layer, op)| {
            let ids: Vec<u64> = items.iter().filter(|i| i.layer == layer.id && operation(s, i) == op).map(|i| i.id).collect();
            let op = op.label().to_lowercase();
            let text = format!("Layer \u{201c}{}\u{201d}: no {op} setting for {material_name} on the {machine_name}, in the material library or on the layer.", layer.name);
            warn(Severity::Error, Kind::NoSetting, text, ids)
        })
        .collect()
}

/// The library operation an item needs; images are engraved.
pub fn operation(s: &State, i: &Item) -> Operation {
    match (s.effective_op(i), &i.shape) {
        (_, Shape::Image { .. }) | (Op::Engrave, _) => Operation::Engrave,
        (Op::Score, _) => Operation::Score,
        _ => Operation::Cut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::store;
    use crate::model::{Contour, Document};

    fn kinds(d: &Document, lib: &Library) -> Vec<(Kind, Vec<u64>)> {
        check(&d.s, lib).into_iter().map(|w| (w.kind, w.items)).collect()
    }

    fn lib() -> Library {
        store::merged(store::LibraryFile::default())
    }

    #[test]
    fn clean_document_has_no_warnings() {
        let mut d = Document::new(100.0, 100.0);
        d.add("box", Op::Cut, Shape::Path { contours: vec![Contour::rect(10.0, 10.0, 40.0, 40.0, 2.0), Contour::circle(30.0, 30.0, 5.0)] });
        // Four lines that close a square cut a piece free.
        for (a, b) in [([60.0, 10.0], [90.0, 10.0]), ([90.0, 10.0], [90.0, 40.0]), ([90.0, 40.0], [60.0, 40.0]), ([60.0, 40.0], [60.0, 10.0])] {
            d.add("side", Op::Cut, Shape::path(Contour::line(a, b)));
        }
        d.add("face", Op::Engrave, Shape::path(Contour::rect(15.0, 15.0, 10.0, 10.0, 0.0)));
        d.add("note", Op::NoOutput, Shape::path(Contour::line([-10.0, 0.0], [0.05, 0.0])));
        d.add("xl", Op::NoOutput, Shape::Guide(crate::model::Guide::Xline { x: 1.0, y: 1.0, angle: 0.0 }));
        d.s.machine = Some("xtool-m2".into());
        d.s.material = Some("basswood-plywood-3mm".into());
        let mut lib = lib();
        // The built-in library only has cut settings; add an engrave one.
        let mut engrave = lib.setting("xtool-m2", None, "basswood-plywood-3mm", Operation::Cut).unwrap().clone();
        engrave.operation = Operation::Engrave;
        lib.material_mut("basswood-plywood-3mm").unwrap().put(engrave);
        assert_eq!(kinds(&d, &lib), vec![]);
    }

    #[test]
    fn each_problem_is_found() {
        let mut d = Document::new(100.0, 100.0);
        let open = d.add("open", Op::Cut, Shape::path(Contour::polyline(&[[10.0, 10.0], [20.0, 10.0], [20.0, 20.0]], false)));
        let a = d.add("a", Op::Cut, Shape::path(Contour::rect(30.0, 30.0, 10.0, 10.0, 0.0)));
        let b = d.add("b", Op::Score, Shape::path(Contour::line([30.0, 30.0], [40.0, 30.0])));
        let out = d.add("out", Op::Score, Shape::path(Contour::rect(90.0, 90.0, 20.0, 5.0, 0.0)));
        let dot = d.add("dot", Op::Cut, Shape::path(Contour::circle(60.0, 60.0, 0.02)));
        let text = Shape::Text { x: 5.0, y: 80.0, size: 1.0, text: "tiny".into(), rotation: 0.0, font: "sans-serif".into(), bold: false, italic: false };
        let small = d.add("t", Op::Engrave, text);
        let lib = lib();
        let found = kinds(&d, &lib);
        assert!(found.contains(&(Kind::OpenCut, vec![open])), "{found:?}");
        assert!(found.contains(&(Kind::DoubleLine, vec![a, b])), "{found:?}");
        assert!(found.contains(&(Kind::OutsideMaterial, vec![out])), "{found:?}");
        // The 1 mm text may be too small as well (its i-dots, in some fonts).
        assert!(found.iter().any(|(k, ids)| *k == Kind::TooSmall && ids.contains(&dot)), "{found:?}");
        assert!(found.contains(&(Kind::SmallText, vec![small])), "{found:?}");
        assert!(!found.iter().any(|w| matches!(w.0, Kind::OutsideWorkArea | Kind::NoSetting)), "no machine chosen: {found:?}");

        // A machine smaller than the item, and a material without score or engrave settings.
        d.s.width = 1000.0;
        let mut lib = lib;
        let mut m = lib.machine("xtool-m2").unwrap().clone();
        m.lasers.iter_mut().for_each(|l| l.area = None);
        (m.width, m.height) = (95.0, 95.0);
        lib.put_machine(m);
        d.s.machine = Some("xtool-m2".into());
        d.s.material = Some("black-acrylic-3mm".into());
        let w = check(&d.s, &lib);
        assert_eq!(w[0].severity, Severity::Error, "errors come first");
        let found: Vec<(Kind, Vec<u64>)> = w.into_iter().map(|w| (w.kind, w.items)).collect();
        assert!(found.contains(&(Kind::OutsideWorkArea, vec![out])), "{found:?}");
        assert!(found.contains(&(Kind::NoSetting, vec![b, out])), "score has no setting: {found:?}");
        assert!(found.contains(&(Kind::NoSetting, vec![small])), "engrave has no setting: {found:?}");
        assert!(!found.iter().any(|w| w.0 == Kind::OutsideMaterial), "{found:?}");
    }

    #[test]
    fn a_circle_drawn_twice_is_a_double_line() {
        let mut d = Document::new(100.0, 100.0);
        let a = d.add("a", Op::Cut, Shape::path(Contour::circle(50.0, 50.0, 10.0)));
        let b = d.add("b", Op::Cut, Shape::path(Contour::circle(50.0, 50.0, 10.0)));
        d.add("c", Op::Cut, Shape::path(Contour::circle(50.0, 50.0, 12.0)));
        assert_eq!(kinds(&d, &lib()), vec![(Kind::DoubleLine, vec![a, b])]);
    }
}
