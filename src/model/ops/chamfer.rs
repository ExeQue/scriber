//! Chamfer: cut a sharp corner off with a straight line.

use super::corner::{Cut, cut_corners};
use crate::model::geom::Contour;

/// Cut the corner at vertex `corner` (None = every sharp corner): `d1` mm back along the incoming side,
/// `d2` mm along the outgoing one.
pub fn chamfer(c: &Contour, corner: Option<usize>, d1: f64, d2: f64) -> Result<Contour, String> {
    if !(d1 > 0.0 && d2 > 0.0 && d1.is_finite() && d2.is_finite()) {
        return Err("chamfer distances must be greater than 0".into());
    }
    cut_corners(c, corner, |_| Cut { back: d1, ahead: d2, bulge: 0.0 }).map_err(|e| format!("chamfer {d1} × {d2} mm: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_corners_off() {
        let r = Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0);
        let all = chamfer(&r, None, 2.0, 2.0).unwrap();
        assert_eq!(all.vertices.len(), 8);
        assert!((all.area() - (100.0 - 4.0 * 2.0)).abs() < 1e-9);
        // One corner, unequal distances: the corner at (10, 0) loses a 3 × 1 triangle.
        let one = chamfer(&r, Some(1), 3.0, 1.0).unwrap();
        let pts: Vec<[f64; 2]> = one.vertices.iter().map(|v| [v.x, v.y]).collect();
        assert_eq!(pts, vec![[0.0, 0.0], [7.0, 0.0], [10.0, 1.0], [10.0, 10.0], [0.0, 10.0]]);
        assert!((one.area() - 98.5).abs() < 1e-9);
        // Two 5 mm chamfers use the whole 10 mm side between them; 6 mm does not fit.
        assert_eq!(chamfer(&r, None, 5.0, 5.0).unwrap().vertices.len(), 4);
        assert!(chamfer(&r, None, 6.0, 6.0).unwrap_err().contains("only 10.000 mm long"));
    }
}
