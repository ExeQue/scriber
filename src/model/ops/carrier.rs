//! The infinite line or full circle a segment lies on: what a two-object fillet or chamfer extends
//! objects along, offsets to find fillet centres, and intersects to find corners.

use super::EPS;
use crate::model::geom::{Point, Vertex, arc_params, distance};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Carrier {
    /// Through `p` with unit direction `u`.
    Line { p: Point, u: Point },
    Circle { c: Point, r: f64 },
}

pub(super) fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

pub(super) fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

pub(super) fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Unit vector along `v` turned a quarter towards increasing angle (the way a positive bulge runs).
pub(super) fn turned(v: Point) -> Point {
    let l = v[0].hypot(v[1]);
    [-v[1] / l, v[0] / l]
}

impl Carrier {
    /// The carrier of the segment from `a` to `b` (None when it has no length).
    pub fn of_segment(a: Vertex, b: Vertex) -> Option<Carrier> {
        let (pa, pb) = ([a.x, a.y], [b.x, b.y]);
        let len = distance(pa, pb);
        if len < EPS {
            return None;
        }
        Some(if a.bulge == 0.0 {
            Carrier::Line { p: pa, u: [(pb[0] - pa[0]) / len, (pb[1] - pa[1]) / len] }
        } else {
            let (c, r, _, _) = arc_params(pa, pb, a.bulge);
            Carrier::Circle { c, r }
        })
    }

    /// The carriers `d` mm away on either side (lines) or outside and inside (circles); itself for d = 0.
    pub fn offsets(self, d: f64) -> Vec<Carrier> {
        if d == 0.0 {
            return vec![self];
        }
        match self {
            Carrier::Line { p, u } => [-d, d].map(|s| Carrier::Line { p: [p[0] - s * u[1], p[1] + s * u[0]], u }).to_vec(),
            Carrier::Circle { c, r } => [r + d, (r - d).abs()].into_iter().filter(|r| *r > EPS).map(|r| Carrier::Circle { c, r }).collect(),
        }
    }

    /// The point of the carrier nearest `q` (None for a circle's own centre).
    pub fn foot(self, q: Point) -> Option<Point> {
        match self {
            Carrier::Line { p, u } => {
                let t = dot(sub(q, p), u);
                Some([p[0] + t * u[0], p[1] + t * u[1]])
            }
            Carrier::Circle { c, r } => {
                let l = distance(q, c);
                (l > EPS).then(|| [c[0] + (q[0] - c[0]) * r / l, c[1] + (q[1] - c[1]) * r / l])
            }
        }
    }

    /// Which side of the carrier `q` is on: ±1 (a circle: +1 outside, -1 inside), 0 on it.
    pub fn side(self, q: Point) -> f64 {
        let s = match self {
            Carrier::Line { p, u } => cross(u, sub(q, p)),
            Carrier::Circle { c, r } => distance(q, c) - r,
        };
        if s.abs() < 1e-9 { 0.0 } else { s.signum() }
    }

    /// Where two carriers cross (one point where they touch).
    pub fn intersect(self, other: Carrier) -> Vec<Point> {
        match (self, other) {
            (Carrier::Line { p: p1, u: u1 }, Carrier::Line { p: p2, u: u2 }) => {
                let k = cross(u1, u2);
                if k.abs() < 1e-9 {
                    return vec![];
                }
                let t = cross(sub(p2, p1), u2) / k;
                vec![[p1[0] + t * u1[0], p1[1] + t * u1[1]]]
            }
            (Carrier::Line { p, u }, Carrier::Circle { c, r }) | (Carrier::Circle { c, r }, Carrier::Line { p, u }) => {
                let f = Carrier::Line { p, u }.foot(c).unwrap();
                let h2 = r * r - distance(f, c).powi(2);
                split(f, u, h2, r)
            }
            (Carrier::Circle { c: c1, r: r1 }, Carrier::Circle { c: c2, r: r2 }) => {
                let d = distance(c1, c2);
                if d < EPS {
                    return vec![];
                }
                let e = [(c2[0] - c1[0]) / d, (c2[1] - c1[1]) / d];
                let a = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                split([c1[0] + a * e[0], c1[1] + a * e[1]], [-e[1], e[0]], r1 * r1 - a * a, r1.max(r2))
            }
        }
    }
}

/// The points `±sqrt(h2)` from `m` along `u`; one when they (nearly) touch, none when `h2` < 0.
fn split(m: Point, u: Point, h2: f64, scale: f64) -> Vec<Point> {
    if h2 < -1e-9 * scale.max(1.0).powi(2) {
        return vec![];
    }
    let h = h2.max(0.0).sqrt();
    if h < 1e-9 {
        return vec![m];
    }
    vec![[m[0] - h * u[0], m[1] - h * u[1]], [m[0] + h * u[0], m[1] + h * u[1]]]
}
