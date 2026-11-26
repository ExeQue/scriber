//! Trim (AutoCAD TRIM): cut away the piece of a contour between its nearest crossings with other edges.

use super::{EPS, to_contour};
use crate::model::geom::{Contour, Point};
use cavalier_contours::core::math::Vector2;
use cavalier_contours::polyline::{PlineSource, PlineViewData, Polyline, seg_length, seg_split_at_point};

/// Trim the contour of `contours` nearest to `at`: the piece around `at` up to the nearest crossings with
/// `edges` (and with the other contours) is removed. Returns the remaining contours.
pub fn trim(contours: &[Contour], edges: &[Contour], at: Point) -> Result<Vec<Contour>, String> {
    let pick = Vector2::new(at[0], at[1]);
    let plines: Vec<Polyline<f64>> = contours.iter().map(Contour::pline).collect();
    let (ci, p) = plines
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.closest_point(pick, EPS).map(|r| (i, r.distance)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| (i, &plines[i]))
        .ok_or("there is nothing to trim")?;
    let total = p.path_length();

    // Where the other edges cross, as distances along the contour.
    let others = edges.iter().chain(contours.iter().enumerate().filter(|(i, _)| *i != ci).map(|(_, c)| c));
    let mut cuts: Vec<f64> = Vec::new();
    for e in others {
        let x = p.find_intersects(&e.pline());
        cuts.extend(x.basic_intersects.iter().map(|i| along(p, i.start_index1, i.point)));
        cuts.extend(x.overlapping_intersects.iter().flat_map(|i| [along(p, i.start_index1, i.point1), along(p, i.start_index1, i.point2)]));
    }
    if !p.is_closed() {
        cuts.retain(|s| *s > EPS && *s < total - EPS);
    }
    let picked = p.closest_point(pick, EPS).map(|r| along(p, r.seg_start_index, r.seg_point)).unwrap_or(0.0);
    let before = cuts.iter().copied().filter(|s| *s < picked).max_by(f64::total_cmp);
    let after = cuts.iter().copied().filter(|s| *s > picked).min_by(f64::total_cmp);

    let mut kept: Vec<Contour> = if p.is_closed() {
        if cuts.len() < 2 {
            return Err("a closed contour needs at least two crossings to trim between".into());
        }
        // Keep the way round from the crossing after the pick to the one before it.
        let first = cuts.iter().copied().min_by(f64::total_cmp).unwrap();
        let last = cuts.iter().copied().max_by(f64::total_cmp).unwrap();
        slice(p, after.unwrap_or(first), before.unwrap_or(last)).into_iter().collect()
    } else {
        if before.is_none() && after.is_none() {
            return Err("nothing crosses this contour, so there is nothing to trim to".into());
        }
        [before.and_then(|b| slice(p, 0.0, b)), after.and_then(|a| slice(p, a, total))].into_iter().flatten().collect()
    };
    let mut out = contours.to_vec();
    kept.iter_mut().for_each(super::merge_arcs);
    out.splice(ci..=ci, kept);
    Ok(out)
}

/// Distance along `p` to `point`, which lies on the segment starting at vertex `seg`.
pub(super) fn along(p: &Polyline<f64>, seg: usize, point: Vector2<f64>) -> f64 {
    let before: f64 = p.iter_segments().take(seg).map(|(a, b)| seg_length(a, b)).sum();
    let split = seg_split_at_point(p.at(seg), p.at(p.next_wrapping_index(seg)), point, EPS);
    before + seg_length(split.updated_start, split.split_vertex)
}

/// The piece of `p` from distance `s0` to `s1` (wrapping round for closed contours).
pub(super) fn slice(p: &Polyline<f64>, s0: f64, s1: f64) -> Option<Contour> {
    let point_at = |s: f64| {
        p.find_point_at_path_length(s).unwrap_or_else(|_| {
            let last = p.segment_count() - 1;
            (last, p.at(p.next_wrapping_index(last)).pos())
        })
    };
    let ((i0, a), (i1, b)) = (point_at(s0), point_at(s1));
    PlineViewData::from_slice_points(p, a, i0, b, i1, 1e-6).map(|d| to_contour(&d.view(p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_a_line_between_two_crossings() {
        let line = vec![Contour::line([0.0, 0.0], [30.0, 0.0])];
        let edges = [Contour::line([10.0, -5.0], [10.0, 5.0]), Contour::line([20.0, -5.0], [20.0, 5.0])];
        // Pick the middle: both outer pieces stay.
        let out = trim(&line, &edges, [15.0, 1.0]).unwrap();
        let ends: Vec<[f64; 4]> = out.iter().map(|c| [c.vertices[0].x, c.vertices[0].y, c.vertices[1].x, c.vertices[1].y]).collect();
        assert_eq!(ends, vec![[0.0, 0.0, 10.0, 0.0], [20.0, 0.0, 30.0, 0.0]]);
        // Pick an end: it is cut back to the first crossing.
        let out = trim(&line, &edges, [2.0, 0.0]).unwrap();
        assert_eq!(out.len(), 1);
        assert!((out[0].vertices[0].x - 10.0).abs() < 1e-9 && (out[0].length() - 20.0).abs() < 1e-9);
        assert!(trim(&line, &[Contour::line([0.0, 5.0], [30.0, 5.0])], [2.0, 0.0]).is_err());
    }

    #[test]
    fn trims_an_arc_off_a_circle() {
        // A circle crossed by a vertical line through its centre: trimming the left half keeps the right one.
        let circle = vec![Contour::circle(0.0, 0.0, 10.0)];
        let out = trim(&circle, &[Contour::line([0.0, -20.0], [0.0, 20.0])], [-10.0, 0.0]).unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].closed);
        assert_eq!(out[0].vertices.len(), 2, "one half-circle arc: {:?}", out[0]);
        let b = out[0].bbox();
        assert!(b[0].abs() < 1e-9 && (b[2] - 10.0).abs() < 1e-9 && (b[3] - 20.0).abs() < 1e-9);
        assert!((out[0].length() - std::f64::consts::PI * 10.0).abs() < 1e-6);
    }
}
