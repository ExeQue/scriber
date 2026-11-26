//! Constraints inferred while drawing (AutoCAD's INFER): points of a new item that were snapped
//! onto existing geometry stay bound to it, and level or plumb sides stay that way.

use super::geo::path_contours;
use super::pick::{nearest_point, nearest_segment};
use super::{Kind, Ref};
use crate::model::geom::{arc_params, distance};
use crate::model::{Point, State};

/// A placed point of a new item and what it was snapped to. `kind` is Coincident (an end, a centre
/// or a point on the object), Perpendicular or Tangent. A new circle's edge point on a line or
/// curve makes the circle tangent to it.
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
            // The edge point of a new circle or arc (no vertex there) on a line or curve: touching it,
            // on the side the new centre is on.
            (Kind::Coincident | Kind::Perpendicular | Kind::Tangent, None) if matches!(l.target, Ref::Segment { .. }) => {
                if let Some((seg, _)) = nearest_segment(s, item, l.p).filter(|(_, d)| *d < 0.05)
                    && segment_at(s, seg).is_some_and(|(.., bulge)| bulge != 0.0)
                {
                    out.push((Kind::Tangent, vec![l.target, seg]));
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

/// A placed point of a new item that was locked to a tracking path, and what the new segment
/// through it gets when it runs along that path: Perpendicular, Parallel or Collinear to the line
/// `target`, Perpendicular to an arc (along its radius), Concentric with an arc (on its circle), or
/// Horizontal / Vertical (no target).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Along {
    pub p: Point,
    pub kind: Kind,
    pub target: Option<Ref>,
}

/// How far (sine of the angle) a new segment may turn from a path and still run along it.
const ALONG: f64 = 0.01;

/// Segment `r`'s ends and bulge.
fn segment_at(s: &State, r: Ref) -> Option<(Point, Point, f64)> {
    let Ref::Segment { item, contour, segment } = r else { return None };
    let c = path_contours(s, item).ok()?.get(contour)?;
    let (a, b) = (c.vertices.get(segment)?, c.vertices[(segment + 1) % c.vertices.len()]);
    Some(([a.x, a.y], [b.x, b.y], a.bulge))
}

fn unit(a: Point, b: Point) -> Point {
    let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-12);
    [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
}

/// The constraints for new item `item` whose segments run along the tracking paths in `links`.
pub fn along_constraints(s: &State, item: u64, links: &[Along]) -> Vec<(Kind, Vec<Ref>)> {
    let mut out = Vec::new();
    for l in links.iter().filter(|l| l.target.is_none_or(|t| t.item() != item)) {
        let Some((Ref::Point { contour, vertex, .. }, d)) = nearest_point(s, item, l.p) else { continue };
        let Some(c) = path_contours(s, item).ok().and_then(|c| c.get(contour)).filter(|_| d < 1e-6) else { continue };
        let n = c.vertices.len();
        let segments = if c.closed { n } else { n.saturating_sub(1) };
        // The segments ending and starting at the point.
        let around = [if vertex > 0 { Some(vertex - 1) } else { c.closed.then_some(n - 1) }, (vertex < segments).then_some(vertex)];
        for k in around.into_iter().flatten() {
            out.extend(along(s, l, Ref::Segment { item, contour, segment: k }));
        }
    }
    out
}

/// What new segment `new` gets from `l` when it runs along the path.
fn along(s: &State, l: &Along, new: Ref) -> Vec<(Kind, Vec<Ref>)> {
    let Some((a, b, bulge)) = segment_at(s, new) else { return Vec::new() };
    let u = unit(a, b);
    let cross = |v: Point| (u[0] * v[1] - u[1] * v[0]).abs();
    let Some(t) = l.target else {
        let level = bulge == 0.0 && match l.kind {
            Kind::Horizontal => u[1].abs() < ALONG,
            Kind::Vertical => u[0].abs() < ALONG,
            _ => false,
        };
        return if level { vec![(l.kind, vec![new])] } else { Vec::new() };
    };
    let Some((ta, tb, tbulge)) = segment_at(s, t) else { return Vec::new() };
    let pair = |kind: Kind| (kind, vec![t, new]);
    match (l.kind, bulge == 0.0, tbulge == 0.0) {
        (Kind::Parallel | Kind::Collinear, true, true) if cross(unit(ta, tb)) < ALONG => vec![pair(l.kind)],
        (Kind::Perpendicular, true, true) if cross(unit(ta, tb)) > 1.0 - ALONG => vec![pair(Kind::Perpendicular)],
        // Square to an arc is along its radius: the arc's centre stays on the new line.
        (Kind::Perpendicular, true, false) if cross(unit(arc_params(ta, tb, tbulge).0, l.p)) < ALONG => {
            let Ref::Segment { item, contour, segment } = t else { return Vec::new() };
            vec![(Kind::Coincident, vec![Ref::Centre { item, contour, segment }, new])]
        }
        (Kind::Concentric, false, false) => {
            let ((c1, r1, ..), (c2, r2, ..)) = (arc_params(ta, tb, tbulge), arc_params(a, b, bulge));
            if distance(c1, c2) < 1e-6 && (r1 - r2).abs() < 1e-6 { vec![pair(Kind::Concentric), pair(Kind::Equal)] } else { Vec::new() }
        }
        _ => Vec::new(),
    }
}
