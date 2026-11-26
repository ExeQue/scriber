//! INFER and constraints on the selection, through the `Document` API.

use super::attach::{Link, axis_constraints, link_constraints, rect_constraints, touching};
use super::selection::selection_refs;
use super::{Kind, Ref};
use crate::model::geom::distance;
use crate::model::{Contour, Document, Op, Point, Shape, Transform};

fn add(d: &mut Document, c: Contour) -> u64 {
    d.checkpoint();
    d.add("", Op::Cut, Shape::path(c))
}

fn start(d: &Document, id: u64) -> Point {
    let v = d.item(id).unwrap().shape.contours()[0].vertices[0];
    [v.x, v.y]
}

#[test]
fn a_line_started_on_an_end_stays_attached() {
    let mut d = Document::new(200.0, 200.0);
    let a = add(&mut d, Contour::line([0.0, 0.0], [50.0, 0.0]));
    d.settle(true).unwrap();
    let b = add(&mut d, Contour::line([50.0, 0.0], [50.0, 40.0]));
    let link = Link { p: [50.0, 0.0], kind: Kind::Coincident, target: Ref::Point { item: a, contour: 0, vertex: 1 } };
    let found = link_constraints(&d.s, b, &[link]);
    assert_eq!(found, vec![(Kind::Coincident, vec![link.target, Ref::Point { item: b, contour: 0, vertex: 0 }])]);
    assert_eq!(d.infer_new(found), 1);
    d.transform(&[a], &Transform::translate(10.0, 5.0));
    d.settle(true).unwrap();
    assert!(distance(start(&d, b), [60.0, 5.0]) < 1e-6, "{:?}", start(&d, b));
}

#[test]
fn a_circle_on_an_arc_centre_is_concentric() {
    let mut d = Document::new(200.0, 200.0);
    let arc = add(&mut d, Contour::arc(50.0, 50.0, 20.0, 0.0, 90.0));
    let c = add(&mut d, Contour::circle(50.0, 50.0, 5.0));
    let link = Link { p: [50.0, 50.0], kind: Kind::Coincident, target: Ref::Centre { item: arc, contour: 0, segment: 0 } };
    let found = link_constraints(&d.s, c, &[link]);
    assert_eq!(found[0].0, Kind::Concentric);
    assert_eq!(d.infer_new(found), 1);
}

#[test]
fn conflicting_inferred_constraints_are_dropped() {
    let mut d = Document::new(200.0, 200.0);
    let a = add(&mut d, Contour::line([0.0, 0.0], [50.0, 0.0]));
    d.add_constraint(Kind::Fix, vec![Ref::Item { item: a }]).unwrap();
    d.settle(true).unwrap();
    let b = add(&mut d, Contour::line([0.0, 10.0], [50.0, 10.0]));
    // Joining a fixed line's end to the other line, which is also fixed horizontal at y = 10, cannot hold.
    d.add_constraint(Kind::Fix, vec![Ref::Item { item: b }]).unwrap();
    d.settle(true).unwrap();
    let found = vec![(Kind::Coincident, vec![Ref::Point { item: a, contour: 0, vertex: 0 }, Ref::Point { item: b, contour: 0, vertex: 0 }])];
    assert_eq!(d.infer_new(found), 0);
    assert_eq!(d.s.constraints.len(), 2, "only the fixes are left");
    assert!(d.item(b).is_some(), "the drawing stays");
}

#[test]
fn rectangles_stay_level_and_agents_get_joined_by_coordinates() {
    let mut d = Document::new(200.0, 200.0);
    let r = add(&mut d, Contour::rect(0.0, 0.0, 40.0, 20.0, 0.0));
    let kinds: Vec<Kind> = axis_constraints(&d.s, r).into_iter().map(|c| c.0).collect();
    assert_eq!(kinds, [Kind::Horizontal, Kind::Vertical, Kind::Horizontal, Kind::Vertical]);
    let kinds: Vec<Kind> = rect_constraints(&d.s, r).into_iter().map(|c| c.0).collect();
    assert_eq!(kinds, [Kind::Perpendicular; 3], "upright too");
    let t = crate::model::Transform { rotate: 30.0, pivot: Some([0.0, 0.0]), ..Default::default() };
    let turned = add(&mut d, Contour::rect(0.0, 0.0, 40.0, 20.0, 0.0).transformed(&t, [0.0, 0.0]));
    let kinds: Vec<Kind> = rect_constraints(&d.s, turned).into_iter().map(|c| c.0).collect();
    assert_eq!(kinds, [Kind::Perpendicular; 3]);
    assert_eq!(d.infer_new(rect_constraints(&d.s, turned)), 3, "they hold together");
    let l = add(&mut d, Contour::line([40.0, 20.0], [80.0, 60.0]));
    let links = touching(&d.s, l);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, Ref::Point { item: r, contour: 0, vertex: 2 });
}

#[test]
fn selection_picks_the_closest_ends_to_join() {
    let mut d = Document::new(200.0, 200.0);
    let a = add(&mut d, Contour::line([0.0, 0.0], [50.0, 0.0]));
    let b = add(&mut d, Contour::line([51.0, 1.0], [90.0, 30.0]));
    let sets = selection_refs(&d.s, Kind::Coincident, &[a, b]).unwrap();
    assert_eq!(sets, vec![vec![Ref::Point { item: a, contour: 0, vertex: 1 }, Ref::Point { item: b, contour: 0, vertex: 0 }]]);
    let c = add(&mut d, Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0));
    assert!(selection_refs(&d.s, Kind::Parallel, &[a, c]).is_err(), "a rectangle is not one line");
    assert_eq!(selection_refs(&d.s, Kind::Horizontal, &[a, b]).unwrap().len(), 2);
}

fn circle(d: &Document, id: u64) -> (Point, f64) {
    d.item(id).unwrap().shape.contours()[0].as_circle().unwrap()
}

fn seg(item: u64) -> Ref {
    Ref::Segment { item, contour: 0, segment: 0 }
}

#[test]
fn a_circle_drawn_against_another_stays_touching() {
    let mut d = Document::new(200.0, 200.0);
    let a = add(&mut d, Contour::circle(50.0, 50.0, 20.0));
    // Centre (80, 50), radius point (70, 50) on the first circle (its quadrant).
    let b = add(&mut d, Contour::circle(80.0, 50.0, 10.0));
    let found = link_constraints(&d.s, b, &[Link { p: [70.0, 50.0], kind: Kind::Coincident, target: seg(a) }]);
    assert_eq!(found, vec![(Kind::Tangent, vec![seg(a), seg(b)])]);
    assert_eq!(d.infer_new(found), 1);
    d.transform(&[a], &Transform { dx: 5.0, scale_x: 1.2, scale_y: 1.2, ..Transform::default() });
    d.settle(true).unwrap();
    let ((c1, r1), (c2, r2)) = (circle(&d, a), circle(&d, b));
    assert!((r1 - 24.0).abs() < 1e-6, "{r1}");
    assert!((distance(c1, c2) - (r1 + r2)).abs() < 1e-6, "{c1:?} {r1} {c2:?} {r2}");
}

#[test]
fn a_circle_inside_another_touches_it_from_inside() {
    let mut d = Document::new(200.0, 200.0);
    let a = add(&mut d, Contour::circle(50.0, 50.0, 20.0));
    let b = add(&mut d, Contour::circle(60.0, 50.0, 10.0));
    let found = link_constraints(&d.s, b, &[Link { p: [70.0, 50.0], kind: Kind::Coincident, target: seg(a) }]);
    assert_eq!(d.infer_new(found), 1);
    d.transform(&[a], &Transform::translate(0.0, 4.0));
    d.settle(true).unwrap();
    let ((c1, r1), (c2, r2)) = (circle(&d, a), circle(&d, b));
    assert!((distance(c1, c2) - (r1 - r2)).abs() < 1e-6, "{c1:?} {r1} {c2:?} {r2}");
}

#[test]
fn a_circle_centred_on_another_keeps_its_centre_on_it() {
    let mut d = Document::new(200.0, 200.0);
    let a = add(&mut d, Contour::circle(50.0, 50.0, 20.0));
    let b = add(&mut d, Contour::circle(70.0, 50.0, 5.0));
    let found = link_constraints(&d.s, b, &[Link { p: [70.0, 50.0], kind: Kind::Coincident, target: seg(a) }]);
    assert_eq!(found, vec![(Kind::Coincident, vec![seg(a), Ref::Centre { item: b, contour: 0, segment: 0 }])]);
}

#[test]
fn a_circle_touching_a_line_stays_tangent() {
    let mut d = Document::new(200.0, 200.0);
    let line = add(&mut d, Contour::line([0.0, 0.0], [100.0, 0.0]));
    let c = add(&mut d, Contour::circle(50.0, 10.0, 10.0));
    let found = link_constraints(&d.s, c, &[Link { p: [50.0, 0.0], kind: Kind::Perpendicular, target: seg(line) }]);
    assert_eq!(found, vec![(Kind::Tangent, vec![seg(line), seg(c)])]);
    assert_eq!(d.infer_new(found), 1);
    d.transform(&[line], &Transform::translate(0.0, 6.0));
    d.settle(true).unwrap();
    let (centre, r) = circle(&d, c);
    assert!(((centre[1] - 6.0).abs() - r).abs() < 1e-6, "{centre:?} {r}");
}
