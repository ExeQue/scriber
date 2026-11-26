//! Fillet: round a sharp corner with an arc of a given radius.

use super::corner::{Cut, cut_corners};
use crate::model::geom::Contour;
use std::f64::consts::PI;

/// Round the corner at vertex `corner` (None = every sharp corner) with an arc of `radius` mm.
pub fn fillet(c: &Contour, corner: Option<usize>, radius: f64) -> Result<Contour, String> {
    if !(radius > 0.0 && radius.is_finite()) {
        return Err("the fillet radius must be greater than 0".into());
    }
    cut_corners(c, corner, |k| {
        // The arc touches both sides this far from the corner and turns the same way as the path.
        let d = radius / (k.angle / 2.0).tan();
        Cut { back: d, ahead: d, bulge: k.turn * ((PI - k.angle) / 4.0).tan() }
    })
    .map_err(|e| format!("fillet radius {radius} mm: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::arc_params;

    #[test]
    fn rounds_square_corners_like_a_rounded_rect() {
        let filleted = fillet(&Contour::rect(0.0, 0.0, 40.0, 20.0, 0.0), None, 5.0).unwrap();
        let rounded = Contour::rect(0.0, 0.0, 40.0, 20.0, 5.0);
        assert_eq!(filleted.vertices.len(), 8);
        assert!((filleted.area() - rounded.area()).abs() < 1e-9);
        assert!((filleted.length() - rounded.length()).abs() < 1e-9);
        // The first corner's arc is centred 5 mm in from both sides.
        let (a, b) = (filleted.vertices[0], filleted.vertices[1]);
        assert!([a.x, a.y, b.x, b.y].iter().zip([0.0, 5.0, 5.0, 0.0]).all(|(v, e)| (v - e).abs() < 1e-9));
        let (centre, r, _, _) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
        assert!((centre[0] - 5.0).abs() < 1e-9 && (centre[1] - 5.0).abs() < 1e-9 && (r - 5.0).abs() < 1e-9);
    }

    #[test]
    fn one_corner_of_an_open_polyline_and_errors() {
        // A 60° corner: the arc touches each side r / tan(30°) from the corner.
        let c = Contour::polyline(&[[0.0, 0.0], [20.0, 0.0], [10.0, 17.320508075688775]], false);
        let f = fillet(&c, Some(1), 2.0).unwrap();
        assert_eq!(f.vertices.len(), 4);
        let d = 2.0 / 30f64.to_radians().tan();
        assert!((f.vertices[1].x - (20.0 - d)).abs() < 1e-9);
        let (a, b) = (f.vertices[1], f.vertices[2]);
        let (centre, r, _, sweep) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
        assert!((r - 2.0).abs() < 1e-9 && (sweep.abs() - 2.0 * PI / 3.0).abs() < 1e-9);
        assert!(centre[1] > 0.0 && centre[0] < 20.0, "the arc's centre lies inside the corner");
        assert!(fillet(&c, Some(1), 30.0).unwrap_err().contains("only 20.000 mm long"));
        assert!(fillet(&c, Some(0), 1.0).unwrap_err().contains("not a sharp corner"));
        assert!(fillet(&c, None, 0.0).is_err());
        // Circles have no sharp corners: unchanged.
        let circle = Contour::circle(0.0, 0.0, 5.0);
        assert_eq!(fillet(&circle, None, 1.0).unwrap(), circle);
    }
}
