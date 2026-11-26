//! Constraints inferred while drawing (AutoCAD's INFER): points of a new item that were snapped
//! onto existing geometry stay bound to it, and level or plumb sides stay that way.

use super::geo::path_contours;
use super::pick::{nearest_point, nearest_segment};
use super::{Kind, Ref};
use crate::model::geom::arc_params;
use crate::model::{Point, State};

/// A placed point of a new item and what it was snapped to. `kind` is Coincident (an end, a centre
/// or a point on the object), Perpendicular or Tangent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Link {
    pub p: Point,
    pub kind: Kind,
    pub target: Ref,
}

/// The constraint binding `new` (a point or centre of the new item) to `target`: the target comes
/// first, so it stays put. Two centres become concentric.
pub fn bind(new: Ref, target: Ref) -> (Kind, Vec<Ref>) {
    match (new, target) {
        (Ref::Centre { item, contour, segment }, Ref::Centre { item: ti, contour: tc, segment: ts }) => {
            (Kind::Concentric, vec![Ref::Segment { item: ti, contour: tc, segment: ts }, Ref::Segment { item, contour, segment }])
        }
        _ => (Kind::Coincident, vec![target, new]),
    }
}

/// The constraints for new item `item` from the snaps its points were placed with.
pub fn link_constraints(s: &State, item: u64, links: &[Link]) -> Vec<(Kind, Vec<Ref>)> {
    let mut out = Vec::new();
    for l in links.iter().filter(|l| l.target.item() != item) {
        let at = nearest_point(s, item, l.p).filter(|(_, d)| *d < 1e-6).map(|(r, _)| r);
        match (l.kind, at) {
            (Kind::Coincident, Some(new)) => out.push(bind(new, l.target)),
            // A point on the new curve (a circle through a snapped end): flattened, so a looser tolerance.
            (Kind::Coincident, None) if matches!(l.target, Ref::Point { .. } | Ref::Centre { .. }) => {
                if let Some((seg, _)) = nearest_segment(s, item, l.p).filter(|(_, d)| *d < 0.05) {
                    out.push((Kind::Coincident, vec![l.target, seg]));
                }
            }
            // Perpendicular to or tangent on the target: the new segment that ends there.
            (Kind::Perpendicular | Kind::Tangent, Some(Ref::Point { item, contour, vertex })) => {
                out.push((Kind::Coincident, vec![l.target, Ref::Point { item, contour, vertex }]));
                out.push((l.kind, vec![l.target, Ref::Segment { item, contour, segment: vertex.saturating_sub(1) }]));
            }
            _ => {}
        }
    }
    out
}

/// Every vertex and arc centre of `item`, with its reference.
pub(super) fn points(s: &State, item: u64) -> Vec<(Ref, Point)> {
    let Ok(contours) = path_contours(s, item) else { return Vec::new() };
    let mut out = Vec::new();
    for (ci, c) in contours.iter().enumerate() {
        let v = |k: usize| [c.vertices[k % c.vertices.len()].x, c.vertices[k % c.vertices.len()].y];
        if let Some((centre, _)) = c.as_circle() {
            out.push((Ref::Centre { item, contour: ci, segment: 0 }, centre));
            continue;
        }
        let segments = if c.closed { c.vertices.len() } else { c.vertices.len().saturating_sub(1) };
        for k in 0..c.vertices.len() {
            out.push((Ref::Point { item, contour: ci, vertex: k }, v(k)));
        }
        for k in (0..segments).filter(|k| c.vertices[*k].bulge != 0.0) {
            out.push((Ref::Centre { item, contour: ci, segment: k }, arc_params(v(k), v(k + 1), c.vertices[k].bulge).0));
        }
    }
    out
}

/// For items placed by exact coordinates (agents): every vertex or centre of `item` that lies on a
/// vertex or centre of another item, as a link.
pub fn touching(s: &State, item: u64) -> Vec<Link> {
    let mut out = Vec::new();
    for (_, p) in points(s, item) {
        for other in s.items.iter().filter(|i| i.id != item) {
            if let Some((target, d)) = nearest_point(s, other.id, p)
                && d < 1e-6
            {
                out.push(Link { p, kind: Kind::Coincident, target });
            }
        }
    }
    out
}

/// Horizontal or vertical for every exactly level or plumb straight segment of `item` (RECTANG, ORTHO).
pub fn axis_constraints(s: &State, item: u64) -> Vec<(Kind, Vec<Ref>)> {
    let Ok(contours) = path_contours(s, item) else { return Vec::new() };
    let mut out = Vec::new();
    for (ci, c) in contours.iter().enumerate().filter(|(_, c)| c.as_circle().is_none()) {
        let n = c.vertices.len();
        let segments = if c.closed { n } else { n.saturating_sub(1) };
        for k in (0..segments).filter(|k| c.vertices[*k].bulge == 0.0) {
            let (a, b) = (c.vertices[k], c.vertices[(k + 1) % n]);
            let r = vec![Ref::Segment { item, contour: ci, segment: k }];
            if (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() > 1e-9 {
                out.push((Kind::Vertical, r));
            } else if (a.y - b.y).abs() < 1e-9 && (a.x - b.x).abs() > 1e-9 {
                out.push((Kind::Horizontal, r));
            }
        }
    }
    out
}
