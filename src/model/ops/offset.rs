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
    fn offsets_a_270_degree_arc_as_one_arc() {
        let big = Contour::arc(0.0, 0.0, 10.0, 0.0, 270.0);
        let pac = Contour { vertices: vec![big.vertices[0], big.vertices[1], crate::model::geom::Vertex::new(0.0, 0.0, 0.0)], closed: true };
        let out = offset(&[pac], 1.0);
        assert_eq!(out.len(), 1);
        let b = out[0].bbox().map(|v| (v * 1e6).round() / 1e6);
        assert_eq!(b, [-11.0, -11.0, 22.0, 22.0]);
        let c = &out[0];
        let n = c.vertices.len();
        let arcs: Vec<f64> = (0..n)
            .filter(|&k| c.vertices[k].bulge != 0.0)
            .map(|k| crate::model::geom::arc_params([c.vertices[k].x, c.vertices[k].y], [c.vertices[(k + 1) % n].x, c.vertices[(k + 1) % n].y], c.vertices[k].bulge))
            .filter(|a| (a.1 - 11.0).abs() < 1e-9)
            .map(|a| a.3.abs())
            .collect();
        assert_eq!(arcs.len(), 1, "{c:?}");
        assert!((arcs[0] - 1.5 * PI).abs() < 1e-9, "{arcs:?}");
    }

    #[test]
    fn open_contours_move_to_their_left_on_screen() {
        // Walking right along y = 0, left on screen (y down) is towards negative y.
        let out = offset(&[Contour::line([0.0, 0.0], [10.0, 0.0])], 3.0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bbox(), [0.0, -3.0, 10.0, 0.0]);
    }

    #[test]
    fn a_stem_that_shrinks_to_nothing_is_dropped() {
        // The "m" of "// comment" in Noto Sans Mono Light at 3 mm: its right stem is 0.159 mm wide, so
        // -0.08 mm shrinks it to nothing (this tripped a debug assertion in cavalier_contours).
        let m: &[[f64; 2]] = &[
            [49.18, 109.0], [49.18, 107.404], [49.303, 107.404], [49.327, 107.638], [49.336, 107.638], [49.369, 107.56783],
            [49.408, 107.50533], [49.453, 107.4505], [49.49256, 107.41703], [49.53925, 107.39312], [49.59306, 107.37878], [49.654, 107.374],
            [49.71859, 107.37887], [49.77437, 107.3935], [49.82134, 107.41787], [49.8595, 107.452], [49.9, 107.50867], [49.9315, 107.57467],
            [49.954, 107.65], [49.963, 107.65], [49.99817, 107.57317], [50.03967, 107.50667], [50.0875, 107.4505], [50.12959, 107.41703],
            [50.17937, 107.39312], [50.23684, 107.37878], [50.302, 107.374], [50.37925, 107.38206], [50.446, 107.40625], [50.50225, 107.44656],
            [50.548, 107.503], [50.57716, 107.56024], [50.59984, 107.62876], [50.61604, 107.70856], [50.62576, 107.79964], [50.629, 107.902],
            [50.629, 109.0], [50.47, 109.0], [50.47, 107.938], [50.46691, 107.83994], [50.45763, 107.75575], [50.44216, 107.68544],
            [50.4205, 107.629], [50.39209, 107.58569], [50.35638, 107.55475], [50.31334, 107.53619], [50.263, 107.53], [50.20197, 107.53909],
            [50.14788, 107.56637], [50.10072, 107.61184], [50.0605, 107.6755], [50.02834, 107.75566], [50.00537, 107.85062], [49.99159, 107.96041],
            [49.987, 108.085], [49.987, 109.0], [49.828, 109.0], [49.828, 107.965], [49.82453, 107.85634], [49.81413, 107.76437],
            [49.79678, 107.68909], [49.7725, 107.6305], [49.74166, 107.58653], [49.70462, 107.55512], [49.66141, 107.53628], [49.612, 107.53],
            [49.56372, 107.53497], [49.52087, 107.54987], [49.48347, 107.57472], [49.4515, 107.6095], [49.416, 107.66833], [49.3875, 107.73883],
            [49.366, 107.821], [49.34575, 107.95825], [49.339, 108.106], [49.339, 109.0],
        ];
        let c = Contour { vertices: m.iter().map(|p| crate::model::geom::Vertex::new(p[0], p[1], 0.0)).collect(), closed: true };
        let out = offset(std::slice::from_ref(&c), -0.08);
        assert!(!out.is_empty() && out.iter().all(|o| o.closed));
        assert!(out.iter().map(Contour::area).sum::<f64>() < c.area());
    }
}
