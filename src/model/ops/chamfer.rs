//! Chamfer: cut a sharp corner off with a straight line.

use super::corner::{Cut, cut_corners};
use super::objects::{Edit, Pick, corner_between, meet, pick};
use crate::model::geom::Contour;

/// Cut the corner at vertex `corner` (None = every sharp corner): `d1` mm back along the incoming side,
/// `d2` mm along the outgoing one.
pub fn chamfer(c: &Contour, corner: Option<usize>, d1: f64, d2: f64) -> Result<Contour, String> {
    if !(d1 > 0.0 && d2 > 0.0 && d1.is_finite() && d2.is_finite()) {
        return Err("chamfer distances must be greater than 0".into());
    }
    cut_corners(c, corner, |_| Cut { back: d1, ahead: d2, bulge: 0.0 }).map_err(|e| format!("chamfer {d1} × {d2} mm: {e}"))
}

/// Chamfer between two picked objects (AutoCAD CHAMFER): lines, arcs (distances measured along the
/// arc) and the end segments of open polylines. They are brought to the corner where they meet,
/// keeping the picked sides, and cut `d1` mm back along the first and `d2` along the second; 0 and 0
/// leaves the sharp corner.
pub fn chamfer_objects(a: Pick, b: Pick, d1: f64, d2: f64) -> Result<Edit, String> {
    if !(d1 >= 0.0 && d2 >= 0.0 && d1.is_finite() && d2.is_finite()) {
        return Err("chamfer distances must be 0 or more".into());
    }
    if let Some((ci, vertex, first_in)) = corner_between(a, b)? {
        let c = &a.0[ci];
        let (back, ahead) = if first_in { (d1, d2) } else { (d2, d1) };
        return Ok(Edit::one(a.0, ci, if d1 == 0.0 && d2 == 0.0 { c.clone() } else { chamfer(c, Some(vertex), back, ahead)? }));
    }
    let (p, q) = (pick(a.0, a.1, "first")?, pick(b.0, b.1, "second")?);
    if p.is_circle() || q.is_circle() {
        return Err("chamfer works on lines, arcs and open polylines, not circles".into());
    }
    meet(a, b, &p, &q, d1, d2)
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
