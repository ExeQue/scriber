//! Two-object fillet and chamfer on real geometry: tangency, trimmed ends and exact sizes.

use super::{Edit, chamfer_objects, fillet_objects};
use crate::model::geom::{Contour, Point, Vertex, arc_params, distance};
use crate::model::{Document, Op, Shape};

const TOL: f64 = 1e-6;

fn near(a: Point, b: Point) -> bool {
    distance(a, b) < TOL
}

fn pt(v: &Vertex) -> Point {
    [v.x, v.y]
}

/// Direction a segment leaves `a` and arrives at `b`.
fn dirs(a: Vertex, b: Vertex) -> (Point, Point) {
    if a.bulge == 0.0 {
        let d = distance(pt(&a), pt(&b));
        let u = [(b.x - a.x) / d, (b.y - a.y) / d];
        return (u, u);
    }
    let (c, r, _, _) = arc_params(pt(&a), pt(&b), a.bulge);
    let s = a.bulge.signum();
    let t = |p: Point| [-s * (p[1] - c[1]) / r, s * (p[0] - c[0]) / r];
    (t(pt(&a)), t(pt(&b)))
}

/// Every junction of `c` is smooth (no kink).
fn assert_smooth(c: &Contour) {
    let v = &c.vertices;
    let n = v.len();
    let segs = if c.closed { n } else { n - 1 };
    for i in 1..segs + usize::from(c.closed) {
        let (_, end) = dirs(v[i - 1], v[i % n]);
        let (start, _) = dirs(v[i % n], v[(i + 1) % n]);
        assert!(near(end, start), "kink at vertex {i} of {c:?}");
    }
}

/// The centre and radius of segment `i`.
fn arc(c: &Contour, i: usize) -> (Point, f64) {
    let (a, b) = (c.vertices[i], c.vertices[(i + 1) % c.vertices.len()]);
    assert!(a.bulge != 0.0, "segment {i} is not an arc");
    let (centre, r, _, _) = arc_params(pt(&a), pt(&b), a.bulge);
    (centre, r)
}

fn joined(e: Edit) -> Contour {
    match e {
        Edit::Joined(c) => c,
        other => panic!("not joined: {other:?}"),
    }
}

fn line(a: Point, b: Point) -> Vec<Contour> {
    vec![Contour::line(a, b)]
}

#[test]
fn line_line_crossing_keeps_the_picked_quadrant() {
    let (h, v) = (line([0.0, 0.0], [20.0, 0.0]), line([10.0, -10.0], [10.0, 10.0]));
    let c = joined(fillet_objects((&h, [2.0, 0.0]), (&v, [10.0, 8.0]), 3.0).unwrap());
    let pts: Vec<Point> = c.vertices.iter().map(pt).collect();
    assert!([[0.0, 0.0], [7.0, 0.0], [10.0, 3.0], [10.0, 10.0]].iter().zip(&pts).all(|(a, b)| near(*a, *b)), "{pts:?}");
    let (centre, r) = arc(&c, 1);
    assert!(near(centre, [7.0, 3.0]) && (r - 3.0).abs() < TOL);
    assert_smooth(&c);
    // The other side of each line: the arc moves into the opposite quadrant.
    let c = joined(fillet_objects((&h, [18.0, 0.0]), (&v, [10.0, -8.0]), 3.0).unwrap());
    assert!(near(arc(&c, 1).0, [13.0, -3.0]));
    assert!(near(pt(&c.vertices[0]), [20.0, 0.0]) && near(pt(&c.vertices[3]), [10.0, -10.0]));
    assert_smooth(&c);
}

#[test]
fn lines_are_extended_and_radius_zero_makes_a_sharp_corner() {
    let (a, b) = (line([0.0, 0.0], [8.0, 0.0]), line([10.0, 10.0], [10.0, 3.0]));
    let c = joined(fillet_objects((&a, [4.0, 0.0]), (&b, [10.0, 6.0]), 2.0).unwrap());
    assert!(near(pt(&c.vertices[1]), [8.0, 0.0]) && near(pt(&c.vertices[2]), [10.0, 2.0]));
    assert!((arc(&c, 1).1 - 2.0).abs() < TOL);
    assert_smooth(&c);
    let c = joined(fillet_objects((&a, [4.0, 0.0]), (&b, [10.0, 6.0]), 0.0).unwrap());
    let pts: Vec<Point> = c.vertices.iter().map(pt).collect();
    assert_eq!(pts, vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]);
    assert!(fillet_objects((&a, [4.0, 0.0]), (&b, [10.0, 6.0]), 30.0).unwrap_err().contains("radius 30 is too large"));
}

#[test]
fn line_arc_and_arc_arc_are_tangent() {
    // A line under an arc bulging towards it.
    let l = line([0.0, 0.0], [30.0, 0.0]);
    let a = vec![Contour::arc(15.0, 10.0, 5.0, 180.0, 360.0)];
    let c = joined(fillet_objects((&l, [5.0, 0.0]), (&a, [15.0, 5.0]), 3.0).unwrap());
    let i = (0..c.vertices.len() - 1).find(|i| c.vertices[*i].bulge != 0.0).unwrap();
    let (centre, r) = arc(&c, i);
    assert!((r - 3.0).abs() < TOL && (centre[1] - 3.0).abs() < TOL && (distance(centre, [15.0, 10.0]) - 8.0).abs() < TOL);
    assert!(near(pt(&c.vertices[0]), [0.0, 0.0]) && near(pt(&c.vertices[i]), [centre[0], 0.0]));
    assert_smooth(&c);

    // Two arcs crossing at (8, -6), rounded above the crossing.
    let a1 = vec![Contour::arc(0.0, 0.0, 10.0, 270.0, 360.0)];
    let a2 = vec![Contour::arc(16.0, 0.0, 10.0, 180.0, 270.0)];
    let c = joined(fillet_objects((&a1, [3.420201, -9.396926]), (&a2, [12.579799, -9.396926]), 2.0).unwrap());
    assert_eq!(c.vertices.len(), 4);
    let (centre, r) = arc(&c, 1);
    assert!((r - 2.0).abs() < TOL && (distance(centre, [0.0, 0.0]) - 12.0).abs() < TOL && (distance(centre, [16.0, 0.0]) - 12.0).abs() < TOL);
    assert!(centre[1] < -6.0, "the fillet is on the picked side: {centre:?}");
    assert!(near(pt(&c.vertices[0]), [0.0, -10.0]) && near(pt(&c.vertices[3]), [16.0, -10.0]));
    assert_smooth(&c);
}

#[test]
fn circles_are_not_trimmed() {
    // Line and circle: the line is trimmed to the arc, which is a new object.
    let l = line([0.0, 0.0], [30.0, 0.0]);
    let circle = vec![Contour::circle(20.0, 10.0, 5.0)];
    let Edit::Trimmed { a: Some(a), b: None, bridge: Some(br) } = fillet_objects((&l, [5.0, 0.0]), (&circle, [20.0, 5.0]), 3.0).unwrap() else { panic!() };
    let (centre, r) = arc(&br, 0);
    assert!((r - 3.0).abs() < TOL && (distance(centre, [20.0, 10.0]) - 8.0).abs() < TOL && (centre[1] - 3.0).abs() < TOL);
    let (t1, t2) = (pt(&br.vertices[0]), pt(&br.vertices[1]));
    assert!(near(pt(&a[0].vertices[1]), t1) && near(pt(&a[0].vertices[0]), [0.0, 0.0]));
    assert!((distance(t2, [20.0, 10.0]) - 5.0).abs() < TOL);
    assert_smooth(&Contour { vertices: vec![a[0].vertices[0], br.vertices[0], br.vertices[1]], closed: false });

    // Two circles side by side, picked on their upper sides: an outside arc bridging them on top.
    let (c1, c2) = (vec![Contour::circle(0.0, 0.0, 5.0)], vec![Contour::circle(20.0, 0.0, 5.0)]);
    let Edit::Trimmed { a: None, b: None, bridge: Some(br) } = fillet_objects((&c1, [3.0, -4.0]), (&c2, [17.0, -4.0]), 6.0).unwrap() else { panic!() };
    let (centre, r) = arc(&br, 0);
    assert!((r - 6.0).abs() < TOL && near(centre, [10.0, -(21f64.sqrt())]));
    assert!((distance(pt(&br.vertices[0]), [0.0, 0.0]) - 5.0).abs() < TOL && (distance(pt(&br.vertices[1]), [20.0, 0.0]) - 5.0).abs() < TOL);

    // An arc and a circle: the arc is trimmed, the circle stays.
    let a = vec![Contour::arc(0.0, 0.0, 10.0, 270.0, 360.0)];
    let circle = vec![Contour::circle(16.0, 0.0, 10.0)];
    let Edit::Trimmed { a: Some(na), b: None, bridge: Some(br) } = fillet_objects((&a, [3.420201, -9.396926]), (&circle, [12.579799, -9.396926]), 2.0).unwrap() else { panic!() };
    assert!(near(pt(&na[0].vertices[1]), pt(&br.vertices[0])));
    assert_smooth(&Contour { vertices: vec![na[0].vertices[0], br.vertices[0], br.vertices[1]], closed: false });
}

#[test]
fn polyline_end_segments_and_parallel_lines() {
    // The last segment of a polyline meets a line; the polyline's other corner is kept.
    let pl = vec![Contour::polyline(&[[0.0, 10.0], [0.0, 0.0], [10.0, 0.0]], false)];
    let l = line([15.0, 5.0], [15.0, 20.0]);
    let c = joined(fillet_objects((&pl, [5.0, 0.0]), (&l, [15.0, 10.0]), 2.0).unwrap());
    let pts: Vec<Point> = c.vertices.iter().map(pt).collect();
    assert!([[0.0, 10.0], [0.0, 0.0], [13.0, 0.0], [15.0, 2.0], [15.0, 20.0]].iter().zip(&pts).all(|(a, b)| near(*a, *b)), "{pts:?}");
    let long = vec![Contour::polyline(&[[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [20.0, 10.0]], false)];
    assert!(fillet_objects((&long, [15.0, 0.0]), (&l, [15.0, 10.0]), 2.0).unwrap_err().contains("middle segment"));

    // Parallel lines: a half circle at the first line's picked end; the second line is cut to match.
    let (a, b) = (line([0.0, 0.0], [10.0, 0.0]), line([0.0, 4.0], [15.0, 4.0]));
    let c = joined(fillet_objects((&a, [9.0, 0.0]), (&b, [5.0, 4.0]), 1.0).unwrap());
    let pts: Vec<Point> = c.vertices.iter().map(pt).collect();
    assert_eq!(pts, vec![[0.0, 0.0], [10.0, 0.0], [10.0, 4.0], [0.0, 4.0]]);
    let (centre, r) = arc(&c, 1);
    assert!(near(centre, [10.0, 2.0]) && (r - 2.0).abs() < TOL);
    assert!(c.bbox()[2] > 11.9, "the cap bulges outwards");
    assert_smooth(&c);
}

#[test]
fn two_end_segments_of_one_path_close_it() {
    // The end segments head towards each other and meet at (0, 40).
    let pl = vec![Contour::polyline(&[[0.0, 10.0], [0.0, 0.0], [20.0, 0.0], [15.0, 10.0]], false)];
    let Edit::Trimmed { a: Some(c), b: None, bridge: None } = fillet_objects((&pl, [0.0, 8.0]), (&pl, [16.0, 8.0]), 2.0).unwrap() else { panic!() };
    let c = &c[0];
    assert!(c.closed && c.vertices.len() == 4);
    let i = c.vertices.iter().position(|v| v.bulge != 0.0).unwrap();
    let (centre, r) = arc(c, i);
    assert!((r - 2.0).abs() < TOL && (centre[0] - 2.0).abs() < TOL && centre[1] > 30.0);
    let (_, into) = dirs(c.vertices[(i + 3) % 4], c.vertices[i]);
    assert!(near(into, dirs(c.vertices[i], c.vertices[(i + 1) % 4]).0), "tangent where the first segment meets the arc");
    // Parallel end segments: a half circle closes the path.
    let u = vec![Contour::polyline(&[[0.0, 10.0], [0.0, 0.0], [20.0, 0.0], [20.0, 10.0]], false)];
    let Edit::Trimmed { a: Some(c), .. } = fillet_objects((&u, [0.0, 8.0]), (&u, [20.0, 8.0]), 1.0).unwrap() else { panic!() };
    assert!(c[0].closed && (c[0].area() - (200.0 + std::f64::consts::PI * 50.0)).abs() < 1e-6);
    // Adjacent segments of one path: that corner is rounded.
    let Edit::Trimmed { a: Some(c), .. } = fillet_objects((&pl, [10.0, 0.0]), (&pl, [0.0, 5.0]), 2.0).unwrap() else { panic!() };
    assert_eq!(c[0].vertices.len(), 5);
}

#[test]
fn chamfer_lines_and_arcs() {
    let (a, b) = (line([0.0, 0.0], [8.0, 0.0]), line([10.0, 10.0], [10.0, 2.0]));
    let c = joined(chamfer_objects((&a, [4.0, 0.0]), (&b, [10.0, 6.0]), 3.0, 1.0).unwrap());
    let pts: Vec<Point> = c.vertices.iter().map(pt).collect();
    assert_eq!(pts, vec![[0.0, 0.0], [7.0, 0.0], [10.0, 1.0], [10.0, 10.0]]);
    assert!(chamfer_objects((&a, [4.0, 0.0]), (&b, [10.0, 6.0]), 11.0, 1.0).unwrap_err().contains("too long for the first"));
    let circle = vec![Contour::circle(0.0, 0.0, 5.0)];
    assert!(chamfer_objects((&a, [4.0, 0.0]), (&circle, [5.0, 0.0]), 1.0, 1.0).unwrap_err().contains("not circles"));

    // An arc crossing a line: the distance on the arc is measured along it.
    let arc_c = vec![Contour::arc(0.0, 10.0, 10.0, 180.0, 360.0)];
    let l = line([-20.0, 5.0], [20.0, 5.0]);
    let c = joined(chamfer_objects((&arc_c, [0.0, 0.0]), (&l, [15.0, 5.0]), 2.0, 3.0).unwrap());
    // The crossing on the right is at angle -30° (x = 10 cos 30°); the arc ends 2 mm of arc before it.
    let x = [10.0 * 30f64.to_radians().cos(), 5.0];
    let n = c.vertices.len();
    let p1 = pt(&c.vertices[n - 3]);
    let back = 2.0 / 10.0;
    assert!(near(p1, [10.0 * (-30f64.to_radians() - back).cos(), 10.0 + 10.0 * (-30f64.to_radians() - back).sin()]), "{p1:?}");
    assert!(near(pt(&c.vertices[n - 2]), [x[0] + 3.0, 5.0]) && near(pt(&c.vertices[n - 1]), [20.0, 5.0]));
    assert!(near(pt(&c.vertices[0]), [-10.0, 10.0]));
}

#[test]
fn document_fillet_objects_ids_chains_and_errors() {
    let mut d = Document::new(200.0, 200.0);
    let l = d.add("l", Op::Cut, Shape::path(Contour::line([0.0, 0.0], [30.0, 0.0])));
    let c = d.add("c", Op::Score, Shape::path(Contour::circle(20.0, 10.0, 5.0)));
    let ids = d.fillet_objects((l, [5.0, 0.0]), (c, [20.0, 5.0]), 3.0).unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], l);
    assert_eq!(d.s.op(d.item(ids[1]).unwrap()), Op::Cut);
    // An error changes nothing.
    let before = d.s.items.clone();
    assert!(d.fillet_objects((l, [5.0, 0.0]), (c, [20.0, 5.0]), 500.0).is_err());
    assert_eq!(d.s.items, before);

    // Two lines drawn as one L (a chain): the corner between them is rounded and they become one path.
    let mut d = Document::new(200.0, 200.0);
    let a = d.add("a", Op::Cut, Shape::path(Contour::line([0.0, 0.0], [20.0, 0.0])));
    let b = d.add("b", Op::Cut, Shape::path(Contour::line([20.0, 20.0], [20.0, 0.0])));
    let ids = d.chamfer_objects((a, [10.0, 0.0]), (b, [20.0, 10.0]), 2.0, 3.0).unwrap();
    assert_eq!(ids, vec![a]);
    assert_eq!(d.s.items.len(), 1);
    let pts: Vec<Point> = d.s.items[0].shape.contours()[0].vertices.iter().map(pt).collect();
    assert_eq!(pts, vec![[0.0, 0.0], [18.0, 0.0], [20.0, 3.0], [20.0, 20.0]]);
}
