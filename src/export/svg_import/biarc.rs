//! Cubic Béziers as arcs (biarc fitting), because a laser runs arcs more smoothly than many short lines.
//!
//! Each cubic gets one biarc: two arcs that meet tangentially, leaving the start and arriving at
//! the end with the curve's own tangents (joint by the equal-tangent-length rule). When the biarc
//! strays more than the tolerance from the curve, the curve is halved and each half is fitted
//! again; very deep splits fall back to straight lines.

use crate::model::geom::{Point, Vertex, arc_params, distance, segment_distance};
use std::f64::consts::PI;

/// Splits before giving up on arcs and drawing a line (1/4096 of the curve).
const MAX_DEPTH: u32 = 12;
/// Curve samples used to measure how far a biarc strays.
const SAMPLES: usize = 16;

/// A fitted piece: an arc (or line, bulge 0) from the previous point to `to`.
pub type Seg = (Point, f64);

/// Append the segments that follow the cubic `p` (from `p[0]`) within `tol` mm.
pub fn fit_cubic(p: [Point; 4], tol: f64, out: &mut Vec<Seg>) {
    fit(p, tol, 0, out);
}

fn fit(p: [Point; 4], tol: f64, depth: u32, out: &mut Vec<Seg>) {
    let flat = segment_distance(p[1], p[0], p[3]).max(segment_distance(p[2], p[0], p[3])) <= tol;
    if (flat && distance(p[0], p[3]) > 0.0) || depth >= MAX_DEPTH {
        out.push((p[3], 0.0));
        return;
    }
    if let Some(arcs) = biarc(p).filter(|arcs| deviation(p, arcs) <= tol) {
        out.extend(arcs);
        return;
    }
    let (a, b) = split(p);
    fit(a, tol, depth + 1, out);
    fit(b, tol, depth + 1, out);
}

/// The biarc from `p[0]` to `p[3]` with the curve's end tangents, if there is one.
fn biarc(p: [Point; 4]) -> Option<[Seg; 2]> {
    let t0 = unit(sub(if distance(p[0], p[1]) > 1e-12 { p[1] } else { p[2] }, p[0]))?;
    let t1 = unit(sub(p[3], if distance(p[2], p[3]) > 1e-12 { p[2] } else { p[1] }))?;
    let v = sub(p[3], p[0]);
    let t = [t0[0] + t1[0], t0[1] + t1[1]];
    // Equal tangent lengths d: |v - d·(t0 + t1)| = 2d.
    let denom = 2.0 * (1.0 - dot(t0, t1));
    let d = if denom.abs() < 1e-12 {
        dot(v, v) / (2.0 * dot(v, t1))
    } else {
        (-dot(v, t) + (dot(v, t).powi(2) + denom * dot(v, v)).sqrt()) / denom
    };
    if !(d.is_finite() && d > 0.0) {
        return None;
    }
    let q0 = [p[0][0] + d * t0[0], p[0][1] + d * t0[1]];
    let q1 = [p[3][0] - d * t1[0], p[3][1] - d * t1[1]];
    let joint = [(q0[0] + q1[0]) / 2.0, (q0[1] + q1[1]) / 2.0];
    let tj = unit(sub(q1, q0)).unwrap_or(t0);
    Some([(joint, bulge(t0, sub(joint, p[0]))), (p[3], bulge(tj, sub(p[3], joint)))])
}

/// Bulge of the arc that leaves along `tangent` and spans `chord`: the sweep is twice their angle.
fn bulge(tangent: Point, chord: Point) -> f64 {
    let half_sweep = cross(tangent, chord).atan2(dot(tangent, chord));
    (half_sweep / 2.0).tan()
}

/// Largest distance between the curve and the biarc, both ways (sampled).
fn deviation(p: [Point; 4], arcs: &[Seg; 2]) -> f64 {
    let curve: Vec<Point> = (0..=SAMPLES).map(|i| eval(p, i as f64 / SAMPLES as f64)).collect();
    let pieces = [(p[0], arcs[0]), (arcs[0].0, arcs[1])];
    let curve_to_arcs = curve.iter().map(|q| pieces.iter().map(|(a, (b, bulge))| arc_distance(*q, *a, *b, *bulge)).fold(f64::MAX, f64::min));
    let arcs_to_curve = pieces.iter().flat_map(|(a, (b, bulge))| [0.25, 0.5, 0.75].map(|f| arc_point(*a, *b, *bulge, f))).map(|q| {
        curve.windows(2).map(|w| segment_distance(q, w[0], w[1])).fold(f64::MAX, f64::min)
    });
    curve_to_arcs.chain(arcs_to_curve).fold(0.0, f64::max)
}

/// Distance from `q` to the arc (or line) from `a` to `b`.
pub fn arc_distance(q: Point, a: Point, b: Point, bulge: f64) -> f64 {
    if bulge.abs() < 1e-12 {
        return segment_distance(q, a, b);
    }
    let (c, r, start, sweep) = arc_params(a, b, bulge);
    let angle = (q[1] - c[1]).atan2(q[0] - c[0]);
    let along = if sweep > 0.0 { (angle - start).rem_euclid(2.0 * PI) } else { (start - angle).rem_euclid(2.0 * PI) };
    if along <= sweep.abs() { (distance(q, c) - r).abs() } else { distance(q, a).min(distance(q, b)) }
}

/// The point a fraction `f` along the arc (or line) from `a` to `b`.
pub fn arc_point(a: Point, b: Point, bulge: f64, f: f64) -> Point {
    if bulge.abs() < 1e-12 {
        return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
    }
    let (c, r, start, sweep) = arc_params(a, b, bulge);
    let t = start + sweep * f;
    [c[0] + r * t.cos(), c[1] + r * t.sin()]
}

/// Add a segment to a contour being built, dropping zero-length ones.
pub fn push(v: &mut Vec<Vertex>, to: Point, bulge: f64) {
    let last = v.last_mut().expect("a contour starts with a point");
    if distance([last.x, last.y], to) < 1e-9 {
        return;
    }
    last.bulge = bulge;
    v.push(Vertex::new(to[0], to[1], 0.0));
}

/// Runs of arcs on one circle (within `tol`) become one arc, split into equal parts of at most
/// 270° (a full circle: two halves). ponytail: a run that wraps past the start of a closed contour stays split there.
pub fn merge_arcs(v: Vec<Vertex>, closed: bool, tol: f64) -> Vec<Vertex> {
    let n = v.len();
    if n < 2 {
        return v;
    }
    let xy = |i: usize| [v[i % n].x, v[i % n].y];
    let segs = if closed { n } else { n - 1 };
    let mut out = Vec::new();
    let mut i = 0;
    while i < segs {
        if v[i].bulge == 0.0 {
            out.push(v[i]);
            i += 1;
            continue;
        }
        let (c, r, start, mut sweep) = arc_params(xy(i), xy(i + 1), v[i].bulge);
        let mut j = i + 1;
        while j < segs && v[j].bulge * v[i].bulge > 0.0 {
            let (c2, r2, _, s2) = arc_params(xy(j), xy(j + 1), v[j].bulge);
            if distance(c, c2) > tol / 2.0 || (r - r2).abs() > tol / 2.0 {
                break;
            }
            sweep += s2;
            j += 1;
        }
        let parts = (sweep.abs() / (1.5 * PI) - 1e-9).ceil().max(1.0);
        let bulge = (sweep / parts / 4.0).tan();
        for k in 0..parts as usize {
            let t = start + sweep * k as f64 / parts;
            let p = if k == 0 { xy(i) } else { [c[0] + r * t.cos(), c[1] + r * t.sin()] };
            out.push(Vertex::new(p[0], p[1], bulge));
        }
        i = j;
    }
    if !closed {
        out.push(Vertex::new(v[n - 1].x, v[n - 1].y, 0.0));
    }
    out
}

/// de Casteljau split at t = 0.5.
fn split(p: [Point; 4]) -> ([Point; 4], [Point; 4]) {
    let mid = |a: Point, b: Point| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let (ab, bc, cd) = (mid(p[0], p[1]), mid(p[1], p[2]), mid(p[2], p[3]));
    let (abc, bcd) = (mid(ab, bc), mid(bc, cd));
    let m = mid(abc, bcd);
    ([p[0], ab, abc, m], [m, bcd, cd, p[3]])
}

fn eval(p: [Point; 4], t: f64) -> Point {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    [0, 1].map(|k| (0..4).map(|i| w[i] * p[i][k]).sum())
}

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn unit(a: Point) -> Option<Point> {
    let l = a[0].hypot(a[1]);
    (l > 1e-12).then(|| [a[0] / l, a[1] / l])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fit the cubics as one contour (starting at the first one's start).
    fn fit_all(cubics: &[[Point; 4]], tol: f64) -> Vec<Vertex> {
        let mut v = vec![Vertex::new(cubics[0][0][0], cubics[0][0][1], 0.0)];
        for c in cubics {
            let mut segs = Vec::new();
            fit_cubic(*c, tol, &mut segs);
            for (to, b) in segs {
                push(&mut v, to, b);
            }
        }
        merge_arcs(v, false, tol)
    }

    /// Largest distance from the curve to the fitted segments, sampled densely.
    fn max_deviation(c: [Point; 4], v: &[Vertex]) -> f64 {
        (0..=500)
            .map(|i| eval(c, i as f64 / 500.0))
            .map(|q| v.windows(2).map(|w| arc_distance(q, [w[0].x, w[0].y], [w[1].x, w[1].y], w[0].bulge)).fold(f64::MAX, f64::min))
            .fold(0.0, f64::max)
    }

    #[test]
    fn s_curves_and_loops_stay_within_the_tolerance() {
        let curves = [
            [[0.0, 0.0], [30.0, -40.0], [60.0, 40.0], [90.0, 0.0]],  // S-curve (inflection)
            [[0.0, 0.0], [100.0, 0.0], [0.0, 50.0], [80.0, 60.0]],   // sharp bend
            [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [30.0, 0.0]],     // straight
            [[0.0, 0.0], [60.0, -60.0], [-60.0, -60.0], [5.0, 0.0]], // near loop
        ];
        for tol in [0.1, 0.01, 0.001] {
            for c in curves {
                let v = fit_all(&[c], tol);
                let dev = max_deviation(c, &v);
                assert!(dev <= tol * 1.05, "deviation {dev} > {tol} for {c:?}");
                let end = v.last().unwrap();
                assert!(distance([end.x, end.y], c[3]) < 1e-9);
            }
        }
        assert_eq!(fit_all(&[curves[2]], 0.01).len(), 2, "a straight cubic is one line");
        assert!(fit_all(&[curves[0]], 0.01).iter().any(|v| v.bulge != 0.0), "curves become arcs");
    }

    #[test]
    fn a_circle_of_four_cubics_comes_back_as_two_half_circles() {
        let (r, k) = (10.0, 0.552_284_749_830_793_6 * 10.0);
        let (cx, cy) = (50.0, 50.0);
        let quarter = |a: Point, a2: Point, b2: Point, b: Point| [a, a2, b2, b].map(|p| [cx + p[0], cy + p[1]]);
        let cubics = [
            quarter([r, 0.0], [r, k], [k, r], [0.0, r]),
            quarter([0.0, r], [-k, r], [-r, k], [-r, 0.0]),
            quarter([-r, 0.0], [-r, -k], [-k, -r], [0.0, -r]),
            quarter([0.0, -r], [k, -r], [r, -k], [r, 0.0]),
        ];
        let v = fit_all(&cubics, 0.01);
        assert_eq!(v.len(), 3, "start, half way and back at the start: {v:?}");
        for w in v.windows(2) {
            let (c, radius, _, sweep) = arc_params([w[0].x, w[0].y], [w[1].x, w[1].y], w[0].bulge);
            assert!((radius - r).abs() < 0.01 && distance(c, [cx, cy]) < 0.01);
            assert!((sweep - PI).abs() < 1e-6, "clockwise on screen (y down)");
        }
    }
}
