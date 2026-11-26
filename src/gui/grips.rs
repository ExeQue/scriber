//! Grips: the handles on selected items. Item grips move one defining point (a vertex, a circle's
//! radius, a dimension end); the selection box grips resize, stretch and rotate everything selected.

use crate::model::geom::{arc_params, distance};
use crate::model::{BBox, Contour, Item, Point, Shape, Transform};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Grip {
    /// A path vertex.
    Vertex { id: u64, contour: usize, vertex: usize },
    /// The middle of a path segment: moves a straight segment, bends an arc through it.
    Mid { id: u64, contour: usize, segment: usize },
    /// A circle's quadrant: changes its radius.
    Radius { id: u64, contour: usize },
    /// A circle's centre: moves that circle.
    Centre { id: u64, contour: usize },
    /// Text insertion point or image centre: moves the item.
    Move { id: u64 },
    /// A dimension's first or second measured point.
    DimEnd { id: u64, second: bool },
    /// A dimension's line: changes its offset.
    DimLine { id: u64 },
    /// A selection box handle at fraction (fx, fy) of the box: corners resize, edges stretch.
    Scale { fx: f64, fy: f64 },
    /// Above the selection box: rotates around the box centre.
    Rotate,
}

impl Grip {
    /// The item an item grip edits; None for the selection box grips.
    pub fn id(self) -> Option<u64> {
        match self {
            Grip::Vertex { id, .. } | Grip::Mid { id, .. } | Grip::Radius { id, .. } | Grip::Centre { id, .. } | Grip::Move { id } | Grip::DimEnd { id, .. } | Grip::DimLine { id } => Some(id),
            Grip::Scale { .. } | Grip::Rotate => None,
        }
    }
}

fn circle(c: &Contour) -> Option<(Point, f64)> {
    c.as_circle()
}

/// How many segments a contour has.
pub fn segment_count(c: &Contour) -> usize {
    if c.closed { c.vertices.len() } else { c.vertices.len().saturating_sub(1) }
}

/// The middle of segment `i` (on the arc for a bent one).
pub fn segment_mid(c: &Contour, i: usize) -> Point {
    let (a, b) = (&c.vertices[i], &c.vertices[(i + 1) % c.vertices.len()]);
    if a.bulge.abs() < 1e-9 {
        return [(a.x + b.x) / 2.0, (a.y + b.y) / 2.0];
    }
    let (m, r, start, sweep) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
    let ang = start + sweep / 2.0;
    [m[0] + r * ang.cos(), m[1] + r * ang.sin()]
}

/// The bulge of the arc from `a` to `b` through `m` (0 when `m` is on the chord).
pub fn bulge_through(a: Point, b: Point, m: Point) -> f64 {
    let (u, v) = ([a[0] - m[0], a[1] - m[1]], [b[0] - m[0], b[1] - m[1]]);
    let cross = (b[0] - a[0]) * (m[1] - a[1]) - (b[1] - a[1]) * (m[0] - a[0]);
    let phi = (u[0] * v[1] - u[1] * v[0]).abs().atan2(u[0] * v[0] + u[1] * v[1]);
    if cross.abs() < 1e-12 {
        return 0.0;
    }
    // The sweep is 2π minus twice the angle at m; the bulge is tan(sweep / 4).
    -cross.signum() * ((std::f64::consts::PI - phi) / 2.0).tan()
}

/// The grips of one selected item, with their positions.
pub fn item_grips(item: &Item) -> Vec<(Grip, Point)> {
    let id = item.id;
    match &item.shape {
        Shape::Path { contours } => {
            let mut out = Vec::new();
            for (ci, c) in contours.iter().enumerate() {
                if let Some((m, r)) = circle(c) {
                    out.push((Grip::Centre { id, contour: ci }, m));
                    for [dx, dy] in [[r, 0.0], [0.0, r], [-r, 0.0], [0.0, -r]] {
                        out.push((Grip::Radius { id, contour: ci }, [m[0] + dx, m[1] + dy]));
                    }
                } else {
                    out.extend(c.vertices.iter().enumerate().map(|(vi, v)| (Grip::Vertex { id, contour: ci, vertex: vi }, [v.x, v.y])));
                    out.extend((0..segment_count(c)).map(|s| (Grip::Mid { id, contour: ci, segment: s }, segment_mid(c, s))));
                }
            }
            out
        }
        Shape::Text { x, y, .. } => vec![(Grip::Move { id }, [*x, *y])],
        Shape::Image { x, y, w, h, .. } => vec![(Grip::Move { id }, [x + w / 2.0, y + h / 2.0])],
        Shape::Guide(g) => vec![(Grip::Move { id }, g.anchor())],
        Shape::Dimension { .. } => {
            let g = item.shape.dimension().unwrap();
            vec![(Grip::DimEnd { id, second: false }, g.a), (Grip::DimEnd { id, second: true }, g.b), (Grip::DimLine { id }, g.mid)]
        }
    }
}

/// `b` grown by `pad` mm on every side.
pub fn padded(b: BBox, pad: f64) -> BBox {
    [b[0] - pad, b[1] - pad, b[2] + 2.0 * pad, b[3] + 2.0 * pad]
}

/// The free-transform grips around the selection box `b`, drawn `pad` mm outside it so they do not
/// cover item grips; the rotate grip sits `lift` mm above the top edge.
pub fn box_grips(b: BBox, pad: f64, lift: f64) -> Vec<(Grip, Point)> {
    let p = padded(b, pad);
    let mut out: Vec<(Grip, Point)> = [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.5, 1.0), (0.0, 1.0), (0.0, 0.5)]
        .into_iter()
        .map(|(fx, fy)| (Grip::Scale { fx, fy }, [p[0] + fx * p[2], p[1] + fy * p[3]]))
        .collect();
    out.push((Grip::Rotate, [p[0] + p[2] / 2.0, p[1] - lift]));
    out
}

/// An item grip moved from `at` to `p`, applied to the shape as it was when the drag started.
pub fn drag_item(shape: &Shape, grip: Grip, at: Point, p: Point) -> Shape {
    let mut s = shape.clone();
    let (dx, dy) = (p[0] - at[0], p[1] - at[1]);
    match (&mut s, grip) {
        (Shape::Path { contours }, Grip::Vertex { contour, vertex, .. }) => {
            if let Some(v) = contours.get_mut(contour).and_then(|c| c.vertices.get_mut(vertex)) {
                (v.x, v.y) = (p[0], p[1]);
            }
        }
        (Shape::Path { contours }, Grip::Mid { contour, segment, .. }) => {
            if let Some(c) = contours.get_mut(contour).filter(|c| segment < segment_count(c)) {
                let n = c.vertices.len();
                let (a, b) = (c.vertices[segment], c.vertices[(segment + 1) % n]);
                if a.bulge.abs() < 1e-9 {
                    for k in [segment, (segment + 1) % n] {
                        c.vertices[k].x += dx;
                        c.vertices[k].y += dy;
                    }
                } else {
                    c.vertices[segment].bulge = bulge_through([a.x, a.y], [b.x, b.y], p);
                }
            }
        }
        (Shape::Path { contours }, Grip::Radius { contour, .. }) => {
            if let Some(c) = contours.get_mut(contour)
                && let Some((m, r)) = circle(c)
            {
                let f = (distance(m, p) / r).max(1e-6);
                *c = c.transformed(&Transform { scale_x: f, scale_y: f, ..Default::default() }, m);
            }
        }
        (Shape::Path { contours }, Grip::Centre { contour, .. }) => {
            // To `p` (not by p - at), so it also works on a shape the solver has moved since.
            if let Some(c) = contours.get_mut(contour)
                && let Some((m, _)) = circle(c)
            {
                *c = c.transformed(&Transform::translate(p[0] - m[0], p[1] - m[1]), [0.0, 0.0]);
            }
        }
        (_, Grip::Move { .. }) => s = s.transformed(&Transform::translate(dx, dy), [0.0, 0.0]),
        (Shape::Dimension { x1, y1, x2, y2, .. }, Grip::DimEnd { second, .. }) => {
            let (x, y) = if second { (x2, y2) } else { (x1, y1) };
            (*x, *y) = (p[0], p[1]);
        }
        (Shape::Dimension { x1, y1, x2, y2, offset }, Grip::DimLine { .. }) => {
            let len = (*x2 - *x1).hypot(*y2 - *y1);
            if len > 0.0 {
                // Signed distance along the dimension's normal (left of 1 → 2 on screen).
                let (ux, uy) = ((*x2 - *x1) / len, (*y2 - *y1) / len);
                *offset = (p[0] - *x1) * uy - (p[1] - *y1) * ux;
            }
        }
        _ => {}
    }
    s
}

/// The transform a selection box grip makes when dragged from `at` to `p`. `b` is the selection's
/// bounding box at the start of the drag. Shift keeps proportions on corners and snaps rotation to 15°;
/// Alt resizes around the box's centre instead of the opposite side.
pub fn box_transform(b: BBox, grip: Grip, at: Point, p: Point, shift: bool, alt: bool) -> Option<Transform> {
    match grip {
        Grip::Scale { fx, fy } => {
            let handle = [b[0] + fx * b[2], b[1] + fy * b[3]];
            let pivot = if alt { [b[0] + b[2] / 2.0, b[1] + b[3] / 2.0] } else { [b[0] + (1.0 - fx) * b[2], b[1] + (1.0 - fy) * b[3]] };
            let target = [handle[0] + p[0] - at[0], handle[1] + p[1] - at[1]];
            let factor = |axis: usize, f: f64| {
                let span = handle[axis] - pivot[axis];
                if f == 0.5 || span.abs() < 1e-9 { 1.0 } else { (target[axis] - pivot[axis]) / span }
            };
            let (mut sx, mut sy) = (factor(0, fx), factor(1, fy));
            if shift && fx != 0.5 && fy != 0.5 {
                let s = if sx.abs() > sy.abs() { sx } else { sy };
                (sx, sy) = (s, s);
            }
            // Scale 0 means "unchanged" in Transform; keep a sliver instead.
            let nz = |s: f64| if s.abs() < 1e-4 { 1e-4_f64.copysign(s) } else { s };
            Some(Transform { scale_x: nz(sx), scale_y: nz(sy), pivot: Some(pivot), ..Default::default() })
        }
        Grip::Rotate => {
            let c = [b[0] + b[2] / 2.0, b[1] + b[3] / 2.0];
            let ang = |q: Point| (q[1] - c[1]).atan2(q[0] - c[0]).to_degrees();
            let mut rotate = ang(p) - ang(at);
            if shift {
                rotate = (rotate / 15.0).round() * 15.0;
            }
            Some(Transform { rotate, pivot: Some(c), ..Default::default() })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(shape: Shape) -> Item {
        Item { id: 7, name: String::new(), layer: 1, shape, note: String::new() }
    }

    #[test]
    fn vertex_and_circle_grips() {
        let rect = item(Shape::path(Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0)));
        let grips = item_grips(&rect);
        assert_eq!(grips.len(), 8, "4 corners and 4 segment middles");
        let (g, at) = grips[2];
        let moved = drag_item(&rect.shape, g, at, [12.0, 15.0]);
        assert_eq!(moved.metrics().bbox, [0.0, 0.0, 12.0, 15.0]);

        let c = item(Shape::path(Contour::circle(5.0, 5.0, 2.0)));
        let grips = item_grips(&c);
        assert_eq!(grips.len(), 5);
        let (g, at) = grips[1];
        assert!(matches!(g, Grip::Radius { .. }));
        let bigger = drag_item(&c.shape, g, at, [5.0, 9.0]);
        assert!((bigger.metrics().bbox[2] - 8.0).abs() < 1e-9);
        let moved = drag_item(&c.shape, grips[0].0, grips[0].1, [6.0, 5.0]);
        assert!((moved.metrics().bbox[0] - 4.0).abs() < 1e-9);
    }

    #[test]
    fn mid_grips_move_lines_and_bend_arcs() {
        let line = item(Shape::path(Contour::line([0.0, 0.0], [10.0, 0.0])));
        let (g, at) = item_grips(&line)[2];
        assert_eq!((g, at), (Grip::Mid { id: 7, contour: 0, segment: 0 }, [5.0, 0.0]));
        let moved = drag_item(&line.shape, g, at, [5.0, 3.0]);
        assert_eq!(moved.metrics().bbox, [0.0, 3.0, 10.0, 0.0]);
        // Bent through a point above the chord, and back through itself.
        let mut arc = Contour::line([0.0, 0.0], [10.0, 0.0]);
        for m in [[5.0, -5.0], [5.0, 2.0], [3.0, -4.0]] {
            arc.vertices[0].bulge = bulge_through([0.0, 0.0], [10.0, 0.0], m);
            let (c, r, ..) = arc_params([0.0, 0.0], [10.0, 0.0], arc.vertices[0].bulge);
            assert!((distance(c, m) - r).abs() < 1e-9, "arc passes through {m:?}");
            let mid = segment_mid(&arc, 0);
            assert!((mid[0] - 5.0).abs() < 1e-9 && (mid[1] < 0.0) == (m[1] < 0.0), "{mid:?} on the side of {m:?}");
        }
        assert!((bulge_through([0.0, 0.0], [10.0, 0.0], [5.0, -5.0]).abs() - 1.0).abs() < 1e-9, "half circle");
    }

    #[test]
    fn dimension_grips() {

        let d = item(Shape::Dimension { x1: 0.0, y1: 0.0, x2: 10.0, y2: 0.0, offset: 5.0 });
        let grips = item_grips(&d);
        let (g, at) = grips[2];
        let s = drag_item(&d.shape, g, at, [5.0, -8.0]);
        assert!(matches!(s, Shape::Dimension { offset, .. } if (offset - 8.0).abs() < 1e-9));
    }

    #[test]
    fn box_resize_and_rotate() {
        let b = [0.0, 0.0, 10.0, 20.0];
        // Bottom-right corner dragged by (+10, +10): 2× wide, 1.5× tall around the top-left.
        let t = box_transform(b, Grip::Scale { fx: 1.0, fy: 1.0 }, [10.0, 20.0], [20.0, 30.0], false, false).unwrap();
        assert_eq!((t.scale_x, t.scale_y, t.pivot), (2.0, 1.5, Some([0.0, 0.0])));
        let t = box_transform(b, Grip::Scale { fx: 1.0, fy: 1.0 }, [10.0, 20.0], [20.0, 30.0], true, false).unwrap();
        assert_eq!((t.scale_x, t.scale_y), (2.0, 2.0));
        // Right edge only stretches x.
        let t = box_transform(b, Grip::Scale { fx: 1.0, fy: 0.5 }, [10.0, 10.0], [15.0, 99.0], false, false).unwrap();
        assert_eq!((t.scale_x, t.scale_y), (1.5, 1.0));
        // Rotating from straight above the centre to its right is +90° (clockwise on screen).
        let t = box_transform(b, Grip::Rotate, [5.0, -10.0], [30.0, 11.0], true, false).unwrap();
        assert_eq!((t.rotate, t.pivot), (90.0, Some([5.0, 10.0])));
        // Alt: around the centre, so the same drag doubles the half-width.
        let t = box_transform(b, Grip::Scale { fx: 1.0, fy: 0.5 }, [10.0, 10.0], [15.0, 99.0], false, true).unwrap();
        assert_eq!((t.scale_x, t.pivot), (2.0, Some([5.0, 10.0])));
        assert!(box_transform(b, Grip::Move { id: 1 }, [0.0, 0.0], [1.0, 1.0], false, false).is_none());
    }
}
