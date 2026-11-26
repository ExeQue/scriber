//! Drafting aids: where a pointer lands. Object snaps (OSNAP) on existing shapes, ORTHO and
//! POLAR tracking, the status-bar toggles, and the AutoCAD-style snap markers.

use crate::model::geom::{arc_params, distance};
use crate::model::{Item, Kind as Constraint, Point, Ref, Shape};
use eframe::egui::{self, Color32, Key, Pos2, Stroke, pos2, vec2};
use std::f64::consts::{FRAC_PI_2, TAU};

/// The status-bar toggles (SNAP is `App::snap`) and what they did to the pointer this frame.
pub struct Aids {
    pub grid: bool,
    pub ortho: bool,
    pub osnap: bool,
    pub polar: bool,
    /// OTRACK: alignment paths from acquired snap points.
    pub otrack: bool,
    /// INFER: constraints from the snaps used while drawing.
    pub infer: bool,
    /// POLAR's increment angle, and extra angles it also tracks (degrees).
    pub polar_step: f64,
    pub polar_extra: Vec<f64>,
    /// The object snap the pointer locked to.
    pub hit: Option<Snap>,
    /// The tracking path(s) the pointer locked to (POLAR, OTRACK, Extension, Parallel).
    pub tracking: Option<super::tracking::Locked>,
}

impl Default for Aids {
    fn default() -> Self {
        Self { grid: true, ortho: false, osnap: true, polar: true, otrack: true, infer: true, polar_step: 15.0, polar_extra: Vec::new(), hit: None, tracking: None }
    }
}

impl Aids {
    /// Every angle POLAR tracks, in degrees: the increment's multiples plus the extra angles both ways.
    pub fn polar_angles(&self) -> Vec<f64> {
        let step = self.polar_step.clamp(1.0, 180.0);
        let mut out: Vec<f64> = (0..(360.0 / step).round() as usize).map(|k| k as f64 * step).collect();
        out.extend(self.polar_extra.iter().flat_map(|a| [a.rem_euclid(360.0), (a + 180.0).rem_euclid(360.0)]));
        out
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Aid {
    Grid,
    Snap,
    Ortho,
    Osnap,
    Polar,
    Otrack,
    Infer,
}

impl Aid {
    pub const ALL: [Aid; 7] = [Aid::Grid, Aid::Snap, Aid::Ortho, Aid::Polar, Aid::Osnap, Aid::Otrack, Aid::Infer];

    pub fn label(self) -> &'static str {
        match self {
            Aid::Grid => "GRID",
            Aid::Snap => "SNAP",
            Aid::Ortho => "ORTHO",
            Aid::Osnap => "OSNAP",
            Aid::Polar => "POLAR",
            Aid::Otrack => "OTRACK",
            Aid::Infer => "INFER",
        }
    }

    pub fn key(self) -> Option<Key> {
        Some(match self {
            Aid::Grid => Key::F7,
            Aid::Snap => Key::F9,
            Aid::Ortho => Key::F8,
            Aid::Osnap => Key::F3,
            Aid::Polar => Key::F10,
            Aid::Otrack => Key::F11,
            Aid::Infer => return None,
        })
    }

    pub fn help(self) -> &'static str {
        match self {
            Aid::Grid => "Show the grid on the material (F7).",
            Aid::Snap => "Round points to the grid (F9).",
            Aid::Ortho => "Only horizontal and vertical lines from the last point (F8).",
            Aid::Osnap => "Lock onto ends, middles, centres and crossings of shapes (F3).",
            Aid::Polar => "Track angles from the last point and from tracked points (F10). Pick the angle step with ▾.",
            Aid::Otrack => "Rest the pointer on a snap point to track lines from it; dashed lines guide the cursor (F11).",
            Aid::Infer => "Automatically connect what you draw to the points you snap to",
        }
    }
}

/// Object snap kinds, in the order they win when two land on the same spot.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    Endpoint,
    Intersection,
    Quadrant,
    Center,
    Midpoint,
    Perpendicular,
    Tangent,
    Nearest,
    /// Along a line or arc beyond its end (a tracking snap).
    Extension,
    /// Parallel to a line pointed at earlier (a tracking snap).
    Parallel,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Endpoint => "Endpoint",
            Kind::Midpoint => "Midpoint",
            Kind::Center => "Center",
            Kind::Quadrant => "Quadrant",
            Kind::Intersection => "Intersection",
            Kind::Perpendicular => "Perpendicular",
            Kind::Tangent => "Tangent",
            Kind::Nearest => "Nearest",
            Kind::Extension => "Extension",
            Kind::Parallel => "Parallel",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Snap {
    pub p: Point,
    pub kind: Kind,
    /// What it snapped to, in a path item (None for crossings and other shapes): a vertex for an
    /// end, the arc's centre, else the segment.
    pub target: Option<Ref>,
}

impl Snap {
    /// The constraint drawing from this snap infers (INFER): with `target` it binds to.
    pub fn infer(&self) -> Option<(Constraint, Ref)> {
        let kind = match self.kind {
            Kind::Endpoint | Kind::Center | Kind::Nearest | Kind::Quadrant => Constraint::Coincident,
            Kind::Perpendicular => Constraint::Perpendicular,
            Kind::Tangent => Constraint::Tangent,
            Kind::Intersection | Kind::Midpoint | Kind::Extension | Kind::Parallel => return None,
        };
        Some((kind, self.target?))
    }
}

/// Where a piece of an outline sits in its path item: segment `segment` of `contour`, from vertex
/// `segment` to vertex `next`. A circle's halves are both its segment 0.
#[derive(Clone, Copy, Debug)]
struct On {
    item: u64,
    contour: usize,
    segment: usize,
    next: usize,
    circle: bool,
}

impl On {
    fn segment(self) -> Ref {
        Ref::Segment { item: self.item, contour: self.contour, segment: if self.circle { 0 } else { self.segment } }
    }

    fn centre(self) -> Ref {
        Ref::Centre { item: self.item, contour: self.contour, segment: if self.circle { 0 } else { self.segment } }
    }

    /// End 0 or 1 of the piece (a circle has no ends: a point on it).
    fn end(self, k: usize) -> Ref {
        if self.circle {
            return self.segment();
        }
        Ref::Point { item: self.item, contour: self.contour, vertex: if k == 0 { self.segment } else { self.next } }
    }
}

/// A straight or arc piece of an outline. Arcs run from `start` by `sweep` radians around `c`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line(Point, Point),
    Arc { a: Point, b: Point, c: Point, r: f64, start: f64, sweep: f64 },
}

impl Seg {
    pub(super) fn ends(self) -> [Point; 2] {
        match self {
            Seg::Line(a, b) | Seg::Arc { a, b, .. } => [a, b],
        }
    }

    fn mid(self) -> Point {
        match self {
            Seg::Line(a, b) => [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0],
            Seg::Arc { c, r, start, sweep, .. } => polar_point(c, r, start + sweep / 2.0),
        }
    }

    pub(super) fn nearest(self, p: Point) -> Point {
        match self {
            Seg::Line(a, b) => foot(p, a, b).map_or_else(|| if distance(p, a) < distance(p, b) { a } else { b }, |(q, _)| q),
            Seg::Arc { a, b, c, r, start, sweep } => {
                let ang = (p[1] - c[1]).atan2(p[0] - c[0]);
                if on_arc(start, sweep, ang) {
                    polar_point(c, r, ang)
                } else if distance(p, a) < distance(p, b) {
                    a
                } else {
                    b
                }
            }
        }
    }

    /// Where lines from `from` touch this arc.
    fn tangents(self, from: Point) -> Vec<Point> {
        let Seg::Arc { c, r, start, sweep, .. } = self else { return Vec::new() };
        let d = distance(from, c);
        if d <= r + 1e-9 {
            return Vec::new();
        }
        let (base, half) = ((from[1] - c[1]).atan2(from[0] - c[0]), (r / d).acos());
        [base - half, base + half].into_iter().filter(|a| on_arc(start, sweep, *a)).map(|a| polar_point(c, r, a)).collect()
    }

    /// The point where a line from `from` meets this piece at a right angle.
    fn perpendicular(self, from: Point) -> Option<Point> {
        match self {
            Seg::Line(a, b) => foot(from, a, b).map(|(q, _)| q),
            Seg::Arc { c, r, start, sweep, .. } => {
                let ang = (from[1] - c[1]).atan2(from[0] - c[0]);
                (distance(from, c) > 1e-9 && on_arc(start, sweep, ang)).then(|| polar_point(c, r, ang))
            }
        }
    }
}

pub(super) fn polar_point(c: Point, r: f64, ang: f64) -> Point {
    [c[0] + r * ang.cos(), c[1] + r * ang.sin()]
}

/// Whether angle `ang` lies on the arc from `start` sweeping `sweep` (either direction).
fn on_arc(start: f64, sweep: f64, ang: f64) -> bool {
    let d = if sweep >= 0.0 { (ang - start).rem_euclid(TAU) } else { (start - ang).rem_euclid(TAU) };
    d <= sweep.abs() + 1e-9 || d >= TAU - 1e-9
}

/// Foot of `p` on the segment a–b, with its parameter; None when it falls outside the segment.
fn foot(p: Point, a: Point, b: Point) -> Option<(Point, f64)> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return None;
    }
    let t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2;
    (-1e-9..=1.0 + 1e-9).contains(&t).then_some(([a[0] + t * dx, a[1] + t * dy], t))
}

/// Where the segments a–b and c–d cross.
pub fn line_line(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (r, s) = ([b[0] - a[0], b[1] - a[1]], [d[0] - c[0], d[1] - c[1]]);
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let (qx, qy) = (c[0] - a[0], c[1] - a[1]);
    let t = (qx * s[1] - qy * s[0]) / den;
    let u = (qx * r[1] - qy * r[0]) / den;
    let ok = |v: f64| (-1e-9..=1.0 + 1e-9).contains(&v);
    (ok(t) && ok(u)).then(|| [a[0] + t * r[0], a[1] + t * r[1]])
}

/// Where the segment a–b crosses the circle around `c`.
fn line_circle(a: Point, b: Point, c: Point, r: f64) -> Vec<Point> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (fx, fy) = (a[0] - c[0], a[1] - c[1]);
    let qa = dx * dx + dy * dy;
    let qb = 2.0 * (fx * dx + fy * dy);
    let disc = qb * qb - 4.0 * qa * (fx * fx + fy * fy - r * r);
    if qa == 0.0 || disc < 0.0 {
        return Vec::new();
    }
    let root = disc.sqrt();
    [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)]
        .into_iter()
        .filter(|t| (-1e-9..=1.0 + 1e-9).contains(t))
        .map(|t| [a[0] + t * dx, a[1] + t * dy])
        .collect()
}

pub(super) fn circle_circle(c1: Point, r1: f64, c2: Point, r2: f64) -> Vec<Point> {
    let d = distance(c1, c2);
    if d < 1e-12 || d > r1 + r2 || d < (r1 - r2).abs() {
        return Vec::new();
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let (ux, uy) = ((c2[0] - c1[0]) / d, (c2[1] - c1[1]) / d);
    let m = [c1[0] + a * ux, c1[1] + a * uy];
    vec![[m[0] - h * uy, m[1] + h * ux], [m[0] + h * uy, m[1] - h * ux]]
}

fn intersections(s: Seg, t: Seg) -> Vec<Point> {
    let on = |seg: Seg, p: Point| match seg {
        Seg::Line(..) => true,
        Seg::Arc { c, start, sweep, .. } => on_arc(start, sweep, (p[1] - c[1]).atan2(p[0] - c[0])),
    };
    let pts = match (s, t) {
        (Seg::Line(a, b), Seg::Line(c, d)) => line_line(a, b, c, d).into_iter().collect(),
        (Seg::Line(a, b), Seg::Arc { c, r, .. }) | (Seg::Arc { c, r, .. }, Seg::Line(a, b)) => line_circle(a, b, c, r),
        (Seg::Arc { c: c1, r: r1, .. }, Seg::Arc { c: c2, r: r2, .. }) => circle_circle(c1, r1, c2, r2),
    };
    pts.into_iter().filter(|p| on(s, *p) && on(t, *p)).collect()
}

/// The pieces of an item's outline: real arcs for paths, straight lines for the rest.
fn segments(item: &Item) -> Vec<(Seg, Option<On>)> {
    if matches!(item.shape, Shape::Guide(_)) {
        return Vec::new(); // guide lines come in as `guides`, cut to the view
    }
    let Shape::Path { contours } = &item.shape else {
        return item.shape.outline(0.1).iter().flat_map(|l| l.windows(2).map(|w| (Seg::Line(w[0], w[1]), None)).collect::<Vec<_>>()).collect();
    };
    let mut out = Vec::new();
    for (ci, c) in contours.iter().enumerate() {
        let (v, n) = (&c.vertices, c.vertices.len());
        let on = |segment: usize| Some(On { item: item.id, contour: ci, segment, next: (segment + 1) % n, circle: c.as_circle().is_some() });
        let count = if c.closed { n } else { n.saturating_sub(1) };
        for i in 0..count {
            let (a, b) = (v[i], v[(i + 1) % n]);
            let (pa, pb) = ([a.x, a.y], [b.x, b.y]);
            if a.bulge.abs() > 1e-9 {
                let (c, r, start, sweep) = arc_params(pa, pb, a.bulge);
                out.push((Seg::Arc { a: pa, b: pb, c, r, start, sweep }, on(i)));
            } else if pa != pb {
                out.push((Seg::Line(pa, pb), on(i)));
            }
        }
    }
    out
}

/// The pieces of outline (and guide lines) within `tol` mm of `p`, closest first, with the segment
/// they are in a path item.
pub fn pieces_near(items: &[Item], guides: &[[Point; 2]], p: Point, tol: f64) -> Vec<(Seg, Option<Ref>)> {
    let mut out: Vec<_> = near(items, guides, p, tol).into_iter().filter(|(s, _)| distance(p, s.nearest(p)) <= tol).map(|(s, on)| (s, on.map(On::segment))).collect();
    out.sort_by(|(a, _), (b, _)| distance(p, a.nearest(p)).total_cmp(&distance(p, b.nearest(p))));
    out
}

/// Pieces near `p` (also arcs whose centre is near), guide lines last.
fn near(items: &[Item], guides: &[[Point; 2]], p: Point, tol: f64) -> Vec<(Seg, Option<On>)> {
    let guides = guides.iter().map(|[a, b]| (Seg::Line(*a, *b), None));
    items
        .iter()
        .flat_map(segments)
        .chain(guides)
        .filter(|(s, _)| distance(p, s.nearest(p)) <= tol || matches!(s, Seg::Arc { c, .. } if distance(p, *c) <= tol))
        .collect()
}

/// The object snap for pointer `p`: the closest snap point within `tol` mm, else the nearest
/// point on a shape. `from` is the last placed point (for perpendicular and tangent). `guides` are
/// the guide lines cut to (a bit more than) the view: they give ends, crossings and nearest points.
pub fn find(items: &[Item], guides: &[[Point; 2]], p: Point, tol: f64, from: Option<Point>) -> Option<Snap> {
    find_with(items, guides, p, tol, from, None)
}

/// A one-shot object snap override: only snaps of `kind` count, whatever OSNAP says.
pub fn find_only(items: &[Item], guides: &[[Point; 2]], p: Point, tol: f64, from: Option<Point>, kind: Kind) -> Option<Snap> {
    find_with(items, guides, p, tol, from, Some(kind))
}

fn find_with(items: &[Item], guides: &[[Point; 2]], p: Point, tol: f64, from: Option<Point>, only: Option<Kind>) -> Option<Snap> {
    let segs = near(items, guides, p, tol);
    let mut cands = Vec::new();
    let mut add = |kind, q: Point, target: Option<Ref>| cands.push(Snap { p: q, kind, target });
    for (i, &(s, on)) in segs.iter().enumerate() {
        let target = |f: fn(On) -> Ref| on.map(f);
        for (k, q) in s.ends().into_iter().enumerate() {
            add(Kind::Endpoint, q, on.map(|o| o.end(k)));
        }
        // A guide line's middle is only where the view cuts it.
        if on.is_some() || !is_guide(guides, s) {
            add(Kind::Midpoint, s.mid(), target(On::segment));
        }
        if let Seg::Arc { c, r, start, sweep, .. } = s {
            add(Kind::Center, c, target(On::centre));
            for k in 0..4 {
                let ang = k as f64 * FRAC_PI_2;
                if on_arc(start, sweep, ang) {
                    add(Kind::Quadrant, polar_point(c, r, ang), target(On::segment));
                }
            }
        }
        for (t, _) in &segs[i + 1..] {
            intersections(s, *t).into_iter().for_each(|q| add(Kind::Intersection, q, None));
        }
        if let Some(f) = from {
            s.perpendicular(f).into_iter().for_each(|q| add(Kind::Perpendicular, q, target(On::segment)));
            s.tangents(f).into_iter().for_each(|q| add(Kind::Tangent, q, target(On::segment)));
        }
    }
    // Closest wins; on the same spot the kind listed first wins (e.g. a quadrant beats an arc's midpoint).
    let best = cands
        .into_iter()
        .filter(|s| distance(s.p, p) <= tol && only.is_none_or(|k| k == s.kind))
        .min_by_key(|s| ((distance(s.p, p) * 1e6).round() as i64, s.kind));
    if only.is_some_and(|k| k != Kind::Nearest) {
        return best;
    }
    best.or_else(|| {
        segs.iter()
            .map(|(s, on)| (s.nearest(p), on.map(On::segment)))
            .min_by(|a, b| distance(a.0, p).total_cmp(&distance(b.0, p)))
            .map(|(q, target)| Snap { p: q, kind: Kind::Nearest, target })
    })
}

fn is_guide(guides: &[[Point; 2]], s: Seg) -> bool {
    matches!(s, Seg::Line(a, b) if guides.contains(&[a, b]))
}

/// ORTHO: keep only the larger of the horizontal and vertical move from `from`.
pub fn ortho(from: Point, p: Point) -> Point {
    if (p[0] - from[0]).abs() >= (p[1] - from[1]).abs() { [p[0], from[1]] } else { [from[0], p[1]] }
}


/// AutoCAD's marker for a snap kind, centred on `at`.
pub fn paint_marker(painter: &egui::Painter, at: Pos2, kind: Kind) {
    let s = 6.0;
    let stroke = Stroke::new(2.0, Color32::from_rgb(255, 170, 0));
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], stroke);
    let (tl, tr, br, bl) = (at + vec2(-s, -s), at + vec2(s, -s), at + vec2(s, s), at + vec2(-s, s));
    match kind {
        Kind::Endpoint => _ = painter.rect_stroke(egui::Rect::from_two_pos(tl, br), 0.0, stroke, egui::StrokeKind::Middle),
        Kind::Midpoint => _ = painter.add(egui::Shape::closed_line(vec![at + vec2(0.0, -s), br, bl], stroke)),
        Kind::Center => _ = painter.circle_stroke(at, s, stroke),
        Kind::Quadrant => _ = painter.add(egui::Shape::closed_line(vec![at + vec2(0.0, -s), at + vec2(s, 0.0), at + vec2(0.0, s), at + vec2(-s, 0.0)], stroke)),
        Kind::Intersection => {
            line(tl, br);
            line(tr, bl);
        }
        Kind::Perpendicular => {
            line(bl, br);
            line(bl, tl);
            line(pos2(at.x - s, at.y), at);
            line(at, pos2(at.x, at.y + s));
        }
        Kind::Tangent => {
            painter.circle_stroke(at + vec2(0.0, s / 3.0), s * 2.0 / 3.0, stroke);
            line(tl, tr);
        }
        Kind::Nearest => _ = painter.add(egui::Shape::closed_line(vec![tl, tr, bl, br], stroke)),
        Kind::Extension => {
            for dx in [-s, 0.0, s] {
                painter.circle_filled(at + vec2(dx, 0.0), 1.5, stroke.color);
            }
        }
        Kind::Parallel => {
            line(bl + vec2(s * 0.3, 0.0), tr - vec2(s * 0.7, 0.0));
            line(bl + vec2(s, 0.0), tr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Contour;

    fn item(id: u64, c: Contour) -> Item {
        Item { id, name: String::new(), layer: 1, shape: Shape::path(c), note: String::new() }
    }

    fn snap_at(items: &[Item], p: Point) -> Snap {
        find(items, &[], p, 1.0, None).expect("a snap")
    }

    fn close(a: Point, b: Point) -> bool {
        distance(a, b) < 1e-6
    }

    #[test]
    fn line_snaps() {
        let items = [item(1, Contour::line([0.0, 0.0], [10.0, 0.0]))];
        assert_eq!(snap_at(&items, [0.3, 0.2]).kind, Kind::Endpoint);
        let s = snap_at(&items, [5.2, 0.1]);
        assert!(s.kind == Kind::Midpoint && close(s.p, [5.0, 0.0]));
        let s = snap_at(&items, [3.0, 0.5]);
        assert!(s.kind == Kind::Nearest && close(s.p, [3.0, 0.0]));
        assert!(find(&items, &[], [3.0, 5.0], 1.0, None).is_none());
        let s = find(&items, &[], [7.1, 0.2], 1.0, Some([7.0, 20.0])).unwrap();
        assert!(s.kind == Kind::Perpendicular && close(s.p, [7.0, 0.0]));
    }

    #[test]
    fn crossings() {
        let items = [item(1, Contour::line([0.0, 0.0], [10.0, 10.0])), item(2, Contour::line([0.0, 10.0], [10.0, 0.0]))];
        let s = snap_at(&items, [5.3, 4.8]);
        assert!(s.kind == Kind::Intersection && close(s.p, [5.0, 5.0]));
        // Line through a circle: crosses it at (±10, 0) relative to the centre.
        let items = [item(1, Contour::circle(20.0, 20.0, 10.0)), item(2, Contour::line([0.0, 20.0], [40.0, 20.0]))];
        let s = snap_at(&items, [10.2, 20.3]);
        assert!(close(s.p, [10.0, 20.0]), "{s:?}");
        // Two circles crossing.
        let items = [item(1, Contour::circle(0.0, 0.0, 5.0)), item(2, Contour::circle(8.0, 0.0, 5.0))];
        let s = snap_at(&items, [4.2, 3.1]);
        assert!(s.kind == Kind::Intersection && close(s.p, [4.0, 3.0]), "{s:?}");
    }

    #[test]
    fn circle_and_arc_snaps() {
        let items = [item(1, Contour::circle(20.0, 20.0, 10.0))];
        let s = snap_at(&items, [20.4, 20.2]);
        assert!(s.kind == Kind::Center && close(s.p, [20.0, 20.0]));
        let s = snap_at(&items, [20.3, 29.8]);
        assert!(s.kind == Kind::Quadrant && close(s.p, [20.0, 30.0]), "{s:?}");
        // Quarter arc from 0° to 90° (clockwise on screen): midpoint at 45°.
        let items = [item(1, Contour::arc(0.0, 0.0, 10.0, 0.0, 90.0))];
        let m = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
        let s = snap_at(&items, [m + 0.2, m]);
        assert!(s.kind == Kind::Midpoint && close(s.p, [m, m]), "{s:?}");
        // No quadrant at 180°, which is not on the arc.
        assert!(find(&items, &[], [-10.0, 0.0], 1.0, None).is_none());
    }

    #[test]
    fn snaps_say_what_they_hit() {
        let items = [item(1, Contour::line([0.0, 0.0], [10.0, 0.0])), item(2, Contour::circle(30.0, 0.0, 5.0))];
        let at = |p: Point| snap_at(&items, p).target;
        assert_eq!(at([9.8, 0.1]), Some(Ref::Point { item: 1, contour: 0, vertex: 1 }));
        assert_eq!(at([3.0, 0.5]), Some(Ref::Segment { item: 1, contour: 0, segment: 0 }));
        assert_eq!(at([30.2, 0.3]), Some(Ref::Centre { item: 2, contour: 0, segment: 0 }));
        // A circle's "ends" are just points on it.
        assert_eq!(at([25.1, 0.2]), Some(Ref::Segment { item: 2, contour: 0, segment: 0 }));
        assert_eq!(snap_at(&items, [9.8, 0.1]).infer().map(|i| i.0), Some(Constraint::Coincident));
    }

    #[test]
    fn tangent_snap() {
        // From (30, -10) to the circle around (30, 0) radius 5: touches at ±60° from straight up.
        let items = [item(2, Contour::circle(30.0, 0.0, 5.0))];
        let (sx, sy) = (5.0 * 60f64.to_radians().sin(), -5.0 * 60f64.to_radians().cos());
        let s = find(&items, &[], [30.0 + sx + 0.2, sy], 1.0, Some([30.0, -10.0])).unwrap();
        assert!(s.kind == Kind::Tangent && close(s.p, [30.0 + sx, sy]), "{s:?}");
    }

    #[test]
    fn ortho_and_polar_angles() {
        assert_eq!(ortho([0.0, 0.0], [10.0, 3.0]), [10.0, 0.0]);
        assert_eq!(ortho([0.0, 0.0], [2.0, -7.0]), [0.0, -7.0]);
        let aids = Aids { polar_step: 90.0, polar_extra: vec![30.0], ..Aids::default() };
        assert_eq!(aids.polar_angles(), vec![0.0, 90.0, 180.0, 270.0, 30.0, 210.0]);
        assert_eq!(Aids::default().polar_angles().len(), 24);
    }

    #[test]
    fn guide_lines_snap() {
        // A guide line across the view and a line crossing it: their crossing and the guide's
        // nearest point snap, but the guide's middle (where the view cuts it) does not.
        let guides = [[[0.0, 5.0], [100.0, 5.0]]];
        let items = [item(1, Contour::line([20.0, 0.0], [20.0, 10.0]))];
        let s = find(&items, &guides, [20.3, 5.2], 1.0, None).unwrap();
        assert!(s.kind == Kind::Intersection && close(s.p, [20.0, 5.0]), "{s:?}");
        let s = find(&items, &guides, [50.2, 5.3], 1.0, None).unwrap();
        assert_eq!(s.kind, Kind::Nearest);
        assert_eq!(find_only(&items, &guides, [20.3, 5.2], 1.0, None, Kind::Endpoint), None);
        assert_eq!(find_only(&items, &guides, [20.3, 9.5], 1.0, None, Kind::Endpoint).map(|s| s.p), Some([20.0, 10.0]));
    }

}
