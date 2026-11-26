//! The solver on real cases, through the `Document` API the GUI and MCP use.

use super::{Kind, Ref};
use crate::model::geom::distance;
use crate::model::{Contour, Document, Op, Point, Shape, Transform};

fn seg(item: u64, segment: usize) -> Ref {
    Ref::Segment { item, contour: 0, segment }
}

fn pt(item: u64, vertex: usize) -> Ref {
    Ref::Point { item, contour: 0, vertex }
}

fn add(d: &mut Document, c: Contour) -> u64 {
    d.add("", Op::Cut, Shape::path(c))
}

fn vertices(d: &Document, id: u64) -> Vec<Point> {
    d.item(id).unwrap().shape.contours()[0].vertices.iter().map(|v| [v.x, v.y]).collect()
}

fn close(a: Point, b: Point) -> bool {
    distance(a, b) < 1e-6
}

fn move_vertex(d: &mut Document, id: u64, k: usize, to: Point) {
    let Shape::Path { contours } = &mut d.item_mut(id).unwrap().shape else { panic!() };
    (contours[0].vertices[k].x, contours[0].vertices[k].y) = (to[0], to[1]);
    d.touch();
}

fn is_rectangle(p: &[Point]) -> bool {
    (0..4).all(|k| {
        let (a, b, c) = (p[k], p[(k + 1) % 4], p[(k + 2) % 4]);
        ((b[0] - a[0]) * (c[0] - b[0]) + (b[1] - a[1]) * (c[1] - b[1])).abs() < 1e-6
    })
}

#[test]
fn rectangle_stays_a_rectangle_when_a_corner_is_dragged() {
    let mut d = Document::new(200.0, 200.0);
    let r = add(&mut d, Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0));
    d.add_constraint(Kind::Perpendicular, vec![seg(r, 0), seg(r, 1)]).unwrap();
    d.add_constraint(Kind::Parallel, vec![seg(r, 0), seg(r, 2)]).unwrap();
    d.add_constraint(Kind::Parallel, vec![seg(r, 1), seg(r, 3)]).unwrap();
    d.settle(true).unwrap();
    move_vertex(&mut d, r, 2, [70.0, 45.0]);
    d.settle(true).unwrap();
    let p = vertices(&d, r);
    assert!(is_rectangle(&p), "{p:?}");
    assert!(close(p[2], [70.0, 45.0]), "the dragged corner stays under the pointer: {p:?}");

    // Level and plumb sides: the opposite corner does not move at all.
    let b = add(&mut d, Contour::rect(100.0, 100.0, 40.0, 20.0, 0.0));
    for (k, kind) in [(0, Kind::Horizontal), (1, Kind::Vertical), (2, Kind::Horizontal), (3, Kind::Vertical)] {
        d.add_constraint(kind, vec![seg(b, k)]).unwrap();
    }
    d.settle(true).unwrap();
    move_vertex(&mut d, b, 2, [150.0, 130.0]);
    d.settle(true).unwrap();
    assert_eq!(vertices(&d, b), vec![[100.0, 100.0], [150.0, 100.0], [150.0, 130.0], [100.0, 130.0]]);
}

#[test]
fn tangent_concentric_and_equal() {
    let mut d = Document::new(200.0, 200.0);
    let line = add(&mut d, Contour::line([0.0, 0.0], [100.0, 0.0]));
    let c = add(&mut d, Contour::circle(50.0, 20.0, 10.0));
    d.add_constraint(Kind::Tangent, vec![seg(line, 0), seg(c, 0)]).unwrap();
    d.settle(true).unwrap();
    let circle = d.item(c).unwrap().shape.contours()[0].as_circle().unwrap();
    assert!((circle.0[1].abs() - circle.1).abs() < 1e-6, "the circle touches the line: {circle:?}");
    assert_eq!(vertices(&d, line), vec![[0.0, 0.0], [100.0, 0.0]], "the first object stays");

    let a = add(&mut d, Contour::circle(150.0, 150.0, 5.0));
    let b = add(&mut d, Contour::circle(140.0, 160.0, 12.0));
    d.add_constraint(Kind::Concentric, vec![seg(a, 0), seg(b, 0)]).unwrap();
    d.add_constraint(Kind::Equal, vec![seg(a, 0), seg(b, 0)]).unwrap();
    d.settle(true).unwrap();
    let (ca, cb) = (d.item(a).unwrap().shape.contours()[0].as_circle().unwrap(), d.item(b).unwrap().shape.contours()[0].as_circle().unwrap());
    assert!(close(ca.0, [150.0, 150.0]) && close(cb.0, [150.0, 150.0]), "{ca:?} {cb:?}");
    assert!((ca.1 - 5.0).abs() < 1e-6 && (cb.1 - 5.0).abs() < 1e-6);

    // An arc keeps its bulge meaning: its ends stay on its circle when the radius is driven.
    let arc = add(&mut d, Contour::arc(50.0, 150.0, 20.0, 0.0, 90.0));
    d.add_dimension(Kind::DimRadial, vec![seg(arc, 0)], None, Some("30"), 0.0).unwrap();
    d.settle(true).unwrap();
    let v = &d.item(arc).unwrap().shape.contours()[0].vertices;
    let (centre, r, _, sweep) = crate::model::geom::arc_params([v[0].x, v[0].y], [v[1].x, v[1].y], v[0].bulge);
    assert!((r - 30.0).abs() < 1e-6 && sweep > 0.0, "{centre:?} {r}");
}

#[test]
fn dimension_changes_propagate_through_expressions() {
    let mut d = Document::new(200.0, 200.0);
    let r = add(&mut d, Contour::rect(0.0, 0.0, 40.0, 10.0, 0.0));
    for (k, kind) in [(0, Kind::Horizontal), (1, Kind::Vertical), (2, Kind::Horizontal), (3, Kind::Vertical)] {
        d.add_constraint(kind, vec![seg(r, k)]).unwrap();
    }
    let d1 = d.add_dimension(Kind::DimHorizontal, vec![pt(r, 0), pt(r, 1)], None, None, -5.0).unwrap();
    assert_eq!(d.constraint(d1).unwrap().name, "d1");
    assert_eq!(d.constraint(d1).unwrap().expr, "40");
    d.add_dimension(Kind::DimVertical, vec![seg(r, 1)], None, Some("d1 / 2"), 5.0).unwrap();
    d.settle(true).unwrap();
    assert_eq!(d.item(r).unwrap().shape.metrics().bbox, [0.0, 0.0, 40.0, 20.0]);
    d.set_parameter("d1", "60").unwrap();
    d.settle(true).unwrap();
    let b = d.item(r).unwrap().shape.metrics().bbox;
    assert!(b.iter().zip([0.0, 0.0, 60.0, 30.0]).all(|(v, e)| (v - e).abs() < 1e-6), "{b:?}");
    d.rename_parameter("d1", "width").unwrap();
    assert_eq!(d.parameter("d2").unwrap().expr, "width / 2");
    assert!(d.set_parameter("width", "d2").unwrap_err().contains("itself"));
    assert!(d.list_constraints().iter().all(|c| c.satisfied));
}

#[test]
fn conflicts_are_refused_and_named() {
    let mut d = Document::new(100.0, 100.0);
    let l = add(&mut d, Contour::line([0.0, 0.0], [10.0, 10.0]));
    d.add_constraint(Kind::Horizontal, vec![seg(l, 0)]).unwrap();
    d.settle(true).unwrap();
    d.checkpoint();
    d.add_constraint(Kind::Vertical, vec![seg(l, 0)]).unwrap();
    d.settle(true).unwrap();
    // Level and plumb leave a line of length 0; asking for 10 cannot work.
    d.checkpoint();
    d.add_dimension(Kind::DimAligned, vec![seg(l, 0)], None, Some("10"), 0.0).unwrap();
    let err = d.settle(true).unwrap_err();
    assert!(err.contains("horizontal #") && err.contains("vertical #") && err.contains("d1 = 10"), "{err}");
    assert_eq!(d.s.constraints.len(), 2, "the refused constraint is gone");
    assert!(d.undo(), "the refused step left no undo entry; this undoes vertical");
    assert_eq!(d.s.constraints.len(), 1);
    assert!(d.add_constraint(Kind::Parallel, vec![seg(l, 0), pt(l, 0)]).is_err(), "parallel needs two lines");
}

#[test]
fn fix_holds_and_deleting_geometry_deletes_its_constraints() {
    let mut d = Document::new(100.0, 100.0);
    let a = add(&mut d, Contour::line([0.0, 0.0], [10.0, 0.0]));
    let b = add(&mut d, Contour::line([12.0, 1.0], [20.0, 5.0]));
    d.add_constraint(Kind::Fix, vec![pt(a, 0)]).unwrap();
    d.add_constraint(Kind::Coincident, vec![pt(a, 1), pt(b, 0)]).unwrap();
    d.settle(true).unwrap();
    assert!(close(vertices(&d, b)[0], [10.0, 0.0]), "the second object moves onto the first");
    d.transform(&[a], &Transform::translate(5.0, 5.0));
    d.settle(true).unwrap();
    assert!(close(vertices(&d, a)[0], [0.0, 0.0]), "a fixed point stays put");
    assert!(close(vertices(&d, a)[1], vertices(&d, b)[0]));
    d.delete(&[b]);
    assert_eq!(d.s.constraints.len(), 1);
}

#[test]
fn constraints_survive_save_open_undo_and_are_dropped_when_the_shape_changes() {
    let mut d = Document::new(100.0, 100.0);
    let r = add(&mut d, Contour::rect(10.0, 10.0, 30.0, 20.0, 0.0));
    d.checkpoint();
    let ids = d.auto_constrain(&[r], 0.01, 0.5);
    let kinds: Vec<Kind> = ids.iter().map(|id| d.constraint(*id).unwrap().kind).collect();
    assert_eq!(kinds, vec![Kind::Horizontal, Kind::Vertical, Kind::Horizontal, Kind::Vertical]);
    d.add_dimension(Kind::DimAligned, vec![seg(r, 0)], Some("width"), None, 5.0).unwrap();
    d.settle(true).unwrap();

    let dir = std::env::temp_dir().join(format!("scriber-constraints-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("c.scrbr");
    d.save(&path).unwrap();
    let e = Document::open(&path).unwrap();
    assert_eq!(e.s.constraints, d.s.constraints);
    std::fs::remove_dir_all(dir).ok();

    assert!(d.undo());
    assert!(d.s.constraints.is_empty());
    assert!(d.redo());
    assert_eq!(d.s.constraints.len(), 5);

    // A fillet adds vertices: the numbers no longer mean the same corners, so the constraints go.
    d.fillet(&[r], 2.0, Some([10.0, 10.0])).unwrap();
    d.settle(true).unwrap();
    assert!(d.s.constraints.is_empty());
}

#[test]
fn autoconstrain_finds_relations_between_items() {
    let mut d = Document::new(200.0, 200.0);
    let a = add(&mut d, Contour::polyline(&[[0.0, 0.0], [30.0, 10.0]], false));
    let b = add(&mut d, Contour::polyline(&[[30.0, 10.0], [20.0, 40.0]], false));
    let c1 = add(&mut d, Contour::circle(100.0, 100.0, 10.0));
    let c2 = add(&mut d, Contour::circle(100.0, 100.0, 10.0005));
    let ids = d.auto_constrain(&[a, b, c1, c2], 0.01, 1.0);
    let kinds: Vec<Kind> = ids.iter().map(|id| d.constraint(*id).unwrap().kind).collect();
    // The two lines happen to be the same length too.
    assert_eq!(kinds, vec![Kind::Coincident, Kind::Perpendicular, Kind::Concentric, Kind::Equal, Kind::Equal]);
    d.settle(true).unwrap();
    assert!(d.list_constraints().iter().all(|c| c.satisfied));
}

fn is_regular(p: &[Point]) -> bool {
    let n = p.len();
    let side = distance(p[0], p[1]);
    let diag = distance(p[0], p[2]);
    (0..n).all(|k| (distance(p[k], p[(k + 1) % n]) - side).abs() < 1e-6 && (distance(p[k], p[(k + 2) % n]) - diag).abs() < 1e-6)
}

#[test]
fn a_regular_polygon_stays_regular_when_a_corner_is_dragged() {
    for sides in [3, 5, 6] {
        let mut d = Document::new(200.0, 200.0);
        let p = add(&mut d, Contour::polygon(50.0, 50.0, 20.0, sides, 10.0));
        let before = vertices(&d, p);
        assert_eq!(d.constrain_new(vec![(Kind::Regular, vec![Ref::Item { item: p }])], Vec::new()), 1);
        assert_eq!(vertices(&d, p), before, "settling right after creation moves nothing");
        move_vertex(&mut d, p, 1, [before[1][0] + 7.0, before[1][1] + 3.0]);
        d.settle(true).unwrap();
        let after = vertices(&d, p);
        assert!(close(after[1], [before[1][0] + 7.0, before[1][1] + 3.0]), "{sides}: {after:?}");
        assert!(is_regular(&after), "{sides}: {after:?}");
    }
}

#[test]
fn regular_only_fits_one_closed_straight_contour() {
    let mut d = Document::new(200.0, 200.0);
    let c = add(&mut d, Contour::circle(50.0, 50.0, 10.0));
    let l = add(&mut d, Contour::polyline(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false));
    for item in [c, l] {
        assert!(d.add_constraint(Kind::Regular, vec![Ref::Item { item }]).is_err());
    }
}

#[test]
fn own_constraints_survive_when_inferred_ones_conflict() {
    // The line was drawn 30 long and typed so; an inferred vertical on it cannot hold with a level end.
    let mut d = Document::new(200.0, 200.0);
    let l = add(&mut d, Contour::line([0.0, 0.0], [30.0, 0.0]));
    d.add_constraint(Kind::Fix, vec![Ref::Item { item: l }]).unwrap();
    d.settle(true).unwrap();
    let n = d.constrain_new(vec![(Kind::DimAligned, vec![seg(l, 0)])], vec![(Kind::Vertical, vec![seg(l, 0)])]);
    assert_eq!(n, 1);
    let kinds: Vec<Kind> = d.s.constraints.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, vec![Kind::Fix, Kind::DimAligned]);
    assert_eq!(d.parameter("d1").unwrap().expr, "30");
}
