//! Offset: a parallel copy of the outline at a distance (also kerf compensation).

use super::{depth, to_contour, wound};
use crate::model::geom::Contour;
use cavalier_contours::polyline::PlineSource;
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};

/// Offset contours by `distance` mm. Positive grows the area of closed contours (their holes shrink, by
/// the even-odd rule) and moves open ones to their left on screen; negative shrinks / moves right.
/// For kerf compensation of a part, offset by half the kerf. Empty when the area shrinks away.
pub fn offset(contours: &[Contour], distance: f64) -> Vec<Contour> {
    // Cavalier offsets positive values to the left in y-up maths, which is inwards for counter-clockwise
    // outlines and to the right on our y-down screen; so both cases use -distance.
    let closed: Vec<Contour> = contours.iter().filter(|c| c.closed).cloned().collect();
    let shape = Shape::from_plines(closed.iter().map(|c| wound(c, depth(c, &closed).is_multiple_of(2))));
    let grown = shape.parallel_offset(-distance, &ShapeOffsetOptions::default());
    let mut out: Vec<Contour> = grown.ccw_plines.iter().chain(&grown.cw_plines).map(|p| to_contour(&p.polyline)).collect();
    for c in contours.iter().filter(|c| !c.closed) {
        out.extend(c.pline().parallel_offset(-distance).iter().filter(|p| p.vertex_count() > 1).map(to_contour));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn grows_outlines_and_shrinks_holes() {
        let out = offset(&[Contour::rect(0.0, 0.0, 40.0, 20.0, 0.0)], 2.0);
        assert_eq!(out.len(), 1);
        // Outward offset of a rectangle: rounded corners with radius 2.
        assert_eq!(out[0].bbox(), [-2.0, -2.0, 44.0, 24.0]);
        assert!((out[0].area() - (44.0 * 24.0 - (4.0 - PI) * 4.0)).abs() < 1e-6);
        // Inward: plain smaller rectangle.
        let inner = offset(&[Contour::rect(0.0, 0.0, 40.0, 20.0, 0.0)], -2.0);
        assert!((inner[0].area() - 36.0 * 16.0).abs() < 1e-6);
        // A plate with a hole: the plate grows, the hole shrinks (kerf compensation of a part).
        let plate = [Contour::rect(0.0, 0.0, 40.0, 40.0, 0.0), Contour::circle(20.0, 20.0, 10.0)];
        let out = offset(&plate, 1.0);
        let mut radii: Vec<f64> = out.iter().map(|c| c.bbox()[2]).collect();
        radii.sort_by(f64::total_cmp);
        assert!((radii[0] - 18.0).abs() < 1e-9 && (radii[1] - 42.0).abs() < 1e-9);
        // Shrinking too far leaves nothing.
        assert!(offset(&[Contour::circle(0.0, 0.0, 5.0)], -6.0).is_empty());
    }

    #[test]
    fn open_contours_move_to_their_left_on_screen() {
        // Walking right along y = 0, left on screen (y down) is towards negative y.
        let out = offset(&[Contour::line([0.0, 0.0], [10.0, 0.0])], 3.0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bbox(), [0.0, -3.0, 10.0, 0.0]);
    }
}
