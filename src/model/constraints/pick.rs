//! Finding references on the drawing: the vertex or segment nearest a click, and where a
//! reference is (for constraint bars, highlights and dimensions).

use super::Ref;
use super::geo::path_contours;
use crate::model::geom::{arc_params, distance, segment_distance};
use crate::model::{Contour, Point, State};

fn vertex(c: &Contour, k: usize) -> Point {
    let v = c.vertices[k % c.vertices.len()];
    [v.x, v.y]
}

fn segments(c: &Contour) -> usize {
    if c.as_circle().is_some() {
        1
    } else if c.closed {
        c.vertices.len()
    } else {
        c.vertices.len().saturating_sub(1)
    }
}

/// The vertex or arc centre of `item` nearest `p`, with its distance.
pub fn nearest_point(s: &State, item: u64, p: Point) -> Option<(Ref, f64)> {
    let contours = path_contours(s, item).ok()?;
    let mut best: Option<(Ref, f64)> = None;
    let mut consider = |r: Ref, q: Point| {
        let d = distance(p, q);
        if best.is_none_or(|b| d < b.1) {
            best = Some((r, d));
        }
    };
    for (ci, c) in contours.iter().enumerate() {
        if c.as_circle().is_none() {
            for k in 0..c.vertices.len() {
                consider(Ref::Point { item, contour: ci, vertex: k }, vertex(c, k));
            }
        }
        for k in 0..segments(c) {
            if c.vertices[k].bulge != 0.0 {
                let (centre, ..) = arc_params(vertex(c, k), vertex(c, k + 1), c.vertices[k].bulge);
                consider(Ref::Centre { item, contour: ci, segment: k }, centre);
            }
        }
    }
    best
}

/// The line or arc of `item` nearest `p`, with its distance.
pub fn nearest_segment(s: &State, item: u64, p: Point) -> Option<(Ref, f64)> {
    let contours = path_contours(s, item).ok()?;
    let mut best: Option<(Ref, f64)> = None;
    for (ci, c) in contours.iter().enumerate() {
        for k in 0..segments(c) {
            let d = segment(c, k).windows(2).map(|w| segment_distance(p, w[0], w[1])).fold(f64::MAX, f64::min);
            if best.is_none_or(|b| d < b.1) {
                best = Some((Ref::Segment { item, contour: ci, segment: k }, d));
            }
        }
    }
    best
}

/// Segment `k` as points (arcs flattened; a circle's segment 0 is the whole circle).
fn segment(c: &Contour, k: usize) -> Vec<Point> {
    if c.as_circle().is_some() {
        return c.flatten(0.05);
    }
    let mut one = Contour { vertices: vec![c.vertices[k], c.vertices[(k + 1) % c.vertices.len()]], closed: false };
    one.vertices[1].bulge = 0.0;
    one.flatten(0.05)
}

/// The geometry a reference stands for, as a polyline (a point is one point).
pub fn outline(s: &State, r: &Ref) -> Vec<Point> {
    let Ok(contours) = path_contours(s, r.item()) else { return Vec::new() };
    let get = |ci: usize| contours.get(ci);
    match *r {
        Ref::Point { contour, vertex: k, .. } => get(contour).filter(|c| k < c.vertices.len()).map(|c| vec![vertex(c, k)]).unwrap_or_default(),
        Ref::Segment { contour, segment: k, .. } => get(contour).filter(|c| k < c.vertices.len()).map(|c| segment(c, k)).unwrap_or_default(),
        Ref::Centre { contour, segment: k, .. } => get(contour)
            .filter(|c| k < c.vertices.len())
            .map(|c| vec![arc_params(vertex(c, k), vertex(c, k + 1), c.vertices[k].bulge).0])
            .unwrap_or_default(),
        Ref::Item { .. } => contours.iter().flat_map(|c| c.flatten(0.05)).collect(),
    }
}

/// Where a reference sits: the point itself, or the middle of a segment.
pub fn anchor(s: &State, r: &Ref) -> Option<Point> {
    let pts = outline(s, r);
    match (r, pts.as_slice()) {
        (Ref::Item { .. }, _) => pts.first().copied(),
        (_, [a, b]) => Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]),
        _ => pts.get(pts.len() / 2).copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, Op, Shape};

    #[test]
    fn picks_nearest() {
        let mut d = Document::new(100.0, 100.0);
        let r = d.add("r", Op::Cut, false, Shape::path(Contour::rect(0.0, 0.0, 40.0, 20.0, 0.0)));
        let c = d.add("c", Op::Cut, false, Shape::path(Contour::circle(70.0, 50.0, 10.0)));
        assert_eq!(nearest_point(&d.s, r, [39.0, 1.0]).unwrap().0, Ref::Point { item: r, contour: 0, vertex: 1 });
        assert_eq!(nearest_segment(&d.s, r, [41.0, 10.0]).unwrap().0, Ref::Segment { item: r, contour: 0, segment: 1 });
        assert_eq!(nearest_point(&d.s, c, [71.0, 50.0]).unwrap().0, Ref::Centre { item: c, contour: 0, segment: 0 });
        assert_eq!(nearest_segment(&d.s, c, [70.0, 61.0]).unwrap().0, Ref::Segment { item: c, contour: 0, segment: 0 });
        assert_eq!(anchor(&d.s, &Ref::Segment { item: r, contour: 0, segment: 0 }), Some([20.0, 0.0]));
    }
}
