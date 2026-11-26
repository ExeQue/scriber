//! Consecutive arcs on one circle merged into one arc, so pieces cut from a circle (TRIM, BREAK,
//! JOIN) stay a single arc that the constraint solver moves as one.

use crate::model::geom::{Contour, Vertex, arc_params, distance};
use std::f64::consts::FRAC_PI_2;

/// Whether the arcs `a`→`b` and `b`→`c` lie on one circle and turn the same way.
fn co_circular(a: Vertex, b: Vertex, c: Vertex) -> bool {
    if a.bulge == 0.0 || b.bulge == 0.0 || a.bulge.signum() != b.bulge.signum() {
        return false;
    }
    let (ca, ra, ..) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
    let (cb, rb, ..) = arc_params([b.x, b.y], [c.x, c.y], b.bulge);
    let tol = 1e-6 * ra.max(1.0);
    distance(ca, cb) < tol && (ra - rb).abs() < tol
}

/// The bulge of one arc from `a` over `b` to `c` when the two arcs lie on one circle, turn the same
/// way and sweep less than a full turn together.
fn merged(a: Vertex, b: Vertex, c: Vertex) -> Option<f64> {
    // A quarter of the sweep each: together under a quarter turn = under a full circle.
    let quarter = a.bulge.atan() + b.bulge.atan();
    (co_circular(a, b, c) && quarter.abs() < FRAC_PI_2 - 1e-9).then(|| quarter.tan())
}

/// Merge every run of consecutive arcs of `c` that lie on one circle into one arc (any sweep under a
/// full turn). A closed contour that is all one circle becomes two half circles.
pub fn merge_arcs(c: &mut Contour) {
    let mut k = 0;
    while c.vertices.len() > 2 {
        let n = c.vertices.len();
        let pairs = if c.closed { n } else { n - 2 };
        if k >= pairs {
            break;
        }
        let (j, l) = ((k + 1) % n, (k + 2) % n);
        match merged(c.vertices[k], c.vertices[j], c.vertices[l]) {
            Some(bulge) => {
                c.vertices[k].bulge = bulge;
                c.vertices.remove(j);
                // The wrap-around pair dropped vertex 0: everything moved down one.
                k -= usize::from(j < k);
            }
            None => k += 1,
        }
    }
    if let [a, b] = c.vertices[..]
        && c.closed
        && co_circular(a, b, a)
        && c.as_circle().is_none()
    {
        let (centre, ..) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
        let s = a.bulge.signum();
        c.vertices = vec![Vertex::new(a.x, a.y, s), Vertex::new(2.0 * centre[0] - a.x, 2.0 * centre[1] - a.y, s)];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_co_circular_arcs_only() {
        // Three quarter arcs of one circle, then a line: one 270° arc and the line.
        let b = (std::f64::consts::PI / 8.0).tan();
        let v = Vertex::new;
        let mut c = Contour { vertices: vec![v(10.0, 0.0, b), v(0.0, 10.0, b), v(-10.0, 0.0, b), v(0.0, -10.0, 0.0), v(20.0, -10.0, 0.0)], closed: false };
        let length = c.length();
        merge_arcs(&mut c);
        assert_eq!(c.vertices.len(), 3, "{c:?}");
        assert!((c.vertices[0].bulge - (3.0 * std::f64::consts::PI / 8.0).tan()).abs() < 1e-9);
        assert!((c.length() - length).abs() < 1e-9);
        // A full circle stays as it is; so do arcs of different circles.
        let mut circle = Contour::circle(0.0, 0.0, 5.0);
        merge_arcs(&mut circle);
        assert_eq!(circle.vertices.len(), 2);
        // Four quarters of a closed circle: two halves again, not 270° + 90°.
        let mut quarters = Contour { vertices: vec![v(10.0, 0.0, b), v(0.0, 10.0, b), v(-10.0, 0.0, b), v(0.0, -10.0, b)], closed: true };
        merge_arcs(&mut quarters);
        assert_eq!(quarters.as_circle(), Some(([0.0, 0.0], 10.0)), "{quarters:?}");
        let mut s = Contour { vertices: vec![v(0.0, 0.0, 1.0), v(10.0, 0.0, -1.0), v(20.0, 0.0, 0.0)], closed: false };
        merge_arcs(&mut s);
        assert_eq!(s.vertices.len(), 3);
    }
}
