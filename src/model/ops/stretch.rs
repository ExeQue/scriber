//! STRETCH: the vertices inside a window move, the others stay, so the segments between them stretch.

use crate::model::geom::{BBox, Contour, Point};

pub fn inside(w: BBox, p: Point) -> bool {
    p[0] >= w[0] && p[0] <= w[0] + w[2] && p[1] >= w[1] && p[1] <= w[1] + w[3]
}

/// The contour with its vertices inside `window` moved by `d`. A circle moves whole when its centre is
/// inside (as in AutoCAD), since moving one of its two vertices would change its size.
pub fn stretch(c: &Contour, window: BBox, d: Point) -> Contour {
    let mut out = c.clone();
    let circle = c.as_circle().map(|(m, _)| inside(window, m));
    for v in &mut out.vertices {
        if circle.unwrap_or_else(|| inside(window, [v.x, v.y])) {
            v.x += d[0];
            v.y += d[1];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enclosed_vertices_move() {
        let r = Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0);
        // A window round the right side: the rectangle gets 5 mm wider.
        let s = stretch(&r, [8.0, -1.0, 5.0, 12.0], [5.0, 0.0]);
        assert_eq!(s.bbox(), [0.0, 0.0, 15.0, 10.0]);
        assert_eq!((s.vertices[0].x, s.vertices[3].x), (0.0, 0.0), "the left side stays");
        let c = Contour::circle(50.0, 50.0, 5.0);
        assert_eq!(stretch(&c, [54.0, 40.0, 5.0, 20.0], [5.0, 0.0]), c, "centre outside: the circle stays");
        assert_eq!(stretch(&c, [45.0, 45.0, 10.0, 10.0], [5.0, 0.0]).as_circle().unwrap().0, [55.0, 50.0]);
    }
}
