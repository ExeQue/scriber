//! POLYGON: centre first, then the size. The size is measured to a corner (AutoCAD's Inscribed)
//! or to the middle of a side (Circumscribed); the choice is remembered.

use super::App;
use crate::model::geom::distance;
use crate::model::{Contour, Point, Shape};
use std::f64::consts::PI;

/// A regular polygon around `c` whose corner (or, with `flat`, the middle of a side) is at `e`.
pub fn polygon(c: Point, e: Point, sides: u32, flat: bool) -> Contour {
    let n = sides.max(3) as f64;
    let d = distance(c, e);
    let ang = (e[1] - c[1]).atan2(e[0] - c[0]).to_degrees();
    // `Contour::polygon` puts the first corner straight up before rotation.
    let (r, rot) = if flat { (d / (PI / n).cos(), ang + 90.0 + 180.0 / n) } else { (d, ang + 90.0) };
    Contour::polygon(c[0], c[1], r, sides, rot)
}

impl App {
    pub(super) fn polygon_shape(&self, pts: &[Point]) -> Option<Shape> {
        let [c, e, ..] = pts else { return None };
        Some(Shape::path(polygon(*c, *e, self.sides, self.cmdline.polygon_flat)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_or_flat() {
        // Hexagon, size 10 to the right: with a corner there the width is 20, with a flat side there it is 20 across flats.
        let corner = polygon([0.0, 0.0], [10.0, 0.0], 6, false);
        assert!((corner.bbox()[2] - 20.0).abs() < 1e-9);
        let flat = polygon([0.0, 0.0], [10.0, 0.0], 6, true);
        assert!((flat.bbox()[2] - 20.0).abs() < 1e-9 && flat.bbox()[3] > 20.0, "{:?}", flat.bbox());
        // The point given lies on a side, not on a corner.
        assert!(flat.vertices.iter().all(|v| distance([v.x, v.y], [10.0, 0.0]) > 1.0));
    }
}
