//! xTool display geometry → Scriber contours. Every outline is built as line and Bézier segments in
//! the element's own (local) coordinates, then moved by its `transform` and fitted with arcs, so
//! rotation and non-uniform scale come out right for every type.

use crate::export::svg_import::bezier_contour;
use crate::model::Contour;
use crate::model::geom::Point;
use serde_json::Value;
use svgtypes::{SimplePathSegment, SimplifyingPathParser};

/// Largest distance in mm between an imported curve and the arcs that replace it.
pub const TOLERANCE: f64 = 0.002;

/// Cubic Bézier handle length for a quarter circle of radius 1.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// xTool's `transform` [a, b, c, d, e, f]: world = (a x + c y + e, b x + d y + f).
pub type Affine = [f64; 6];

pub fn apply(t: &Affine, p: Point) -> Point {
    [t[0] * p[0] + t[2] * p[1] + t[4], t[1] * p[0] + t[3] * p[1] + t[5]]
}

/// The display's transform (missing: a move to offsetX/offsetY).
pub fn transform(d: &Value) -> Affine {
    let n = |v: &Value| v.as_f64();
    match d["transform"].as_array().map(|a| a.iter().filter_map(n).collect::<Vec<_>>()) {
        Some(v) if v.len() == 6 => [v[0], v[1], v[2], v[3], v[4], v[5]],
        _ => [1.0, 0.0, 0.0, 1.0, n(&d["offsetX"]).unwrap_or(0.0), n(&d["offsetY"]).unwrap_or(0.0)],
    }
}

/// How much the transform scales x and y (local units per world mm, inverted).
pub fn scales(t: &Affine) -> (f64, f64) {
    (t[0].hypot(t[1]), t[2].hypot(t[3]))
}

/// One subpath: start, then segments (control points, end): none = line, one = quadratic, two = cubic.
pub struct Sub {
    pub start: Point,
    pub segs: Vec<(Vec<Point>, Point)>,
    pub closed: bool,
}

impl Sub {
    fn new(start: Point) -> Self {
        Self { start, segs: Vec::new(), closed: false }
    }

    fn line(&mut self, to: Point) {
        self.segs.push((Vec::new(), to));
    }

    fn cubic(&mut self, c1: Point, c2: Point, to: Point) {
        self.segs.push((vec![c1, c2], to));
    }

    /// Straight lines through `points`, closed.
    fn polygon(points: &[Point]) -> Self {
        let mut s = Sub::new(points[0]);
        points[1..].iter().for_each(|p| s.line(*p));
        s.closed = true;
        s
    }
}

/// The subpaths moved by `t` and fitted as arc contours.
pub fn place(subs: &[Sub], t: &Affine) -> Vec<Contour> {
    subs.iter()
        .filter_map(|s| {
            let segs: Vec<(Vec<Point>, Point)> = s.segs.iter().map(|(c, p)| (c.iter().map(|q| apply(t, *q)).collect(), apply(t, *p))).collect();
            bezier_contour(apply(t, s.start), &segs, s.closed, TOLERANCE)
        })
        .collect()
}

/// SVG path data (absolute or relative, arcs become cubics) as subpaths.
pub fn parse_path(d: &str) -> Result<Vec<Sub>, String> {
    let mut out: Vec<Sub> = Vec::new();
    let mut start = [0.0, 0.0];
    for seg in SimplifyingPathParser::from(d) {
        let seg = seg.map_err(|e| format!("bad path data: {e}"))?;
        if let SimplePathSegment::MoveTo { x, y } = seg {
            start = [x, y];
            out.push(Sub::new(start));
            continue;
        }
        // A drawing command after Z (or first) continues from the last subpath's start.
        if out.last().is_none_or(|s| s.closed) {
            out.push(Sub::new(start));
        }
        let sub = out.last_mut().unwrap();
        match seg {
            SimplePathSegment::LineTo { x, y } => sub.line([x, y]),
            SimplePathSegment::CurveTo { x1, y1, x2, y2, x, y } => sub.cubic([x1, y1], [x2, y2], [x, y]),
            SimplePathSegment::Quadratic { x1, y1, x, y } => sub.segs.push((vec![[x1, y1]], [x, y])),
            SimplePathSegment::ClosePath => sub.closed = true,
            SimplePathSegment::MoveTo { .. } => unreachable!(),
        }
    }
    Ok(out)
}

/// A `w` × `h` rectangle at the origin with corners rounded by `r`.
pub fn rect(w: f64, h: f64, r: f64) -> Sub {
    let r = r.clamp(0.0, w.min(h) / 2.0);
    if r <= 1e-9 {
        return Sub::polygon(&[[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]);
    }
    let k = r * (1.0 - KAPPA);
    let mut s = Sub::new([r, 0.0]);
    s.line([w - r, 0.0]);
    s.cubic([w - k, 0.0], [w, k], [w, r]);
    s.line([w, h - r]);
    s.cubic([w, h - k], [w - k, h], [w - r, h]);
    s.line([r, h]);
    s.cubic([k, h], [0.0, h - k], [0.0, h - r]);
    s.line([0.0, r]);
    s.cubic([0.0, k], [k, 0.0], [r, 0.0]);
    s.closed = true;
    s
}

/// The ellipse inscribed in the `w` × `h` box at the origin.
pub fn ellipse(w: f64, h: f64) -> Sub {
    let (rx, ry, cx, cy) = (w / 2.0, h / 2.0, w / 2.0, h / 2.0);
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let mut s = Sub::new([cx + rx, cy]);
    s.cubic([cx + rx, cy + ky], [cx + kx, cy + ry], [cx, cy + ry]);
    s.cubic([cx - kx, cy + ry], [cx - rx, cy + ky], [cx - rx, cy]);
    s.cubic([cx - rx, cy - ky], [cx - kx, cy - ry], [cx, cy - ry]);
    s.cubic([cx + kx, cy - ry], [cx + rx, cy - ky], [cx + rx, cy]);
    s.closed = true;
    s
}

/// A regular polygon (`inner` None) or star (`inner` = inner / outer radius) with `count` points,
/// inscribed in the ellipse of the nominal `ow` × `oh` box at the origin (verified on xTool's
/// triangle and 5-point star: apex up, the tight box centred and touching the top). For counts where
/// apex up does not give the stored tight size `tight` (w, h), the shape turned half a step is used.
pub fn polygon(count: usize, inner: Option<f64>, ow: f64, oh: f64, tight: [f64; 2]) -> Sub {
    let points = |phase: f64| -> Vec<Point> {
        let n = if inner.is_some() { 2 * count } else { count };
        (0..n)
            .map(|k| {
                let a = phase + std::f64::consts::TAU * k as f64 / n as f64;
                let r = if k % 2 == 1 { inner.unwrap_or(1.0) } else { 1.0 };
                [ow / 2.0 + ow / 2.0 * r * a.sin(), oh / 2.0 - oh / 2.0 * r * a.cos()]
            })
            .collect()
    };
    let miss = |p: &[Point]| {
        let b = crate::model::geom::bbox_of(p);
        (b[2] - tight[0]).abs() + (b[3] - tight[1]).abs()
    };
    let (a, b) = (points(0.0), points(std::f64::consts::PI / count as f64));
    Sub::polygon(if miss(&b) + 1e-6 < miss(&a) { &b } else { &a })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_data_with_relative_moves_and_arcs() {
        let subs = parse_path("M0 0 l10 0 v10 h-10 z m20 0 A5 5 0 0 1 30 0 Q35 5 40 0").unwrap();
        assert_eq!(subs.len(), 2);
        assert!(subs[0].closed && !subs[1].closed);
        assert_eq!(subs[0].segs.last().unwrap().1, [0.0, 10.0]);
        assert_eq!(subs[1].start, [20.0, 0.0]);
        let c = place(&subs, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let b = c[1].bbox();
        // The half circle from 20 to 30 bulges up (sweep 1, y down) to y = -5.
        assert!((b[1] + 5.0).abs() < 0.01, "{b:?}");
        assert!(parse_path("M0 0 L").is_err());
    }

    #[test]
    fn polygons_match_xtools_tight_boxes() {
        // From real files: (count, inner, oWidth, oHeight, width, height).
        for (n, inner, ow, oh, w, h) in [(3, None, 65.588406, 75.462329, 56.801226, 56.596747), (5, Some(0.382), 70.723258, 75.357199, 67.261816, 68.161227)] {
            let s = polygon(n, inner, ow, oh, [w, h]);
            let b = place(&[s], &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0])[0].bbox();
            assert!((b[2] - w).abs() < 1e-3 && (b[3] - h).abs() < 1e-3 && b[1].abs() < 1e-9, "{b:?}");
            assert!((b[0] - (ow - w) / 2.0).abs() < 1e-3, "centred: {b:?}");
        }
        // A square drawn flat: the half-step turn fits.
        let b = place(&[polygon(4, None, 10.0, 10.0, [7.0710678, 7.0710678])], &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0])[0].bbox();
        assert!((b[2] - 7.0710678).abs() < 1e-6, "{b:?}");
    }
}
