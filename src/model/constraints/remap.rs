//! Constraints through TRIM. The trimmed item's contours are rebuilt, so vertex and segment numbers
//! change; instead of dropping its constraints (what `settle` does on a topology change), every
//! reference is found again by its geometry: a vertex at the same place, a segment on the same
//! line or circle. A trimmed circle so keeps its tangents, radius and centre on the arc that is left.
//! Then the new ends are bound to the edges they were trimmed against (INFER).

use super::geo::path_contours;
use super::{Kind, Ref};
use crate::model::geom::{Contour, Point, Vertex, arc_params, distance};
use crate::model::ops::carrier::Carrier;
use crate::model::{Document, Shape};
use cavalier_contours::core::math::Vector2;
use cavalier_contours::polyline::PlineSource;

const TOL: f64 = 1e-6;

fn segments(c: &Contour) -> usize {
    if c.closed { c.vertices.len() } else { c.vertices.len().saturating_sub(1) }
}

/// Segment `k` of `c`: its start and end (the end's bulge cleared).
fn seg(c: &Contour, k: usize) -> (Vertex, Vertex) {
    let b = c.vertices[(k + 1) % c.vertices.len()];
    (c.vertices[k], Vertex::new(b.x, b.y, 0.0))
}

fn same_carrier(a: Carrier, b: Carrier) -> bool {
    match (a, b) {
        (Carrier::Line { p, u }, Carrier::Line { p: q, u: v }) => (u[0] * v[1] - u[1] * v[0]).abs() < 1e-9 && (u[0] * (q[1] - p[1]) - u[1] * (q[0] - p[0])).abs() < TOL,
        (Carrier::Circle { c, r }, Carrier::Circle { c: d, r: s }) => distance(c, d) < TOL && (r - s).abs() < TOL,
        _ => false,
    }
}

/// The middle of a segment and its length.
fn middle(a: Vertex, b: Vertex) -> (Point, f64) {
    let (pa, pb) = ([a.x, a.y], [b.x, b.y]);
    if a.bulge == 0.0 {
        return ([(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0], distance(pa, pb));
    }
    let (c, r, start, sweep) = arc_params(pa, pb, a.bulge);
    let m = start + sweep / 2.0;
    ([c[0] + r * m.cos(), c[1] + r * m.sin()], r * sweep.abs())
}

/// Where segment `k` of `old` is now among `new`: the pieces of it, longest first, and whether
/// the longest changed.
fn find_segment(old: &Contour, k: usize, new: &[Contour]) -> Option<(Vec<(usize, usize)>, bool)> {
    let (a, b) = seg(old, k);
    let carrier = Carrier::of_segment(a, b)?;
    let whole_circle = old.as_circle().is_some();
    let piece = Contour { vertices: vec![a, b], closed: false }.pline();
    let mut found: Vec<((usize, usize), f64)> = Vec::new();
    for (ci, c) in new.iter().enumerate() {
        for j in 0..segments(c) {
            let (na, nb) = seg(c, j);
            let (m, len) = middle(na, nb);
            let inside = whole_circle || piece.closest_point(Vector2::new(m[0], m[1]), 1e-9).is_some_and(|r| r.distance < TOL);
            if inside && Carrier::of_segment(na, nb).is_some_and(|n| same_carrier(carrier, n)) {
                found.push(((ci, j), len));
            }
        }
    }
    // Stable: equally long pieces keep their order.
    found.sort_by(|x, y| y.1.total_cmp(&x.1));
    let (ci, j) = found.first()?.0;
    let (na, nb) = seg(&new[ci], j);
    let same = |p: Vertex, q: Vertex| distance([p.x, p.y], [q.x, q.y]) < TOL;
    let changed = whole_circle || !(same(a, na) && same(b, nb) && (a.bulge - na.bulge).abs() < 1e-9);
    Some((found.into_iter().map(|f| f.0).collect(), changed))
}

/// `r` (on the rebuilt item) in the new contours (a segment: its longest piece), and whether its
/// segment changed.
fn find(old: &[Contour], new: &[Contour], r: Ref) -> Option<(Ref, bool)> {
    match r {
        Ref::Point { item, contour, vertex } => {
            let v = old.get(contour)?.vertices.get(vertex)?;
            let at = |ci: usize, k: usize| new.get(ci).and_then(|c| c.vertices.get(k)).is_some_and(|w| distance([v.x, v.y], [w.x, w.y]) < TOL);
            let (contour, vertex) = if at(contour, vertex) {
                (contour, vertex)
            } else {
                new.iter().enumerate().flat_map(|(ci, c)| (0..c.vertices.len()).map(move |k| (ci, k))).find(|(ci, k)| at(*ci, *k))?
            };
            Some((Ref::Point { item, contour, vertex }, false))
        }
        Ref::Segment { item, contour, segment } => find_segment(old.get(contour)?, segment, new).map(|(p, ch)| (Ref::Segment { item, contour: p[0].0, segment: p[0].1 }, ch)),
        Ref::Centre { item, contour, segment } => find_segment(old.get(contour)?, segment, new).map(|(p, ch)| (Ref::Centre { item, contour: p[0].0, segment: p[0].1 }, ch)),
        Ref::Item { .. } => Some((r, false)),
    }
}

/// Whether a constraint of `kind` with `refs` uses where a line or arc ends (so a shorter piece
/// does not satisfy it): smooth joins, line lengths (equal, symmetric, a line's own dimension).
fn uses_ends(kind: Kind, refs: &[Ref], line: bool) -> bool {
    match kind {
        Kind::Smooth => true,
        Kind::Equal | Kind::Symmetric => line,
        Kind::DimAligned | Kind::DimHorizontal | Kind::DimVertical => refs.len() == 1,
        _ => false,
    }
}

impl Document {
    /// Item `id`'s contours were rebuilt from `old` (TRIM): point its constraints at the same
    /// geometry in the new contours, drop those whose geometry is gone or got shorter where it
    /// matters, and tell `settle` this is the solved state, so it does not prune them. Pieces of one
    /// segment are tied to one line (collinear) or circle (concentric, equal) when the item keeps
    /// constraints, or with `tie`.
    pub(crate) fn remap_constraints(&mut self, id: u64, old: &[Contour], tie: bool) {
        let Ok(new) = path_contours(&self.s, id).map(<[Contour]>::to_vec) else { return };
        self.s.constraints.retain_mut(|c| {
            let kind = c.kind;
            let snapshot = c.refs.clone();
            c.refs.iter_mut().filter(|r| r.item() == id).all(|r| {
                let Some((n, changed)) = find(old, &new, *r) else { return false };
                let line = matches!(n, Ref::Segment { contour, segment, .. } if new[contour].vertices[segment].bulge == 0.0);
                *r = n;
                !(changed && uses_ends(kind, &snapshot, line))
            })
        });
        // A circle or arc cut into pieces stays one circle, a line one line, when anything holds them.
        if tie || self.s.constraints.iter().any(|c| c.refs.iter().any(|r| r.item() == id)) {
            for c in old {
                for k in 0..if c.as_circle().is_some() { 1 } else { segments(c) } {
                    let Some((pieces, _)) = find_segment(c, k, &new) else { continue };
                    let main = Ref::Segment { item: id, contour: pieces[0].0, segment: pieces[0].1 };
                    let kinds: &[Kind] = if c.vertices[k].bulge == 0.0 { &[Kind::Collinear] } else { &[Kind::Concentric, Kind::Equal] };
                    for &(contour, segment) in &pieces[1..] {
                        for kind in kinds {
                            let _ = self.add_constraint(*kind, vec![main, Ref::Segment { item: id, contour, segment }]);
                        }
                    }
                }
            }
        }
        if let Some(solved) = &mut self.solved {
            if let Some(item) = solved.items.iter_mut().find(|i| i.id == id) {
                item.shape = Shape::Path { contours: new };
            }
            solved.constraints = self.s.constraints.clone();
        }
    }

    /// Bind the ends trimming made on item `id` (open ends that were not ends of `old`) to the
    /// edge each lies on (a point on that line or curve), all or nothing (see `infer_new`).
    pub(crate) fn bind_trimmed_ends(&mut self, id: u64, old: &[Contour], edges: &[u64]) -> usize {
        let Ok(new) = path_contours(&self.s, id) else { return 0 };
        let was_end = |p: Point| old.iter().filter(|c| !c.closed).flat_map(|c| [c.vertices.first(), c.vertices.last()]).flatten().any(|v| distance([v.x, v.y], p) < TOL);
        let mut found = Vec::new();
        for (ci, c) in new.iter().enumerate().filter(|(_, c)| !c.closed) {
            for k in [0, c.vertices.len() - 1] {
                let p = [c.vertices[k].x, c.vertices[k].y];
                if !was_end(p)
                    && let Some(edge) = self.edge_at(edges, id, p)
                {
                    found.push((Kind::Coincident, vec![edge, Ref::Point { item: id, contour: ci, vertex: k }]));
                }
            }
        }
        self.infer_new(found)
    }

    /// The segment of an `edges` item (not `id`) that `p` lies on.
    fn edge_at(&self, edges: &[u64], id: u64, p: Point) -> Option<Ref> {
        for &item in edges.iter().filter(|e| **e != id) {
            let Ok(contours) = path_contours(&self.s, item) else { continue };
            for (contour, c) in contours.iter().enumerate() {
                if let Some(r) = c.pline().closest_point(Vector2::new(p[0], p[1]), 1e-9).filter(|r| r.distance < TOL) {
                    let segment = if c.as_circle().is_some() { 0 } else { r.seg_start_index };
                    return Some(Ref::Segment { item, contour, segment });
                }
            }
        }
        None
    }
}
