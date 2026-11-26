//! Plain geometry: vertices with arcs, contours, transforms and bounding boxes.
//!
//! Units are millimetres, origin top-left, y pointing down (the same as SVG).
//! A contour is a polyline whose segments may be arcs: each vertex carries a `bulge`
//! (tan of a quarter of the arc's sweep; 0 = straight line to the next vertex), the DXF model.

use cavalier_contours::polyline::{PlineCreation, PlineSource, PlineSourceMut, Polyline};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

pub type Point = [f64; 2];
/// Bounding box: [x, y, width, height].
pub type BBox = [f64; 4];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    /// Arc to the next vertex: tan(sweep/4). 0 = straight line, 1 = half circle; positive = clockwise on screen (y points down).
    #[serde(default)]
    pub bulge: f64,
}

impl Vertex {
    pub fn new(x: f64, y: f64, bulge: f64) -> Self {
        Self { x, y, bulge }
    }
}

/// One closed or open outline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Contour {
    pub vertices: Vec<Vertex>,
    #[serde(default)]
    pub closed: bool,
}

impl Contour {
    pub fn rect(x: f64, y: f64, w: f64, h: f64, radius: f64) -> Self {
        let (x0, x1) = (x.min(x + w), x.max(x + w));
        let (y0, y1) = (y.min(y + h), y.max(y + h));
        let r = radius.max(0.0).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        let v = Vertex::new;
        let vertices = if r <= 0.0 {
            vec![v(x0, y0, 0.0), v(x1, y0, 0.0), v(x1, y1, 0.0), v(x0, y1, 0.0)]
        } else {
            // Quarter-circle corners: bulge = tan(90°/4).
            let b = (PI / 8.0).tan();
            vec![
                v(x0 + r, y0, 0.0),
                v(x1 - r, y0, b),
                v(x1, y0 + r, 0.0),
                v(x1, y1 - r, b),
                v(x1 - r, y1, 0.0),
                v(x0 + r, y1, b),
                v(x0, y1 - r, 0.0),
                v(x0, y0 + r, b),
            ]
        };
        Self { vertices, closed: true }
    }

    pub fn circle(cx: f64, cy: f64, r: f64) -> Self {
        Self { vertices: vec![Vertex::new(cx - r, cy, 1.0), Vertex::new(cx + r, cy, 1.0)], closed: true }
    }

    pub fn line(a: Point, b: Point) -> Self {
        Self { vertices: vec![Vertex::new(a[0], a[1], 0.0), Vertex::new(b[0], b[1], 0.0)], closed: false }
    }

    /// Arc around (cx, cy) from `start` to `end` degrees, clockwise on screen (increasing angle, y down).
    pub fn arc(cx: f64, cy: f64, r: f64, start: f64, end: f64) -> Self {
        let mut sweep = (end - start) % 360.0;
        if sweep <= 0.0 {
            sweep += 360.0;
        }
        if sweep >= 359.999 {
            return Self::circle(cx, cy, r);
        }
        let p = |deg: f64| {
            let (s, c) = deg.to_radians().sin_cos();
            [cx + r * c, cy + r * s]
        };
        let (a, b) = (p(start), p(start + sweep));
        Self {
            vertices: vec![Vertex::new(a[0], a[1], (sweep.to_radians() / 4.0).tan()), Vertex::new(b[0], b[1], 0.0)],
            closed: false,
        }
    }

    /// Regular polygon; the first corner points up before `rotation` (degrees).
    pub fn polygon(cx: f64, cy: f64, r: f64, sides: u32, rotation: f64) -> Self {
        let n = sides.max(3);
        let vertices = (0..n)
            .map(|i| {
                let a = (rotation - 90.0 + 360.0 * i as f64 / n as f64).to_radians();
                Vertex::new(cx + r * a.cos(), cy + r * a.sin(), 0.0)
            })
            .collect();
        Self { vertices, closed: true }
    }

    pub fn polyline(points: &[Point], closed: bool) -> Self {
        Self { vertices: points.iter().map(|p| Vertex::new(p[0], p[1], 0.0)).collect(), closed }
    }

    pub(crate) fn pline(&self) -> Polyline<f64> {
        let mut p = Polyline::with_capacity(self.vertices.len(), self.closed);
        for v in &self.vertices {
            p.add(v.x, v.y, v.bulge);
        }
        p
    }

    pub fn bbox(&self) -> BBox {
        self.pline()
            .extents()
            .map(|e| [e.min_x, e.min_y, e.max_x - e.min_x, e.max_y - e.min_y])
            .unwrap_or_else(|| bbox_of(&self.vertices.iter().map(|v| [v.x, v.y]).collect::<Vec<_>>()))
    }

    pub fn length(&self) -> f64 {
        self.pline().path_length()
    }

    /// Unsigned enclosed area; 0 for open contours.
    pub fn area(&self) -> f64 {
        if self.closed { self.pline().area().abs() } else { 0.0 }
    }

    /// The contour as points, arcs approximated to within `tol` mm.
    pub fn flatten(&self, tol: f64) -> Vec<Point> {
        let v = &self.vertices;
        let n = v.len();
        let mut out = Vec::new();
        let Some(first) = v.first() else { return out };
        out.push([first.x, first.y]);
        let segs = if self.closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            let (a, b) = (v[i], v[(i + 1) % n]);
            if a.bulge.abs() > 1e-9 {
                let (c, r, start, sweep) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
                // Segment count so the chord error stays under tol.
                let step = 2.0 * (1.0 - tol.min(r * 0.5) / r).acos();
                let steps = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(2, 720);
                for k in 1..steps {
                    let t = start + sweep * k as f64 / steps as f64;
                    out.push([c[0] + r * t.cos(), c[1] + r * t.sin()]);
                }
            }
            out.push([b.x, b.y]);
        }
        out
    }

    /// Point in the closed contour (arcs approximated).
    pub fn contains(&self, p: Point) -> bool {
        self.closed && point_in_poly(p, &self.flatten(0.05))
    }

    pub fn transformed(&self, t: &Transform, pivot: Point) -> Self {
        let (sx, sy) = t.scales();
        // Non-uniform scaling turns arcs into ellipses: approximate those arcs with lines first.
        let mut c = self.clone();
        if (sx.abs() - sy.abs()).abs() > 1e-9
            && c.vertices.iter().any(|v| v.bulge != 0.0)
            && let Some(flat) = c.pline().arcs_to_approx_lines(0.01)
        {
            c.vertices = flat.iter_vertexes().map(|v| Vertex::new(v.x, v.y, 0.0)).collect();
        }
        let flip = if (sx < 0.0) != (sy < 0.0) { -1.0 } else { 1.0 };
        for v in &mut c.vertices {
            let [x, y] = t.apply([v.x, v.y], pivot);
            *v = Vertex::new(x, y, v.bulge * flip);
        }
        c
    }
}

/// A move/rotate/scale/mirror around a pivot. Applied as: mirror, scale, rotate (around pivot), then move.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
pub struct Transform {
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
    /// Degrees, positive = clockwise on screen.
    #[serde(default)]
    pub rotate: f64,
    /// Scale factors; 0 or missing means 1.
    #[serde(default)]
    pub scale_x: f64,
    #[serde(default)]
    pub scale_y: f64,
    /// Mirror left-right / top-bottom.
    #[serde(default)]
    pub mirror_x: bool,
    #[serde(default)]
    pub mirror_y: bool,
    /// Centre of rotation/scale/mirror; defaults to the centre of the items' bounding box.
    pub pivot: Option<Point>,
}

impl Transform {
    pub fn translate(dx: f64, dy: f64) -> Self {
        Self { dx, dy, ..Self::default() }
    }

    /// Scale factors with mirroring folded in.
    pub fn scales(&self) -> (f64, f64) {
        let f = |v: f64| if v == 0.0 { 1.0 } else { v };
        let sx = f(self.scale_x) * if self.mirror_x { -1.0 } else { 1.0 };
        let sy = f(self.scale_y) * if self.mirror_y { -1.0 } else { 1.0 };
        (sx, sy)
    }

    pub fn apply(&self, p: Point, pivot: Point) -> Point {
        let (sx, sy) = self.scales();
        let q = [pivot[0] + (p[0] - pivot[0]) * sx, pivot[1] + (p[1] - pivot[1]) * sy];
        let q = rotate_point(q, pivot, self.rotate);
        [q[0] + self.dx, q[1] + self.dy]
    }
}

/// Centre, radius, start angle (rad) and sweep (rad) of the arc from `a` to `b` with `bulge`.
pub fn arc_params(a: Point, b: Point, bulge: f64) -> (Point, f64, f64, f64) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let f = (1.0 - bulge * bulge) / (4.0 * bulge);
    let c = [mid[0] - dy * f, mid[1] + dx * f];
    let r = (a[0] - c[0]).hypot(a[1] - c[1]);
    let start = (a[1] - c[1]).atan2(a[0] - c[0]);
    (c, r, start, 4.0 * bulge.atan())
}

pub fn rotate_point(p: Point, c: Point, deg: f64) -> Point {
    let (s, co) = deg.to_radians().sin_cos();
    let (x, y) = (p[0] - c[0], p[1] - c[1]);
    [c[0] + x * co - y * s, c[1] + x * s + y * co]
}

pub fn union(boxes: &[BBox]) -> Option<BBox> {
    let first = boxes.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first[0], first[1], first[0] + first[2], first[1] + first[3]);
    for b in boxes {
        x0 = x0.min(b[0]);
        y0 = y0.min(b[1]);
        x1 = x1.max(b[0] + b[2]);
        y1 = y1.max(b[1] + b[3]);
    }
    Some([x0, y0, x1 - x0, y1 - y0])
}

pub fn bbox_of(points: &[Point]) -> BBox {
    let boxes: Vec<BBox> = points.iter().map(|p| [p[0], p[1], 0.0, 0.0]).collect();
    union(&boxes).unwrap_or([0.0; 4])
}

pub fn distance(a: Point, b: Point) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Shortest distance from `p` to the segment `a`–`b`.
pub fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

pub fn point_in_poly(p: Point, poly: &[Point]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contours_measure_right() {
        let r = Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0);
        assert_eq!(r.bbox(), [10.0, 10.0, 40.0, 20.0]);
        assert!((r.length() - 120.0).abs() < 1e-9);
        assert!((r.area() - 800.0).abs() < 1e-9);

        let c = Contour::circle(0.0, 0.0, 10.0);
        assert!((c.length() - 2.0 * PI * 10.0).abs() < 1e-6);
        assert!((c.area() - PI * 100.0).abs() < 1e-6);
        assert!((c.bbox()[2] - 20.0).abs() < 1e-9);

        let rr = Contour::rect(0.0, 0.0, 40.0, 20.0, 5.0);
        assert!((rr.area() - (800.0 - (4.0 - PI) * 25.0)).abs() < 1e-6);
        assert!((rr.bbox()[2] - 40.0).abs() < 1e-9 && (rr.bbox()[3] - 20.0).abs() < 1e-9);

        // Quarter arc, clockwise on screen from 0° to 90°: ends straight below the centre.
        let a = Contour::arc(0.0, 0.0, 10.0, 0.0, 90.0);
        assert!(a.vertices[1].x.abs() < 1e-9 && (a.vertices[1].y - 10.0).abs() < 1e-9);
        assert!(a.flatten(0.01).iter().all(|p| (p[0].hypot(p[1]) - 10.0).abs() < 1e-6 && p[0] >= -1e-9 && p[1] >= -1e-9));
    }

    #[test]
    fn transforms() {
        let r = Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0);
        let t = Transform { dx: 5.0, rotate: 90.0, ..Default::default() };
        let b = r.transformed(&t, [5.0, 5.0]).bbox();
        assert!((b[0] - 5.0).abs() < 1e-9 && (b[2] - 10.0).abs() < 1e-9);
        // Mirroring reverses arc direction.
        let c = Contour::arc(0.0, 0.0, 10.0, 0.0, 90.0).transformed(&Transform { mirror_x: true, ..Default::default() }, [0.0, 0.0]);
        assert!(c.vertices[0].bulge < 0.0);
        assert!(c.flatten(0.01).iter().all(|p| p[0] <= 1e-9));
    }
}
