//! BREAK (cut a contour open at one point, or take out the piece between two) and EXPLODE's
//! one-item-per-segment split.

use super::EPS;
use super::trim::{along, slice};
use crate::model::geom::{Contour, Point, Vertex};
use cavalier_contours::core::math::Vector2;
use cavalier_contours::polyline::{PlineSource, Polyline};

/// Distance along `p` to the point of `p` nearest `at`.
fn at_length(p: &Polyline<f64>, at: Point) -> Result<f64, String> {
    let r = p.closest_point(Vector2::new(at[0], at[1]), EPS).ok_or("there is nothing to break")?;
    Ok(along(p, r.seg_start_index, r.seg_point))
}

/// `a` followed by `b` (b starts where a ends).
fn concat(a: Option<Contour>, b: Option<Contour>) -> Option<Contour> {
    match (a, b) {
        (Some(mut a), Some(b)) => {
            a.vertices.pop();
            a.vertices.extend(b.vertices);
            Some(a)
        }
        (a, b) => a.or(b),
    }
}

/// The open contour from `s` round the closed contour `p` back to `s`.
fn opened_at(p: &Polyline<f64>, s: f64, total: f64) -> Option<Contour> {
    let piece = |a: f64, b: f64| if b - a > EPS { slice(p, a, b) } else { None };
    concat(piece(s, total), piece(0.0, s))
}

/// Break `c` at the point nearest `a` (it opens a closed contour there, splits an open one in two), or,
/// with `b`, take out the piece between the two points: on an open contour the part between them, on a
/// closed one the shorter way round. Returns what is left.
pub fn break_at(c: &Contour, a: Point, b: Option<Point>) -> Result<Vec<Contour>, String> {
    let p = c.pline();
    let total = p.path_length();
    let sa = at_length(&p, a)?;
    let Some(b) = b else {
        if c.closed {
            return Ok(opened_at(&p, sa, total).into_iter().collect());
        }
        if sa < EPS.sqrt() || sa > total - EPS.sqrt() {
            return Err("that is the end of the line; break it somewhere along it".into());
        }
        return Ok([slice(&p, 0.0, sa), slice(&p, sa, total)].into_iter().flatten().collect());
    };
    let sb = at_length(&p, b)?;
    let (lo, hi) = (sa.min(sb), sa.max(sb));
    if hi - lo < EPS.sqrt() {
        return break_at(c, a, None);
    }
    let kept: Vec<Contour> = if c.closed {
        if hi - lo <= total - (hi - lo) {
            // Keep the long way: from the second point round to the first.
            opened_at(&p, hi, total).and_then(|o| slice(&o.pline(), 0.0, total - (hi - lo))).into_iter().collect()
        } else {
            slice(&p, lo, hi).into_iter().collect()
        }
    } else {
        let head = (lo > EPS.sqrt()).then(|| slice(&p, 0.0, lo)).flatten();
        let tail = (hi < total - EPS.sqrt()).then(|| slice(&p, hi, total)).flatten();
        [head, tail].into_iter().flatten().collect()
    };
    Ok(kept)
}

/// One open contour per segment (a line or an arc each).
pub fn segments(c: &Contour) -> Vec<Contour> {
    let v = &c.vertices;
    let n = v.len();
    let count = if c.closed { n } else { n.saturating_sub(1) };
    (0..count)
        .map(|i| {
            let b = v[(i + 1) % n];
            Contour { vertices: vec![v[i], Vertex::new(b.x, b.y, 0.0)], closed: false }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::distance;
    use std::f64::consts::PI;

    #[test]
    fn breaks_lines_and_closed_shapes() {
        let line = Contour::line([0.0, 0.0], [30.0, 0.0]);
        let parts = break_at(&line, [10.0, 1.0], None).unwrap();
        assert_eq!(parts.len(), 2);
        assert!((parts[0].length() - 10.0).abs() < 1e-9 && (parts[1].length() - 20.0).abs() < 1e-9);
        assert!(break_at(&line, [0.0, 0.0], None).is_err());
        // Two points take out the piece between them, in either order.
        let parts = break_at(&line, [20.0, 0.0], Some([5.0, 0.0])).unwrap();
        let lengths: Vec<f64> = parts.iter().map(Contour::length).collect();
        assert!((lengths[0] - 5.0).abs() < 1e-9 && (lengths[1] - 10.0).abs() < 1e-9, "{lengths:?}");

        // A square opened at the middle of its top side: one open contour, same length, both ends there.
        let sq = Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0);
        let open = break_at(&sq, [5.0, 0.0], None).unwrap();
        assert_eq!(open.len(), 1);
        assert!(!open[0].closed && (open[0].length() - 40.0).abs() < 1e-9);
        let (f, l) = (open[0].vertices[0], open[0].vertices.last().unwrap());
        assert!(distance([f.x, f.y], [5.0, 0.0]) < 1e-9 && distance([l.x, l.y], [5.0, 0.0]) < 1e-9);
        // Two points on a circle remove the shorter arc between them.
        let circle = Contour::circle(0.0, 0.0, 10.0);
        let left = break_at(&circle, [10.0, 0.0], Some([0.0, 10.0])).unwrap();
        assert_eq!(left.len(), 1);
        assert!((left[0].length() - 1.5 * PI * 10.0).abs() < 1e-6, "{}", left[0].length());
    }

    #[test]
    fn segments_of_a_rounded_rectangle() {
        let r = Contour::rect(0.0, 0.0, 40.0, 20.0, 5.0);
        let parts = segments(&r);
        assert_eq!(parts.len(), 8);
        let sum: f64 = parts.iter().map(Contour::length).sum();
        assert!((sum - r.length()).abs() < 1e-9);
        assert_eq!(parts.iter().filter(|p| p.vertices[0].bulge != 0.0).count(), 4);
    }
}
