//! How far each constraint is from satisfied (its residuals, 0 = satisfied; mm, or the sine/cosine
//! of an angle) and what a dimensional constraint measures.

use super::Kind;
use super::geo::{Compiled, G};
use crate::model::Point;
use crate::model::geom::distance;

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn unit(v: Point) -> Point {
    let len = v[0].hypot(v[1]).max(1e-12);
    [v[0] / len, v[1] / len]
}

/// Signed distance from `p` to the infinite line through `a` and `b`.
fn line_dist(p: Point, (a, b): (Point, Point)) -> f64 {
    cross(unit(sub(b, a)), sub(p, a))
}

/// Mirror of `p` over the line through `a` and `b`.
fn mirror(p: Point, (a, b): (Point, Point)) -> Point {
    let u = unit(sub(b, a));
    let t = dot(sub(p, a), u);
    let foot = [a[0] + u[0] * t, a[1] + u[1] * t];
    [2.0 * foot[0] - p[0], 2.0 * foot[1] - p[1]]
}

/// `p` and `q` mirrored over `l`: their midpoint on it, and the line between them across it.
fn symmetric(p: Point, q: Point, l: (Point, Point), out: &mut Vec<f64>) {
    let mid = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
    out.push(line_dist(mid, l));
    out.push(dot(sub(q, p), unit(sub(l.1, l.0))));
}

/// The tangent direction of a line or arc at its end `end` (0 or 1).
fn tangent_at(g: &G, x: &[f64], end: usize) -> Option<Point> {
    let ends = g.ends(x)?;
    Some(match g.circle(x) {
        Some((c, _)) => {
            let r = sub(ends[end], c);
            unit([-r[1], r[0]])
        }
        None => unit(sub(ends[1], ends[0])),
    })
}

/// Fill `out` with the residuals of `c` at `x`. False when the constraint does not apply to its geometry.
pub fn residuals(c: &Compiled, x: &[f64], out: &mut Vec<f64>) -> bool {
    let g = c.g.as_slice();
    let line = |i: usize| g.get(i).and_then(|g| g.line(x));
    let circle = |i: usize| g.get(i).and_then(|g| g.circle(x));
    let point = |i: usize| g.get(i).and_then(|g| g.point(x));
    match c.kind {
        Kind::Coincident => match (point(0), point(1)) {
            (Some(p), Some(q)) => out.extend(sub(p, q)),
            (Some(p), None) | (None, Some(p)) => {
                let other = if point(0).is_some() { 1 } else { 0 };
                match (line(other), circle(other)) {
                    (Some(l), _) => out.push(line_dist(p, l)),
                    (_, Some((m, r))) => out.push(distance(p, m) - r),
                    _ => return false,
                }
            }
            _ => return false,
        },
        Kind::Collinear => {
            let (Some(a), Some(b)) = (line(0), line(1)) else { return false };
            out.extend([line_dist(b.0, a), line_dist(b.1, a)]);
        }
        Kind::Concentric => {
            let (Some(a), Some(b)) = (circle(0), circle(1)) else { return false };
            out.extend(sub(a.0, b.0));
        }
        Kind::Fix => out.extend(c.vars.iter().zip(&c.target).map(|(i, t)| x[*i] - t)),
        Kind::Parallel | Kind::Perpendicular => {
            let (Some(a), Some(b)) = (line(0), line(1)) else { return false };
            let (u, v) = (unit(sub(a.1, a.0)), unit(sub(b.1, b.0)));
            out.push(if c.kind == Kind::Parallel { cross(u, v) } else { dot(u, v) });
        }
        Kind::Horizontal | Kind::Vertical => {
            let Some((p, q)) = line(0).or(point(0).zip(point(1))) else { return false };
            let axis = if c.kind == Kind::Horizontal { 1 } else { 0 };
            out.push(q[axis] - p[axis]);
        }
        Kind::Tangent => match (line(0), circle(0), line(1), circle(1)) {
            (Some(l), _, _, Some((m, r))) | (_, Some((m, r)), Some(l), _) => out.push(line_dist(m, l).abs() - r),
            (_, Some((m1, r1)), _, Some((m2, r2))) => {
                let d = distance(m1, m2);
                let (outside, inside) = (d - (r1 + r2), d - (r1 - r2).abs());
                out.push(if outside.abs() < inside.abs() { outside } else { inside });
            }
            _ => return false,
        },
        Kind::Smooth => {
            let (Some(ea), Some(eb)) = (g[0].ends(x), g[1].ends(x)) else { return false };
            // The joint: the closest pair of ends.
            let (i, j) = [(0, 0), (0, 1), (1, 0), (1, 1)].into_iter().min_by(|p, q| distance(ea[p.0], eb[p.1]).total_cmp(&distance(ea[q.0], eb[q.1]))).unwrap();
            out.extend(sub(ea[i], eb[j]));
            let (Some(ta), Some(tb)) = (tangent_at(&g[0], x, i), tangent_at(&g[1], x, j)) else { return false };
            out.push(cross(ta, tb));
            if let (Some(a), Some(b)) = (circle(0), circle(1)) {
                out.push(a.1 - b.1);
            }
        }
        Kind::Symmetric => {
            let Some(l) = line(2) else { return false };
            match (point(0), point(1), line(0), line(1), circle(0), circle(1)) {
                (Some(p), Some(q), ..) => symmetric(p, q, l, out),
                (_, _, Some(a), Some(b), ..) => {
                    // Pair the ends the way they are closest to mirrored.
                    let straight = distance(mirror(a.0, l), b.0) + distance(mirror(a.1, l), b.1);
                    let crossed = distance(mirror(a.0, l), b.1) + distance(mirror(a.1, l), b.0);
                    let (b0, b1) = if straight <= crossed { (b.0, b.1) } else { (b.1, b.0) };
                    symmetric(a.0, b0, l, out);
                    symmetric(a.1, b1, l, out);
                }
                (.., Some(a), Some(b)) => {
                    symmetric(a.0, b.0, l, out);
                    out.push(a.1 - b.1);
                }
                _ => return false,
            }
        }
        Kind::Equal => match (line(0), line(1), circle(0), circle(1)) {
            (Some(a), Some(b), ..) => out.push(distance(a.0, a.1) - distance(b.0, b.1)),
            (.., Some(a), Some(b)) => out.push(a.1 - b.1),
            _ => return false,
        },
        Kind::Regular => {
            let [G::Item(v)] = g else { return false };
            regular(&v.iter().map(|i| [x[*i], x[i + 1]]).collect::<Vec<_>>(), out);
        }
        _ => {
            let Some(m) = measure(c.kind, g, x) else { return false };
            let scale = if c.kind == Kind::DimAngular { std::f64::consts::PI / 180.0 } else { 1.0 };
            out.push((m - c.value) * scale);
        }
    }
    true
}

/// A closed polygon `v` regular: sides 1..n-2 as long as side 0, and the turns between sides
/// 0..n-2 all 360°/n (with the polygon's winding). That is 2n − 4 equations for the 2n − 4 ways a
/// polygon can change shape (rather than move, turn or scale); the last side follows from closing.
fn regular(v: &[Point], out: &mut Vec<f64>) {
    let n = v.len();
    let side = |k: usize| sub(v[(k + 1) % n], v[k]);
    let area: f64 = (0..n).map(|k| cross(v[k], v[(k + 1) % n])).sum();
    let turn = area.signum() * std::f64::consts::TAU / n as f64;
    let len0 = side(0)[0].hypot(side(0)[1]);
    for k in 1..n - 1 {
        out.push(side(k)[0].hypot(side(k)[1]) - len0);
    }
    for k in 0..n - 2 {
        let (a, b) = (side(k), side(k + 1));
        out.push(cross(a, b).atan2(dot(a, b)) - turn);
    }
}

/// What a dimensional constraint measures now: mm, or degrees for angular.
pub fn measure(kind: Kind, g: &[G], x: &[f64]) -> Option<f64> {
    let two_points = || match g {
        [a] => a.line(x),
        [a, b] => a.point(x).zip(b.point(x)),
        _ => None,
    };
    match kind {
        Kind::DimHorizontal => two_points().map(|(p, q)| (q[0] - p[0]).abs()),
        Kind::DimVertical => two_points().map(|(p, q)| (q[1] - p[1]).abs()),
        Kind::DimAligned => match g {
            [a, b] if a.point(x).is_some() && b.line(x).is_some() => Some(line_dist(a.point(x)?, b.line(x)?).abs()),
            [a, b] if b.point(x).is_some() && a.line(x).is_some() => Some(line_dist(b.point(x)?, a.line(x)?).abs()),
            _ => two_points().map(|(p, q)| distance(p, q)),
        },
        Kind::DimAngular => {
            let [a, b] = g else { return None };
            let (a, b) = (a.line(x)?, b.line(x)?);
            let (u, v) = corner_rays(a, b);
            Some(cross(u, v).abs().atan2(dot(u, v)).to_degrees())
        }
        Kind::DimRadial => g.first()?.circle(x).map(|c| c.1),
        Kind::DimDiameter => g.first()?.circle(x).map(|c| 2.0 * c.1),
        _ => None,
    }
}

/// Where two lines meet (None when parallel).
pub fn intersection(a: (Point, Point), b: (Point, Point)) -> Option<Point> {
    let (r, s) = (sub(a.1, a.0), sub(b.1, b.0));
    let den = cross(r, s);
    if den.abs() < 1e-12 * (dot(r, r) * dot(s, s)).sqrt().max(1e-12) {
        return None;
    }
    let t = cross(sub(b.0, a.0), s) / den;
    Some([a.0[0] + r[0] * t, a.0[1] + r[1] * t])
}

/// The directions from the lines' intersection towards each line's far end: the corner an angle
/// is measured in. Parallel lines just give their directions.
pub fn corner_rays(a: (Point, Point), b: (Point, Point)) -> (Point, Point) {
    let Some(i) = intersection(a, b) else { return (unit(sub(a.1, a.0)), unit(sub(b.1, b.0))) };
    let far = |l: (Point, Point)| if distance(l.0, i) > distance(l.1, i) { l.0 } else { l.1 };
    (unit(sub(far(a), i)), unit(sub(far(b), i)))
}
