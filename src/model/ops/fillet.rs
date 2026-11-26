//! Fillet: round a sharp corner with an arc of a given radius.

use super::EPS;
use super::carrier::{Carrier, cross, dot, sub, turned};
use super::corner::{Cut, cut_corners};
use super::objects::{Edit, Obj, Pick, corner_between, finish, last, meet, pick};
use crate::model::geom::{Contour, Point, distance};
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

/// Fillet between two picked objects (AutoCAD FILLET): lines, arcs, circles and the end segments of
/// open polylines. Of the arcs of `radius` touching both, the one nearest the picks on their sides is
/// used; lines and arcs are trimmed or extended to it, circles stay. Radius 0 makes a sharp corner.
/// Parallel lines get a half circle at the first line's picked end (the radius is ignored).
pub fn fillet_objects(a: Pick, b: Pick, radius: f64) -> Result<Edit, String> {
    if !(radius >= 0.0 && radius.is_finite()) {
        return Err("the fillet radius must be 0 or more".into());
    }
    if let Some((ci, vertex, _)) = corner_between(a, b)? {
        let c = &a.0[ci];
        return Ok(Edit::one(a.0, ci, if radius == 0.0 { c.clone() } else { fillet(c, Some(vertex), radius)? }));
    }
    let (p, q) = (pick(a.0, a.1, "first")?, pick(b.0, b.1, "second")?);
    if radius == 0.0 {
        return meet(a, b, &p, &q, 0.0, 0.0);
    }
    if let (Carrier::Line { u: u1, .. }, Carrier::Line { u: u2, .. }) = (p.carrier, q.carrier)
        && cross(u1, u2).abs() < 1e-9
    {
        return cap(a, b, &p, &q);
    }
    let centres: Vec<Point> = p.carrier.offsets(radius).into_iter().flat_map(|o1| q.carrier.offsets(radius).into_iter().flat_map(move |o2| o1.intersect(o2))).collect();
    if centres.is_empty() {
        return Err(format!("no arc of radius {radius} can touch both objects"));
    }
    // Prefer centres on the picked side of each object (seen from the other), then the nearest to the picks.
    let agree = |x: f64, y: f64| x == 0.0 || y == 0.0 || x == y;
    let mut best: Option<((bool, f64), _, _, _)> = None;
    for c in centres {
        let (Some(t1), Some(t2)) = (p.carrier.foot(c), q.carrier.foot(c)) else { continue };
        let Some((bulge, leave, arrive)) = minor_arc(c, t1, t2) else { continue };
        let e1 = p.end_at(t1, |o| dot(p.arrival(o), leave) > 0.0);
        let e2 = q.end_at(t2, |o| dot(q.arrival(o), arrive) < 0.0);
        let (Some(e1), Some(e2)) = (e1, e2) else { continue };
        let off = !(agree(q.carrier.side(c), q.carrier.side(p.at)) && agree(p.carrier.side(c), p.carrier.side(q.at)));
        let score = (off, distance(t1, p.at) + distance(t2, q.at));
        if best.as_ref().is_none_or(|(s, ..)| score < *s) {
            best = Some((score, e1, e2, (t1, t2, bulge)));
        }
    }
    let (_, e1, e2, bridge) = best.ok_or(format!("radius {radius} is too large for these objects"))?;
    Ok(finish(a, b, &p, &q, e1, e2, Some(bridge)))
}

/// The shorter arc around `c` from `t1` to `t2`: its bulge, the direction it leaves `t1` and the
/// direction it arrives at `t2`. None when they are the same point.
fn minor_arc(c: Point, t1: Point, t2: Point) -> Option<(f64, Point, Point)> {
    let (v1, v2) = (sub(t1, c), sub(t2, c));
    let sweep = cross(v1, v2).atan2(dot(v1, v2));
    if sweep.abs() < 1e-9 {
        return None;
    }
    let s = sweep.signum();
    Some(((sweep / 4.0).tan(), turned(v1).map(|x| x * s), turned(v2).map(|x| x * s)))
}

/// Parallel lines: a half circle from the first line's end nearest its pick across to the second
/// line, which is trimmed or extended to meet it.
fn cap(a: Pick, b: Pick, p: &Obj, q: &Obj) -> Result<Edit, String> {
    let (o, rev) = p.ends.iter().min_by(|x, y| distance(last(&x.0), p.at).total_cmp(&distance(last(&y.0), p.at))).unwrap();
    let t1 = last(o);
    let t2 = q.carrier.foot(t1).unwrap();
    if distance(t1, t2) < EPS {
        return Err("the lines lie on top of each other".into());
    }
    let leave = p.arrival(o);
    let c = [(t1[0] + t2[0]) / 2.0, (t1[1] + t2[1]) / 2.0];
    // Leaving t1 straight on: a positive bulge turns towards increasing angle.
    let bulge = cross(sub(t1, c), leave).signum();
    let e2 = q.end_at(t2, |o| dot(q.arrival(o), leave) > 0.0).ok_or("the second line cannot be trimmed or extended to the cap")?;
    Ok(finish(a, b, p, q, Some((o.clone(), *rev)), e2, Some((t1, t2, bulge))))
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
