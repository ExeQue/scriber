//! Guide lines (AutoCAD construction geometry): XLINE, RAY, CENTERLINE and CENTERMARK. They are items
//! like any other, so they undo, save and select, but they are never lasered or exported.
//! Centre lines and centre marks made from other items follow those items (see `follow_guides`).

use super::geom::{BBox, Contour, Point, Transform, arc_params, bbox_of, distance};
use super::{Document, Item, Shape};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

/// How far centre lines and centre marks reach past their geometry (AutoCAD's CENTEREXE, metric), mm.
pub const EXT: f64 = 3.5;
/// Half the length of the stand-in segment of an infinite guide where no view is known, mm.
const FAR: f64 = 10_000.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "guide", rename_all = "snake_case")]
pub enum Guide {
    /// Infinite line through (x, y) at `angle` degrees (clockwise on screen, 0 = right).
    Xline { x: f64, y: f64, angle: f64 },
    /// From (x, y) to infinity at `angle` degrees.
    Ray { x: f64, y: f64, angle: f64 },
    /// Centre line from 1 to 2 (drawn `EXT` past both ends); `of` = the two lines it follows.
    Centerline {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        of: Vec<u64>,
    },
    /// Cross through the centre of a circle or arc of radius `r`, reaching `EXT` past it; `of` = the item it follows.
    Centermark {
        cx: f64,
        cy: f64,
        r: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        of: Option<u64>,
    },
}

fn dir(angle: f64) -> Point {
    let (s, c) = angle.to_radians().sin_cos();
    [c, s]
}

/// The part of the line `p + t·u` with `t` in `[t0, t1]` that lies in `view` (Liang–Barsky).
pub fn clip(p: Point, u: Point, t0: f64, t1: f64, view: BBox) -> Option<[Point; 2]> {
    let (mut lo, mut hi) = (t0, t1);
    for k in 0..2 {
        let (min, max) = (view[k], view[k] + view[k + 2]);
        if u[k].abs() < 1e-12 {
            if p[k] < min || p[k] > max {
                return None;
            }
            continue;
        }
        let (a, b) = ((min - p[k]) / u[k], (max - p[k]) / u[k]);
        lo = lo.max(a.min(b));
        hi = hi.min(a.max(b));
    }
    (lo <= hi).then(|| [[p[0] + lo * u[0], p[1] + lo * u[1]], [p[0] + hi * u[0], p[1] + hi * u[1]]])
}

impl Guide {
    /// The point the guide hangs on (base point, start or centre).
    pub fn anchor(&self) -> Point {
        match *self {
            Guide::Xline { x, y, .. } | Guide::Ray { x, y, .. } => [x, y],
            Guide::Centerline { x1, y1, x2, y2, .. } => [(x1 + x2) / 2.0, (y1 + y2) / 2.0],
            Guide::Centermark { cx, cy, .. } => [cx, cy],
        }
    }

    /// The straight pieces of the guide that are inside `view` (what is drawn and snapped to).
    pub fn segments(&self, view: BBox) -> Vec<[Point; 2]> {
        let inf = f64::INFINITY;
        match *self {
            Guide::Xline { x, y, angle } => clip([x, y], dir(angle), -inf, inf, view).into_iter().collect(),
            Guide::Ray { x, y, angle } => clip([x, y], dir(angle), 0.0, inf, view).into_iter().collect(),
            Guide::Centerline { x1, y1, x2, y2, .. } => {
                let len = distance([x1, y1], [x2, y2]);
                if len < 1e-12 {
                    return Vec::new();
                }
                let u = [(x2 - x1) / len, (y2 - y1) / len];
                clip([x1, y1], u, -EXT, len + EXT, view).into_iter().collect()
            }
            Guide::Centermark { cx, cy, r, .. } => {
                let reach = r + EXT;
                [[1.0, 0.0], [0.0, 1.0]].into_iter().filter_map(|u| clip([cx, cy], u, -reach, reach, view)).collect()
            }
        }
    }

    /// The segments with infinite guides cut to a long stand-in, for picking without a view.
    pub fn outline(&self) -> Vec<Vec<Point>> {
        let [x, y] = self.anchor();
        self.segments([x - FAR, y - FAR, 2.0 * FAR, 2.0 * FAR]).into_iter().map(|s| s.to_vec()).collect()
    }

    /// Bounding box of the finite part (an infinite guide counts as its base point).
    pub fn bbox(&self) -> BBox {
        match self {
            Guide::Xline { .. } | Guide::Ray { .. } => bbox_of(&[self.anchor()]),
            _ => bbox_of(&self.outline().concat()),
        }
    }

    pub fn transformed(&self, t: &Transform, pivot: Point) -> Guide {
        let (sx, sy) = t.scales();
        let turned = |x: f64, y: f64, angle: f64| {
            let [a, b] = [t.apply([x, y], pivot), t.apply([x + dir(angle)[0], y + dir(angle)[1]], pivot)];
            (a, (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees())
        };
        match self {
            Guide::Xline { x, y, angle } => {
                let ([x, y], angle) = turned(*x, *y, *angle);
                Guide::Xline { x, y, angle }
            }
            Guide::Ray { x, y, angle } => {
                let ([x, y], angle) = turned(*x, *y, *angle);
                Guide::Ray { x, y, angle }
            }
            Guide::Centerline { x1, y1, x2, y2, of } => {
                let ([x1, y1], [x2, y2]) = (t.apply([*x1, *y1], pivot), t.apply([*x2, *y2], pivot));
                Guide::Centerline { x1, y1, x2, y2, of: of.clone() }
            }
            Guide::Centermark { cx, cy, r, of } => {
                let [cx, cy] = t.apply([*cx, *cy], pivot);
                Guide::Centermark { cx, cy, r: r * (sx.abs() * sy.abs()).sqrt(), of: *of }
            }
        }
    }

    /// AutoCAD's CENTERLINE of two lines: from the middle of their start points to the middle of their
    /// end points, the second line taken the same way round as the first.
    pub fn centerline(a: &Contour, b: &Contour) -> Option<Guide> {
        let ends = |c: &Contour| -> Option<[Point; 2]> {
            let (f, l) = (c.vertices.first()?, c.vertices.last()?);
            (c.vertices.len() >= 2).then_some([[f.x, f.y], [l.x, l.y]])
        };
        let ([a1, a2], [mut b1, mut b2]) = (ends(a)?, ends(b)?);
        let along = (a2[0] - a1[0]) * (b2[0] - b1[0]) + (a2[1] - a1[1]) * (b2[1] - b1[1]);
        if along < 0.0 {
            std::mem::swap(&mut b1, &mut b2);
        }
        let mid = |p: Point, q: Point| [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
        let ([x1, y1], [x2, y2]) = (mid(a1, b1), mid(a2, b2));
        Some(Guide::Centerline { x1, y1, x2, y2, of: Vec::new() })
    }

    /// Centre and radius of a circle or a single arc (what a centre mark goes on).
    pub fn circle_of(c: &Contour) -> Option<(Point, f64)> {
        if let Some(found) = c.as_circle() {
            return Some(found);
        }
        match c.vertices.as_slice() {
            [a, b] if a.bulge != 0.0 && !c.closed => {
                let (centre, r, _, _) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
                Some((centre, r))
            }
            _ => None,
        }
    }
}

impl Document {
    /// Keep centre lines and centre marks on the items they were made from (called on every change).
    pub(super) fn follow_guides(&mut self) {
        let first = |d: &Document, id: u64| d.item(id).and_then(|i| i.shape.contours().first().cloned());
        let mut updates = Vec::new();
        for (n, item) in self.s.items.iter().enumerate() {
            let Shape::Guide(g) = &item.shape else { continue };
            let new = match g {
                Guide::Centerline { of, .. } if of.len() == 2 => match (first(self, of[0]), first(self, of[1])) {
                    (Some(a), Some(b)) => Guide::centerline(&a, &b).map(|g| match g {
                        Guide::Centerline { x1, y1, x2, y2, .. } => Guide::Centerline { x1, y1, x2, y2, of: of.clone() },
                        other => other,
                    }),
                    _ => None,
                },
                Guide::Centermark { of: Some(id), .. } => first(self, *id).and_then(|c| Guide::circle_of(&c)).map(|([cx, cy], r)| Guide::Centermark { cx, cy, r, of: Some(*id) }),
                _ => None,
            };
            if let Some(new) = new.filter(|n| n != g) {
                updates.push((n, new));
            }
        }
        for (n, g) in updates {
            self.s.items[n].shape = Shape::Guide(g);
        }
    }
}

/// Every visible piece of the guide lines among `items` in `view`: for object snaps and EXTEND boundaries.
pub fn guide_segments<'a>(items: impl IntoIterator<Item = &'a Item>, view: BBox) -> Vec<[Point; 2]> {
    items.into_iter().filter_map(|i| if let Shape::Guide(g) = &i.shape { Some(g.segments(view)) } else { None }).flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Op;

    fn close(a: Point, b: Point) -> bool {
        distance(a, b) < 1e-9
    }

    #[test]
    fn xlines_and_rays_are_clipped_to_the_view() {
        let view = [0.0, 0.0, 100.0, 50.0];
        let x = Guide::Xline { x: 10.0, y: 10.0, angle: 0.0 };
        let [a, b] = x.segments(view)[0];
        assert!(close(a, [0.0, 10.0]) && close(b, [100.0, 10.0]));
        // A ray only goes one way.
        let r = Guide::Ray { x: 10.0, y: 10.0, angle: 180.0 };
        let [a, b] = r.segments(view)[0];
        assert!(close(a, [10.0, 10.0]) && close(b, [0.0, 10.0]), "{a:?} {b:?}");
        // 45° down-right through the origin leaves at the bottom edge.
        let d = Guide::Xline { x: 0.0, y: 0.0, angle: 45.0 }.segments(view)[0];
        assert!(close(d[0], [0.0, 0.0]) && close(d[1], [50.0, 50.0]));
        assert!(Guide::Xline { x: 0.0, y: 80.0, angle: 0.0 }.segments(view).is_empty());
        assert!(Guide::Ray { x: 10.0, y: 10.0, angle: 0.0 }.transformed(&Transform::translate(0.0, 100.0), [0.0, 0.0]).segments(view).is_empty());
    }

    #[test]
    fn centre_lines_and_marks_follow_their_items() {
        let mut d = Document::new(100.0, 100.0);
        let a = d.add("a", Op::Cut, Shape::path(Contour::line([0.0, 0.0], [20.0, 0.0])));
        let b = d.add("b", Op::Cut, Shape::path(Contour::line([20.0, 10.0], [0.0, 10.0])));
        let c = d.add("c", Op::Cut, Shape::path(Contour::circle(50.0, 50.0, 10.0)));
        let cl = d.add("cl", Op::NoOutput, Shape::Guide(Guide::Centerline { x1: 0.0, y1: 0.0, x2: 0.0, y2: 0.0, of: vec![a, b] }));
        let cm = d.add("cm", Op::NoOutput, Shape::Guide(Guide::Centermark { cx: 0.0, cy: 0.0, r: 0.0, of: Some(c) }));
        assert_eq!(d.item(cl).unwrap().shape, Shape::Guide(Guide::Centerline { x1: 0.0, y1: 5.0, x2: 20.0, y2: 5.0, of: vec![a, b] }));
        d.transform(&[c], &Transform { scale_x: 2.0, scale_y: 2.0, ..Default::default() });
        let Shape::Guide(Guide::Centermark { cx, cy, r, .. }) = d.item(cm).unwrap().shape else { panic!() };
        assert!(close([cx, cy], [50.0, 50.0]) && (r - 20.0).abs() < 1e-9);
        // The cross reaches EXT past the circle.
        let segs = guide_segments(&d.s.items, [0.0, 0.0, 100.0, 100.0]);

        assert!(segs.iter().any(|s| close(s[0], [50.0 - 20.0 - EXT, 50.0])));
    }

    #[test]
    fn guides_round_trip_through_json() {
        let s = Shape::Guide(Guide::Xline { x: 1.0, y: 2.0, angle: 30.0 });
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"kind":"guide","guide":"xline","x":1.0,"y":2.0,"angle":30.0}"#);
        assert_eq!(serde_json::from_str::<Shape>(&json).unwrap(), s);
    }
}
