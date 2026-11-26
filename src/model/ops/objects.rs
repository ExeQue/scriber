//! Two picked objects for FILLET and CHAMFER (AutoCAD style): lines, arcs, circles and the end
//! segments of open polylines. Objects are trimmed or extended along their carrier line or circle
//! to a new end point; circles are never trimmed.

use super::EPS;
use super::carrier::{Carrier, dot, sub, turned};
use crate::model::chain::reversed;
use crate::model::geom::{Contour, Point, Vertex, arc_params, distance};
use cavalier_contours::core::math::Vector2;
use cavalier_contours::polyline::PlineSource;
use std::f64::consts::TAU;

/// One picked object: the item's contours and the pick point.
pub type Pick<'a> = (&'a [Contour], Point);

/// What a two-object fillet or chamfer does to the two items.
#[derive(Debug)]
pub enum Edit {
    /// Both became one path: it replaces the first item and the second is deleted.
    Joined(Contour),
    /// New contours for each item (None = unchanged, e.g. a circle; `b` is always None when both
    /// picks are on the same item) and the new arc or line between them, if any.
    Trimmed { a: Option<Vec<Contour>>, b: Option<Vec<Contour>>, bridge: Option<Contour> },
}

impl Edit {
    /// Item `a` with contour `ci` replaced.
    pub(super) fn one(contours: &[Contour], ci: usize, c: Contour) -> Edit {
        let mut all = contours.to_vec();
        all[ci] = c;
        Edit::Trimmed { a: Some(all), b: None, bridge: None }
    }
}

/// An object end moved to a new point: the contour turned so that end is its last vertex, and
/// whether it had to be reversed for that.
pub(super) type Moved = (Contour, bool);

pub(super) struct Obj {
    ci: usize,
    pub carrier: Carrier,
    /// The ends that may move, as `Moved` before moving. Empty for circles.
    pub ends: Vec<Moved>,
    /// The pick, moved onto the object.
    pub at: Point,
}

pub(super) fn last(c: &Contour) -> Point {
    let v = c.vertices[c.vertices.len() - 1];
    [v.x, v.y]
}

/// Contour index, segment index and point of `contours` nearest `at`.
fn nearest(contours: &[Contour], at: Point) -> Result<(usize, usize, Point), String> {
    let pick = Vector2::new(at[0], at[1]);
    contours
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.pline().closest_point(pick, EPS).map(|r| (i, r)))
        .min_by(|a, b| a.1.distance.total_cmp(&b.1.distance))
        .map(|(i, r)| (i, contours[i].segment_of_pline(r.seg_start_index), [r.seg_point.x, r.seg_point.y]))
        .ok_or_else(|| "the item has no outline to pick".into())
}

/// When both picks are on adjacent segments of one contour: (contour, shared vertex, whether the
/// first pick is on the segment coming into that vertex). Errors for other picks on one contour,
/// except the two end segments of an open contour, which `finish` closes.
pub(super) fn corner_between(a: Pick, b: Pick) -> Result<Option<(usize, usize, bool)>, String> {
    if !std::ptr::eq(a.0, b.0) {
        return Ok(None);
    }
    let ((ca, sa, _), (cb, sb, _)) = (nearest(a.0, a.1)?, nearest(b.0, b.1)?);
    if ca != cb {
        return Ok(None);
    }
    let c = &a.0[ca];
    let segs = if c.closed { c.vertices.len() } else { c.vertices.len() - 1 };
    let next = |s: usize| if c.closed { (s + 1) % segs } else { s + 1 };
    if sa == sb {
        return Err("both picks are on the same segment; pick two objects".into());
    }
    if next(sa) == sb {
        return Ok(Some((ca, sb, true)));
    }
    if next(sb) == sa {
        return Ok(Some((ca, sa, false)));
    }
    if !c.closed && sa.min(sb) == 0 && sa.max(sb) == segs - 1 {
        return Ok(None);
    }
    Err("these two segments of one path do not meet; pick segments next to each other, or the two end segments".into())
}

/// The full circle a closed contour is, if it is one.
fn circle_of(c: &Contour) -> Option<Carrier> {
    let v = &c.vertices;
    let n = v.len();
    let arcs: Vec<(Point, f64)> = (0..n)
        .map(|i| {
            let (a, b) = (v[i], v[(i + 1) % n]);
            (a.bulge != 0.0).then(|| {
                let (c, r, _, _) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
                (c, r)
            })
        })
        .collect::<Option<_>>()?;
    let (c0, r0) = *arcs.first()?;
    arcs.iter().all(|(c, r)| distance(*c, c0) < 1e-6 && (r - r0).abs() < 1e-6).then_some(Carrier::Circle { c: c0, r: r0 })
}

/// The object of `contours` picked at `at`; `which` ("first"/"second") names it in errors.
pub(super) fn pick(contours: &[Contour], at: Point, which: &str) -> Result<Obj, String> {
    let (ci, seg, at) = nearest(contours, at)?;
    let c = &contours[ci];
    if c.closed {
        let carrier = circle_of(c).ok_or(format!("the {which} object is a closed shape; pick lines, arcs, open polylines or circles"))?;
        return Ok(Obj { ci, carrier, ends: vec![], at });
    }
    let segs = c.vertices.len() - 1;
    let ends = match seg {
        _ if segs == 1 => vec![(c.clone(), false), (reversed(c), true)],
        0 => vec![(reversed(c), true)],
        s if s + 1 == segs => vec![(c.clone(), false)],
        _ => return Err(format!("the {which} pick is on a middle segment of a polyline; pick one of its end segments")),
    };
    let carrier = Carrier::of_segment(c.vertices[seg], c.vertices[seg + 1]).ok_or(format!("the {which} object has a segment of zero length"))?;
    Ok(Obj { ci, carrier, ends, at })
}

impl Obj {
    pub fn is_circle(&self) -> bool {
        self.ends.is_empty()
    }

    /// Angle of `q` around the last segment's start `f`, in that arc's own direction (0..2π).
    fn swept(&self, f: Vertex, q: Point) -> f64 {
        let Carrier::Circle { c, .. } = self.carrier else { unreachable!("only arcs sweep") };
        let ang = |p: Point| (p[1] - c[1]).atan2(p[0] - c[0]);
        (f.bulge.signum() * (ang(q) - ang([f.x, f.y]))).rem_euclid(TAU)
    }

    /// `o` with its last vertex moved to `t` on the carrier; None when nothing of the end segment
    /// would be left.
    pub fn moved(&self, o: &Contour, t: Point) -> Option<Contour> {
        let n = o.vertices.len();
        let f = o.vertices[n - 2];
        let bulge = if f.bulge == 0.0 {
            // The segment may get shorter or longer, but not turn round past its fixed end.
            if distance([f.x, f.y], t) < EPS || dot(sub(t, [f.x, f.y]), sub(last(o), [f.x, f.y])) <= 0.0 {
                return None;
            }
            0.0
        } else {
            let sweep = self.swept(f, t);
            if !(1e-9..TAU - 1e-9).contains(&sweep) {
                return None;
            }
            (f.bulge.signum() * sweep / 4.0).tan()
        };
        let mut vertices = o.vertices.clone();
        vertices[n - 2].bulge = bulge;
        vertices[n - 1] = Vertex::new(t[0], t[1], 0.0);
        Some(Contour { vertices, closed: false })
    }

    /// The unit direction in which the last segment of `o` arrives at its last vertex.
    pub fn arrival(&self, o: &Contour) -> Point {
        let (f, t) = (o.vertices[o.vertices.len() - 2], last(o));
        match self.carrier {
            Carrier::Circle { c, .. } if f.bulge != 0.0 => turned(sub(t, c)).map(|x| x * f.bulge.signum()),
            _ => {
                let d = sub(t, [f.x, f.y]);
                let l = d[0].hypot(d[1]);
                [d[0] / l, d[1] / l]
            }
        }
    }

    /// Whether the pick lies on the last segment of `o`.
    pub fn holds_pick(&self, o: &Contour) -> bool {
        let (f, t) = (o.vertices[o.vertices.len() - 2], last(o));
        if f.bulge == 0.0 { dot(sub(self.at, t), sub([f.x, f.y], t)) > 0.0 } else { self.swept(f, self.at) < self.swept(f, t) }
    }

    /// The point `d` mm back from the last vertex of `o` along its last segment, if that is long enough.
    pub fn back(&self, o: &Contour, d: f64) -> Option<Point> {
        let (f, t) = (o.vertices[o.vertices.len() - 2], last(o));
        match self.carrier {
            Carrier::Circle { c, r } if f.bulge != 0.0 => {
                let a = (t[1] - c[1]).atan2(t[0] - c[0]) - f.bulge.signum() * d / r;
                (d < r * self.swept(f, t) - EPS).then(|| [c[0] + r * a.cos(), c[1] + r * a.sin()])
            }
            _ => {
                let len = distance([f.x, f.y], t);
                (d < len - EPS).then(|| [t[0] + (f.x - t[0]) * d / len, t[1] + (f.y - t[1]) * d / len])
            }
        }
    }

    /// Move the object's end to `t` (on its carrier): of the ends whose moved contour passes `ok`,
    /// the one that was nearest `t`. `Some(None)` for a circle (left as it is); None when no end works.
    pub fn end_at(&self, t: Point, ok: impl Fn(&Contour) -> bool) -> Option<Option<Moved>> {
        if self.is_circle() {
            return Some(None);
        }
        let options = self.ends.iter().filter_map(|(o, rev)| self.moved(o, t).filter(|m| ok(m)).map(|m| (distance(last(o), t), m, *rev)));
        options.min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, m, rev)| Some((m, rev)))
    }
}

/// Put the result together: the moved ends and the bridge from `t1` to `t2` with `bulge` (None
/// when they meet in a sharp corner). Two single open contours become one path; the two end
/// segments of one open contour close it.
pub(super) fn finish(a: Pick, b: Pick, p: &Obj, q: &Obj, e1: Option<Moved>, e2: Option<Moved>, bridge: Option<(Point, Point, f64)>) -> Edit {
    let same = std::ptr::eq(a.0, b.0);
    if let (Some((c1, _)), Some((c2, _))) = (&e1, &e2)
        && ((same && p.ci == q.ci) || (!same && a.0.len() == 1 && b.0.len() == 1))
    {
        // Along the first object to t1, then the bridge to t2. Two contours: then along the second
        // from t2. One contour: it is the second end's version walked back to the first end, which
        // gets the first end's new segment.
        let mut v = if same {
            let mut ring = reversed(c2).vertices;
            let n = ring.len();
            ring.truncate(n - 2);
            ring.extend_from_slice(&c1.vertices[n - 2..]);
            ring
        } else {
            c1.vertices.clone()
        };
        match bridge {
            Some((_, _, bulge)) => v.last_mut().unwrap().bulge = bulge,
            None => _ = v.pop(),
        }
        if !same {
            v.extend(reversed(c2).vertices);
            return Edit::Joined(Contour { vertices: v, closed: false });
        }
        return Edit::one(a.0, p.ci, Contour { vertices: v, closed: true });
    }
    let put = |all: &mut Vec<Contour>, ci: usize, e: Option<Moved>| {
        if let Some((c, rev)) = e {
            all[ci] = if rev { reversed(&c) } else { c };
        }
    };
    let (mut na, mut nb) = (a.0.to_vec(), b.0.to_vec());
    put(&mut na, p.ci, e1.clone());
    if same {
        put(&mut na, q.ci, e2.clone());
    } else {
        put(&mut nb, q.ci, e2.clone());
    }
    let bridge = bridge.map(|(t1, t2, bulge)| Contour { vertices: vec![Vertex::new(t1[0], t1[1], bulge), Vertex::new(t2[0], t2[1], 0.0)], closed: false });
    Edit::Trimmed { a: (e1.is_some() || (same && e2.is_some())).then_some(na), b: (!same && e2.is_some()).then_some(nb), bridge }
}

/// Bring the objects to the corner where their carriers cross nearest the picks, keeping the picked
/// side of each; then step back `d1` and `d2` mm along them for a chamfer line (0 and 0 = sharp corner).
pub(super) fn meet(a: Pick, b: Pick, p: &Obj, q: &Obj, d1: f64, d2: f64) -> Result<Edit, String> {
    let xs = p.carrier.intersect(q.carrier);
    if xs.is_empty() {
        return Err("the objects do not meet and cannot be extended to meet".into());
    }
    let mut best: Option<(f64, Option<Moved>, Option<Moved>)> = None;
    for x in xs {
        let (Some(e1), Some(e2)) = (p.end_at(x, |o| p.holds_pick(o)), q.end_at(x, |o| q.holds_pick(o))) else { continue };
        let score = distance(x, p.at) + distance(x, q.at);
        if best.as_ref().is_none_or(|b| score < b.0) {
            best = Some((score, e1, e2));
        }
    }
    let (_, e1, e2) = best.ok_or("the objects cannot be trimmed or extended to meet on the picked sides")?;
    if d1 == 0.0 && d2 == 0.0 {
        return Ok(finish(a, b, p, q, e1, e2, None));
    }
    let cut = |obj: &Obj, e: Option<Moved>, d: f64, which: &str| -> Result<Moved, String> {
        let (c, rev) = e.ok_or("chamfer works on lines, arcs and open polylines, not circles")?;
        let too_long = || format!("chamfer distance {d} is too long for the {which} object");
        let t = obj.back(&c, d).ok_or_else(too_long)?;
        Ok((obj.moved(&c, t).ok_or_else(too_long)?, rev))
    };
    let (e1, e2) = (cut(p, e1, d1, "first")?, cut(q, e2, d2, "second")?);
    let bridge = (last(&e1.0), last(&e2.0), 0.0);
    Ok(finish(a, b, p, q, Some(e1), Some(e2), Some(bridge)))
}
