//! Constraints through TRIM: what was on a circle stays on the arc that is left, pieces of one
//! circle stay on one circle, and the new ends stay on their cutting edges.

use super::{Kind, Ref};
use crate::model::geom::{arc_params, distance};
use crate::model::{Contour, Document, Op, Point, Shape, Transform};

fn add(d: &mut Document, c: Contour) -> u64 {
    d.add("", Op::Cut, Shape::path(c))
}

fn seg(item: u64, contour: usize, segment: usize) -> Ref {
    Ref::Segment { item, contour, segment }
}

/// Centre and radius of every arc of `id` (a circle counts once).
fn arcs(d: &Document, id: u64) -> Vec<(Point, f64)> {
    let mut out = Vec::new();
    for c in d.item(id).unwrap().shape.contours() {
        if let Some(circle) = c.as_circle() {
            out.push(circle);
            continue;
        }
        for k in 0..c.vertices.len() - usize::from(!c.closed) {
            let (a, b) = (c.vertices[k], c.vertices[(k + 1) % c.vertices.len()]);
            if a.bulge != 0.0 {
                let (centre, r, ..) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
                out.push((centre, r));
            }
        }
    }
    out
}

fn touching(a: (Point, f64), b: (Point, f64)) -> bool {
    (distance(a.0, b.0) - (a.1 + b.1)).abs() < 1e-6
}

/// Circle `a` between circles `b` and `c`, tangent to both, and a vertical line through `a`.
fn three_circles(d: &mut Document) -> (u64, u64, u64, u64) {
    let a = add(d, Contour::circle(50.0, 50.0, 10.0));
    let b = add(d, Contour::circle(30.0, 50.0, 10.0));
    let c = add(d, Contour::circle(70.0, 50.0, 10.0));
    let line = add(d, Contour::line([50.0, 20.0], [50.0, 80.0]));
    d.add_constraint(Kind::Tangent, vec![seg(b, 0, 0), seg(a, 0, 0)]).unwrap();
    d.add_constraint(Kind::Tangent, vec![seg(c, 0, 0), seg(a, 0, 0)]).unwrap();
    d.settle(true).unwrap();
    (a, b, c, line)
}

#[test]
fn a_trimmed_circle_stays_one_arc_with_its_tangents() {
    let mut d = Document::new(200.0, 200.0);
    let (a, b, c, line) = three_circles(&mut d);
    d.checkpoint();
    d.trim(a, [41.0, 50.0], &[line], false).unwrap();
    d.settle(true).unwrap();
    let shape = d.item(a).unwrap().shape.contours().to_vec();
    assert_eq!((shape.len(), shape[0].vertices.len()), (1, 2), "one arc: {shape:?}");
    let arc = arcs(&d, a)[0];
    assert!(distance(arc.0, [50.0, 50.0]) < 1e-9 && (arc.1 - 10.0).abs() < 1e-9, "{arc:?}");
    let tangents: Vec<Vec<Ref>> = d.s.constraints.iter().filter(|k| k.kind == Kind::Tangent).map(|k| k.refs.clone()).collect();
    assert_eq!(tangents, vec![vec![seg(b, 0, 0), seg(a, 0, 0)], vec![seg(c, 0, 0), seg(a, 0, 0)]]);

    // Moving a neighbour drags the arc along; it stays one clean arc touching both.
    d.checkpoint();
    d.transform(&[c], &Transform::translate(5.0, 3.0));
    d.settle(true).unwrap();
    let arc = arcs(&d, a);
    assert_eq!(arc.len(), 1, "{arc:?}");
    assert!(touching(arc[0], arcs(&d, b)[0]) && touching(arc[0], arcs(&d, c)[0]), "{arc:?}");

    // Undo brings the circle back with its constraints.
    d.undo();
    d.settle(true).unwrap();
    d.undo();
    d.settle(true).unwrap();
    assert!(d.item(a).unwrap().shape.contours()[0].as_circle().is_some());
    assert_eq!(d.s.constraints.iter().filter(|k| k.kind == Kind::Tangent && k.refs[1] == seg(a, 0, 0)).count(), 2);
}

#[test]
fn pieces_of_one_circle_stay_on_one_circle() {
    let mut d = Document::new(200.0, 200.0);
    let (a, b, c, line) = three_circles(&mut d);
    d.trim(a, [41.0, 50.0], &[line], true).unwrap();
    d.settle(true).unwrap();
    // Cut the arc that is left in two: two pieces of the same circle, their ends bound to the lines.
    let h1 = add(&mut d, Contour::line([40.0, 47.0], [80.0, 47.0]));
    let h2 = add(&mut d, Contour::line([40.0, 53.0], [80.0, 53.0]));
    d.trim(a, [60.0, 50.0], &[h1, h2], true).unwrap();
    d.settle(true).unwrap();
    assert_eq!(d.item(a).unwrap().shape.contours().len(), 2);
    assert!(d.s.constraints.iter().any(|k| k.kind == Kind::Concentric) && d.s.constraints.iter().any(|k| k.kind == Kind::Equal));

    d.transform(&[c], &Transform::translate(4.0, 0.0));
    d.settle(true).unwrap();
    let pieces = arcs(&d, a);
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    assert!(distance(pieces[0].0, pieces[1].0) < 1e-6 && (pieces[0].1 - pieces[1].1).abs() < 1e-6, "one circle: {pieces:?}");
    assert!(touching(pieces[0], arcs(&d, b)[0]) && touching(pieces[0], arcs(&d, c)[0]), "{pieces:?}");
}

#[test]
fn trimmed_ends_stay_on_their_edges() {
    let mut d = Document::new(200.0, 200.0);
    let line = add(&mut d, Contour::line([0.0, 0.0], [30.0, 0.0]));
    let e1 = add(&mut d, Contour::line([10.0, -5.0], [10.0, 5.0]));
    let e2 = add(&mut d, Contour::line([20.0, -5.0], [20.0, 5.0]));
    d.trim(line, [15.0, 0.0], &[e1, e2], true).unwrap();
    d.settle(true).unwrap();
    let bound: Vec<Ref> = d.s.constraints.iter().filter(|k| k.kind == Kind::Coincident).map(|k| k.refs[0]).collect();
    assert_eq!(bound, vec![seg(e1, 0, 0), seg(e2, 0, 0)]);

    d.transform(&[e1], &Transform::translate(3.0, 0.0));
    d.settle(true).unwrap();
    let c = d.item(line).unwrap().shape.contours();
    assert!((c[0].vertices[1].x - 13.0).abs() < 1e-6, "{c:?}");
    assert!((c[1].vertices[0].x - 20.0).abs() < 1e-6, "{c:?}");

    // Without INFER nothing is added.
    let mut d = Document::new(200.0, 200.0);
    let line = add(&mut d, Contour::line([0.0, 0.0], [30.0, 0.0]));
    let e1 = add(&mut d, Contour::line([10.0, -5.0], [10.0, 5.0]));
    d.trim(line, [2.0, 0.0], &[e1], false).unwrap();
    assert!(d.s.constraints.is_empty());
}

#[test]
fn the_same_point_or_object_twice_is_refused() {
    let mut d = Document::new(200.0, 200.0);
    let l = add(&mut d, Contour::line([0.0, 0.0], [30.0, 0.0]));
    let p = Ref::Point { item: l, contour: 0, vertex: 0 };
    assert_eq!(d.add_constraint(Kind::Coincident, vec![p, p]), Err("Pick two different points".into()));
    assert_eq!(d.add_constraint(Kind::Parallel, vec![seg(l, 0, 0), seg(l, 0, 0)]), Err("Pick two different objects".into()));
    assert!(d.add_dimension(Kind::DimAligned, vec![p, p], None, None, 5.0).is_err());
    assert!(d.s.constraints.is_empty());
}
