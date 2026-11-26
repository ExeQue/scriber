//! AUTOCONSTRAIN: the constraints a drawing already (nearly) satisfies. Each relation is added once
//! per group (three parallel lines get two parallel constraints, not three), and none that a
//! horizontal/vertical pair already implies.

use super::{Kind, Ref};
use crate::model::geom::{arc_params, distance};
use crate::model::{Point, Shape, State};
use std::collections::HashSet;

struct Line {
    r: Ref,
    a: Point,
    b: Point,
}

struct Arc {
    r: Ref,
    c: Point,
    radius: f64,
    /// None for a full circle.
    ends: Option<[Point; 2]>,
}

/// Union-find with "already joined?" in one call.
struct Sets(Vec<usize>);

impl Sets {
    fn new(n: usize) -> Self {
        Self((0..n).collect())
    }

    fn root(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            i = self.0[i];
        }
        i
    }

    /// Join the sets of `a` and `b`; false when they were already one.
    fn join(&mut self, a: usize, b: usize) -> bool {
        let (ra, rb) = (self.root(a), self.root(b));
        self.0[ra] = rb;
        ra != rb
    }
}

/// Constraints to add for `items`: (kind, references). `tol` in mm, `angle_tol` in degrees.
pub fn infer(s: &State, items: &[u64], tol: f64, angle_tol: f64) -> Vec<(Kind, Vec<Ref>)> {
    let (mut points, mut lines, mut arcs) = (Vec::new(), Vec::new(), Vec::new());
    for item in s.items.iter().filter(|i| items.contains(&i.id)) {
        let Shape::Path { contours } = &item.shape else { continue };
        for (ci, c) in contours.iter().enumerate() {
            let n = c.vertices.len();
            let p = |k: usize| [c.vertices[k % n].x, c.vertices[k % n].y];
            if let Some((centre, radius)) = c.as_circle() {
                arcs.push(Arc { r: Ref::Segment { item: item.id, contour: ci, segment: 0 }, c: centre, radius, ends: None });
                continue;
            }
            for k in 0..n {
                points.push((Ref::Point { item: item.id, contour: ci, vertex: k }, p(k), (item.id, ci)));
            }
            let segments = if c.closed { n } else { n.saturating_sub(1) };
            for k in 0..segments {
                let r = Ref::Segment { item: item.id, contour: ci, segment: k };
                let bulge = c.vertices[k].bulge;
                if bulge.abs() < 1e-12 {
                    if distance(p(k), p(k + 1)) > tol {
                        lines.push(Line { r, a: p(k), b: p(k + 1) });
                    }
                } else {
                    let (centre, radius, _, _) = arc_params(p(k), p(k + 1), bulge);
                    arcs.push(Arc { r, c: centre, radius, ends: Some([p(k), p(k + 1)]) });
                }
            }
        }
    }
    let mut out = Vec::new();
    let sin_tol = angle_tol.to_radians().sin();

    // Coincident ends of different contours.
    let mut joined = Sets::new(points.len());
    for i in 0..points.len() {
        for j in 0..i {
            if points[i].2 != points[j].2 && distance(points[i].1, points[j].1) <= tol && joined.join(i, j) {
                out.push((Kind::Coincident, vec![points[j].0, points[i].0]));
            }
        }
    }

    // Directions: classes of parallel lines; H and V are the last two nodes.
    let (h, v) = (lines.len(), lines.len() + 1);
    let mut dirs = Sets::new(lines.len() + 2);
    for (i, l) in lines.iter().enumerate() {
        let (dx, dy, len) = (l.b[0] - l.a[0], l.b[1] - l.a[1], distance(l.a, l.b));
        if dy.abs() <= len * sin_tol {
            dirs.join(i, h);
            out.push((Kind::Horizontal, vec![l.r]));
        } else if dx.abs() <= len * sin_tol {
            dirs.join(i, v);
            out.push((Kind::Vertical, vec![l.r]));
        }
    }
    let unit = |l: &Line| {
        let len = distance(l.a, l.b);
        [(l.b[0] - l.a[0]) / len, (l.b[1] - l.a[1]) / len]
    };
    for i in 0..lines.len() {
        for j in 0..i {
            let (u, w) = (unit(&lines[i]), unit(&lines[j]));
            if (u[0] * w[1] - u[1] * w[0]).abs() <= sin_tol && dirs.join(i, j) {
                out.push((Kind::Parallel, vec![lines[j].r, lines[i].r]));
            }
        }
    }
    let touch = |a: [Point; 2], b: [Point; 2]| a.iter().any(|p| b.iter().any(|q| distance(*p, *q) <= tol));
    let mut perpendicular: HashSet<(usize, usize)> = HashSet::from([(dirs.root(h), dirs.root(v)), (dirs.root(v), dirs.root(h))]);
    for i in 0..lines.len() {
        for j in 0..i {
            let (u, w) = (unit(&lines[i]), unit(&lines[j]));
            let (ri, rj) = (dirs.root(i), dirs.root(j));
            if (u[0] * w[0] + u[1] * w[1]).abs() <= sin_tol && touch([lines[i].a, lines[i].b], [lines[j].a, lines[j].b]) && perpendicular.insert((ri, rj)) {
                perpendicular.insert((rj, ri));
                out.push((Kind::Perpendicular, vec![lines[j].r, lines[i].r]));
            }
        }
    }

    // Tangent: touching at a shared end (or a full circle touching anything).
    for l in &lines {
        for a in &arcs {
            let u = unit(l);
            let off = (u[0] * (a.c[1] - l.a[1]) - u[1] * (a.c[0] - l.a[0])).abs();
            if (off - a.radius).abs() <= tol && a.ends.is_none_or(|e| touch(e, [l.a, l.b])) {
                out.push((Kind::Tangent, vec![l.r, a.r]));
            }
        }
    }
    for i in 0..arcs.len() {
        for j in 0..i {
            let (a, b) = (&arcs[i], &arcs[j]);
            let d = distance(a.c, b.c);
            let tangent = d > tol && ((d - a.radius - b.radius).abs() <= tol || (d - (a.radius - b.radius).abs()).abs() <= tol);
            let shared = match (a.ends, b.ends) {
                (Some(x), Some(y)) => touch(x, y),
                _ => true,
            };
            if tangent && shared {
                out.push((Kind::Tangent, vec![b.r, a.r]));
            }
        }
    }

    // Concentric, then equal radii and equal lengths (not between parallel lines, where a closed
    // shape usually implies it already).
    let mut centres = Sets::new(arcs.len());
    let mut radii = Sets::new(arcs.len());
    for i in 0..arcs.len() {
        for j in 0..i {
            if distance(arcs[i].c, arcs[j].c) <= tol && centres.join(i, j) {
                out.push((Kind::Concentric, vec![arcs[j].r, arcs[i].r]));
            }
            if (arcs[i].radius - arcs[j].radius).abs() <= tol && radii.join(i, j) {
                out.push((Kind::Equal, vec![arcs[j].r, arcs[i].r]));
            }
        }
    }
    let mut lengths = Sets::new(lines.len());
    for i in 0..lines.len() {
        for j in 0..i {
            let same = (distance(lines[i].a, lines[i].b) - distance(lines[j].a, lines[j].b)).abs() <= tol;
            if same && dirs.root(i) != dirs.root(j) && lengths.join(i, j) {
                out.push((Kind::Equal, vec![lines[j].r, lines[i].r]));
            }
        }
    }
    out
}
