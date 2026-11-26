//! The pure part of DIM: what the picks dimension, and which way, from where the pointer is.
//!
//! - one line: its length; two points: their distance; a point and a line: the distance between them;
//! - two lines: the angle between them (the corner towards their far ends);
//! - a circle: its diameter, an arc: its radius (`flip` swaps them).
//!
//! A length or distance is horizontal, vertical or along the points depending on where it is
//! placed (AutoCAD's rule): above or below the measured points (between them left to right) it is
//! horizontal; beside them (between them top to bottom) vertical; anywhere else along the points.

use crate::model::constraints::pick::{DimPlace, dim_place, offset_through};
use crate::model::geom::distance;
use crate::model::{Kind, Point, Ref, State};

/// What a picked reference is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Point,
    Line,
    Arc,
    Circle,
}

pub fn classify(s: &State, r: &Ref) -> Option<Pick> {
    match *r {
        Ref::Point { .. } | Ref::Centre { .. } => Some(Pick::Point),
        Ref::Segment { item, contour, segment } => {
            let c = s.items.iter().find(|i| i.id == item)?.shape.contours().get(contour)?;
            Some(if c.as_circle().is_some() {
                Pick::Circle
            } else if c.vertices.get(segment)?.bulge != 0.0 {
                Pick::Arc
            } else {
                Pick::Line
            })
        }
        Ref::Item { .. } => None,
    }
}

/// The constraint kind the picks make (a length is `DimAligned` until placed); None while a second
/// pick is needed or the picks do not go together.
pub fn plan(s: &State, picks: &[Ref], flip: bool) -> Option<Kind> {
    let kinds: Vec<Pick> = picks.iter().map(|r| classify(s, r)).collect::<Option<_>>()?;
    Some(match kinds.as_slice() {
        [Pick::Circle] if flip => Kind::DimRadial,
        [Pick::Circle] => Kind::DimDiameter,
        [Pick::Arc] if flip => Kind::DimDiameter,
        [Pick::Arc] => Kind::DimRadial,
        [Pick::Line] | [Pick::Point, Pick::Point | Pick::Line] => Kind::DimAligned,
        [Pick::Line, Pick::Line] => Kind::DimAngular,
        _ => return None,
    })
}

/// Horizontal, vertical or aligned, from where the dimension line goes (see the module doc). A
/// kind that would measure nothing (horizontal across a plumb line) falls back to aligned.
pub fn linear_kind(p: Point, q: Point, at: Point) -> Kind {
    let between = |i: usize| (p[i].min(q[i])..=p[i].max(q[i])).contains(&at[i]);
    let spans = |i: usize| (p[i] - q[i]).abs() > 1e-6;
    match (between(0), between(1)) {
        (true, false) if spans(0) => Kind::DimHorizontal,
        (false, true) if spans(1) => Kind::DimVertical,
        _ => Kind::DimAligned,
    }
}

/// What the dimension measures now: mm, or degrees for an angle.
pub fn measured(place: DimPlace, kind: Kind) -> f64 {
    match place {
        DimPlace::Linear(p, q) => match kind {
            Kind::DimHorizontal => (q[0] - p[0]).abs(),
            Kind::DimVertical => (q[1] - p[1]).abs(),
            _ => distance(p, q),
        },
        DimPlace::Angle(_, u, v) => (u[0] * v[1] - u[1] * v[0]).abs().atan2(u[0] * v[0] + u[1] * v[1]).to_degrees(),
        DimPlace::Circle(_, r) if kind == Kind::DimDiameter => 2.0 * r,
        DimPlace::Circle(_, r) => r,
    }
}

/// The dimension the picks make when placed at `at`.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub kind: Kind,
    pub refs: Vec<Ref>,
    pub offset: f64,
    pub value: f64,
}

pub fn layout(s: &State, picks: &[Ref], flip: bool, at: Point) -> Option<Layout> {
    let mut kind = plan(s, picks, flip)?;
    let place = dim_place(s, kind, picks)?;
    let point_to_line = picks.len() == 2 && classify(s, &picks[1]) == Some(Pick::Line);
    if let (Kind::DimAligned, false, DimPlace::Linear(p, q)) = (kind, point_to_line, place) {
        kind = linear_kind(p, q, at);
    }
    Some(Layout { kind, refs: picks.to_vec(), offset: offset_through(place, kind, at), value: measured(place, kind) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Contour, Document, Op, Shape};

    fn seg(item: u64, segment: usize) -> Ref {
        Ref::Segment { item, contour: 0, segment }
    }

    fn pt(item: u64, vertex: usize) -> Ref {
        Ref::Point { item, contour: 0, vertex }
    }

    #[test]
    fn placement_picks_the_linear_kind() {
        let (p, q) = ([0.0, 0.0], [40.0, 10.0]);
        assert_eq!(linear_kind(p, q, [20.0, -15.0]), Kind::DimHorizontal, "above");
        assert_eq!(linear_kind(p, q, [20.0, 30.0]), Kind::DimHorizontal, "below");
        assert_eq!(linear_kind(p, q, [-10.0, 5.0]), Kind::DimVertical, "left");
        assert_eq!(linear_kind(p, q, [60.0, 5.0]), Kind::DimVertical, "right");
        assert_eq!(linear_kind(p, q, [15.0, 8.0]), Kind::DimAligned, "between");
        assert_eq!(linear_kind(p, q, [60.0, 30.0]), Kind::DimAligned, "off a corner");
        // A level line placed beside its end measures nothing vertically: aligned.
        assert_eq!(linear_kind([0.0, 0.0], [40.0, 0.0], [60.0, 0.0]), Kind::DimAligned);
        assert_eq!(linear_kind([0.0, 0.0], [40.0, 0.0], [20.0, -5.0]), Kind::DimHorizontal);
    }

    #[test]
    fn picks_make_the_right_constraint() {
        let mut d = Document::new(200.0, 200.0);
        let r = d.add("", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0)));
        let c = d.add("", Op::Cut, Shape::path(Contour::circle(100.0, 100.0, 10.0)));
        let a = d.add("", Op::Cut, Shape::path(Contour::arc(150.0, 50.0, 5.0, 0.0, 90.0)));
        let slant = d.add("", Op::Cut, Shape::path(Contour::line([0.0, 100.0], [30.0, 140.0])));
        let s = &d.s;
        assert_eq!(classify(s, &seg(r, 0)), Some(Pick::Line));
        assert_eq!(classify(s, &seg(c, 0)), Some(Pick::Circle));
        assert_eq!(classify(s, &seg(a, 0)), Some(Pick::Arc));
        assert_eq!(plan(s, &[seg(c, 0)], false), Some(Kind::DimDiameter));
        assert_eq!(plan(s, &[seg(c, 0)], true), Some(Kind::DimRadial));
        assert_eq!(plan(s, &[seg(a, 0)], false), Some(Kind::DimRadial));
        assert_eq!(plan(s, &[seg(a, 0)], true), Some(Kind::DimDiameter));
        assert_eq!(plan(s, &[seg(r, 0), seg(r, 1)], false), Some(Kind::DimAngular));
        assert_eq!(plan(s, &[pt(r, 0)], false), None, "a point waits for a second one");
        assert_eq!(plan(s, &[pt(r, 0), seg(c, 0)], false), None);

        // The top side, placed above it: its horizontal length, 8 mm above.
        let l = layout(s, &[seg(r, 0)], false, [30.0, 2.0]).unwrap();
        assert_eq!((l.kind, l.refs.clone(), l.offset, l.value), (Kind::DimHorizontal, vec![seg(r, 0)], -8.0, 40.0));
        // Two corners placed beside them: vertical.
        let l = layout(s, &[pt(r, 1), pt(r, 2)], false, [60.0, 20.0]).unwrap();
        assert_eq!((l.kind, l.value), (Kind::DimVertical, 20.0));
        // A slanted line placed just off its middle: along it.
        let l = layout(s, &[seg(slant, 0)], false, [10.0, 125.0]).unwrap();
        assert_eq!((l.kind, l.value), (Kind::DimAligned, 50.0));
        // A corner to a line: always the straight distance.
        let l = layout(s, &[pt(r, 3), seg(r, 0)], false, [5.0, 20.0]).unwrap();
        assert_eq!((l.kind, l.value), (Kind::DimAligned, 20.0));
        let l = layout(s, &[seg(r, 0), seg(r, 1)], false, [45.0, 15.0]).unwrap();
        assert_eq!(l.kind, Kind::DimAngular);
        assert!((l.value - 90.0).abs() < 1e-9);
        let l = layout(s, &[seg(c, 0)], false, [120.0, 100.0]).unwrap();
        assert_eq!((l.kind, l.value, l.offset), (Kind::DimDiameter, 20.0, 0.0));
    }
}
