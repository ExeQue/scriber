//! EXTEND (grow an open end to the nearest boundary) and LENGTHEN (change an open end by a length).
//! Lines grow along their direction, arcs along their circle.

use super::EPS;
use super::carrier::Carrier;
use super::trim::slice;
use crate::model::chain::reversed;
use crate::model::geom::{Contour, Point, Vertex, arc_params, distance};
use cavalier_contours::core::math::Vector2;
use cavalier_contours::polyline::PlineSource;
use std::f64::consts::TAU;

/// The open contour turned so the end nearest `at` is its last vertex, and whether it was turned.
fn end_last(c: &Contour, at: Point) -> Result<(Contour, bool), String> {
    if c.closed || c.vertices.len() < 2 {
        return Err("only open lines and arcs have ends to change".into());
    }
    let (f, l) = (c.vertices[0], c.vertices[c.vertices.len() - 1]);
    let flip = distance(at, [f.x, f.y]) < distance(at, [l.x, l.y]);
    Ok((if flip { reversed(c) } else { c.clone() }, flip))
}

fn turned_back(c: Contour, flip: bool) -> Contour {
    if flip { reversed(&c) } else { c }
}

/// Grow the last segment by `len` mm along its line or circle.
fn grow_tail(c: &mut Contour, len: f64) -> Result<(), String> {
    let n = c.vertices.len();
    let (a, b) = (c.vertices[n - 2], c.vertices[n - 1]);
    if a.bulge == 0.0 {
        let l = distance([a.x, a.y], [b.x, b.y]);
        let u = [(b.x - a.x) / l, (b.y - a.y) / l];
        c.vertices[n - 1] = Vertex::new(b.x + u[0] * len, b.y + u[1] * len, 0.0);
        return Ok(());
    }
    let (centre, r, start, sweep) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
    let new = sweep + sweep.signum() * len / r;
    if new.abs() >= TAU - 1e-9 {
        return Err("the arc would close on itself".into());
    }
    let end = start + new;
    c.vertices[n - 2].bulge = (new / 4.0).tan();
    c.vertices[n - 1] = Vertex::new(centre[0] + r * end.cos(), centre[1] + r * end.sin(), 0.0);
    Ok(())
}

/// Change the end of `c` nearest `at` by `delta` mm: longer when positive, shorter when negative.
pub fn lengthen(c: &Contour, at: Point, delta: f64) -> Result<Contour, String> {
    let (mut t, flip) = end_last(c, at)?;
    let total = t.length();
    if delta >= 0.0 {
        grow_tail(&mut t, delta)?;
        return Ok(turned_back(t, flip));
    }
    if total + delta < EPS.sqrt() {
        return Err(format!("the line is only {total:.3} mm long"));
    }
    let short = slice(&t.pline(), 0.0, total + delta).ok_or("could not shorten the line")?;
    Ok(turned_back(short, flip))
}

/// Extend the end of `c` nearest `at` to the first place where it meets one of `boundaries`.
pub fn extend(c: &Contour, at: Point, boundaries: &[Contour]) -> Result<Contour, String> {
    let (mut t, flip) = end_last(c, at)?;
    let n = t.vertices.len();
    let (a, b) = (t.vertices[n - 2], t.vertices[n - 1]);
    let carrier = Carrier::of_segment(a, b).ok_or("the end segment has no length")?;
    let end = [b.x, b.y];
    // How far along the carrier, past the end, a point is (None = behind or the arc would wrap round).
    let ahead = |q: Point| -> Option<f64> {
        match carrier {
            Carrier::Line { u, .. } => Some((q[0] - end[0]) * u[0] + (q[1] - end[1]) * u[1]),
            Carrier::Circle { c, r } => {
                let (_, _, _, sweep) = arc_params([a.x, a.y], end, a.bulge);
                let (pe, pq) = ((end[1] - c[1]).atan2(end[0] - c[0]), (q[1] - c[1]).atan2(q[0] - c[0]));
                let turn = if sweep > 0.0 { (pq - pe).rem_euclid(TAU) } else { (pe - pq).rem_euclid(TAU) };
                (turn + sweep.abs() < TAU - 1e-9).then_some(turn * r)
            }
        }
    };
    let mut best: Option<f64> = None;
    for edge in boundaries {
        let pline = edge.pline();
        let n = edge.vertices.len();
        let segs = if edge.closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            let Some(other) = Carrier::of_segment(edge.vertices[i], edge.vertices[(i + 1) % n]) else { continue };
            for q in carrier.intersect(other) {
                let on_edge = pline.closest_point(Vector2::new(q[0], q[1]), EPS).is_some_and(|r| r.distance < 1e-6);
                if let Some(s) = ahead(q).filter(|s| *s > 1e-6 && on_edge)
                    && best.is_none_or(|b| s < b)
                {
                    best = Some(s);
                }
            }
        }
    }
    let len = best.ok_or("no boundary ahead of that end to extend to")?;
    grow_tail(&mut t, len)?;
    Ok(turned_back(t, flip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn lengthen_by_delta_and_to_a_total() {
        let line = Contour::polyline(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
        let longer = lengthen(&line, [10.0, 9.0], 5.0).unwrap();
        assert!((longer.length() - 25.0).abs() < 1e-9);
        assert!(distance([longer.vertices[2].x, longer.vertices[2].y], [10.0, 15.0]) < 1e-9);
        // Shortening past a corner drops the segment; the start end is picked near (0, 0).
        let shorter = lengthen(&line, [1.0, 0.0], -12.0).unwrap();
        assert!((shorter.length() - 8.0).abs() < 1e-9);
        assert!(distance([shorter.vertices[0].x, shorter.vertices[0].y], [10.0, 2.0]) < 1e-9, "{shorter:?}");
        assert!(lengthen(&line, [0.0, 0.0], -20.0).is_err());
        // An arc grows along its circle: a quarter arc lengthened by a quarter circle is a half circle.
        let arc = Contour::arc(0.0, 0.0, 10.0, 0.0, 90.0);
        let half = lengthen(&arc, [0.0, 10.0], PI * 5.0).unwrap();
        assert!((half.length() - PI * 10.0).abs() < 1e-9);
        assert!(distance([half.vertices[1].x, half.vertices[1].y], [-10.0, 0.0]) < 1e-9);
    }

    #[test]
    fn extend_to_the_nearest_boundary() {
        let line = Contour::line([0.0, 0.0], [10.0, 0.0]);
        let walls = [Contour::line([30.0, -5.0], [30.0, 5.0]), Contour::line([20.0, -5.0], [20.0, 5.0]), Contour::line([15.0, 1.0], [15.0, 5.0])];
        let out = extend(&line, [9.0, 0.0], &walls).unwrap();
        assert!((out.length() - 20.0).abs() < 1e-9, "the nearest wall the line really meets: {out:?}");
        assert!(extend(&line, [1.0, 0.0], &walls).is_err(), "nothing behind the start");
        // An arc extends round its circle to a line.
        let arc = Contour::arc(0.0, 0.0, 10.0, 0.0, 45.0);
        let out = extend(&arc, [7.0, 7.0], &[Contour::line([-20.0, 0.0], [0.0, 0.0])]).unwrap();
        assert!((out.length() - PI * 10.0).abs() < 1e-6, "{}", out.length());
    }
}
