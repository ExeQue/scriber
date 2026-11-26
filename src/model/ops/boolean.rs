//! Boolean union / difference / intersection of closed areas, holes included.
//!
//! Cavalier combines two simple closed polylines. Areas with holes are built on top of that as faces
//! (one outline minus its holes), using: A − (o − h) = (A − o) ∪ (A ∩ h) and A ∩ (o − h) = (A ∩ o) − h.

use super::{depth, to_contour, wound};
use crate::model::geom::Contour;
use cavalier_contours::polyline::{BooleanOp, BooleanResultInfo, PlineSource, PlineSourceMut, Polyline};
use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BoolOp {
    /// Everything covered by any of the shapes.
    Union,
    /// The first shape minus all the others.
    Difference,
    /// Only what all the shapes cover.
    Intersection,
}

type Pline = Polyline<f64>;

/// An outline minus its holes. Every polyline here is counter-clockwise (cavalier's positive area).
#[derive(Clone)]
struct Face {
    outer: Pline,
    holes: Vec<Pline>,
}

type Region = Vec<Face>;

fn face(outer: Pline) -> Face {
    Face { outer, holes: Vec::new() }
}

/// Combine the areas of several items (each a list of closed contours, even-odd). Returns the
/// contours of the result: outlines counter-clockwise, holes clockwise.
pub fn boolean(op: BoolOp, inputs: &[Vec<Contour>]) -> Vec<Contour> {
    let mut regions = inputs.iter().map(|cs| region(cs));
    let first = regions.next().unwrap_or_default();
    let result = regions.fold(first, |acc, r| match op {
        BoolOp::Union => union(acc, &r),
        BoolOp::Difference => difference(acc, &r),
        BoolOp::Intersection => intersection(&acc, &r),
    });
    let mut out = Vec::new();
    for f in result {
        out.push(to_contour(&f.outer));
        for mut h in f.holes {
            h.invert_direction_mut();
            out.push(to_contour(&h));
        }
    }
    out
}

/// Faces of one item: outlines (even depth) with the holes (odd depth) directly inside them.
fn region(cs: &[Contour]) -> Region {
    let depths: Vec<usize> = cs.iter().map(|c| depth(c, cs)).collect();
    let inside = |o: &Contour, h: &Contour| h.vertices.first().is_some_and(|v| o.contains([v.x, v.y]));
    (0..cs.len())
        .filter(|&i| depths[i].is_multiple_of(2))
        .map(|i| Face {
            outer: wound(&cs[i], true),
            holes: (0..cs.len()).filter(|&j| depths[j] == depths[i] + 1 && inside(&cs[i], &cs[j])).map(|j| wound(&cs[j], true)).collect(),
        })
        .collect()
}

/// Cavalier's boolean of two simple polylines: (positive areas, enclosed negative areas), all counter-clockwise.
fn simple(a: &Pline, b: &Pline, op: BooleanOp) -> (BooleanResultInfo, Vec<Pline>, Vec<Pline>) {
    let r = a.boolean(b, op);
    let ccw = |mut p: Pline| {
        if p.area() < 0.0 {
            p.invert_direction_mut();
        }
        p
    };
    let pos = r.pos_plines.into_iter().map(|p| ccw(p.pline)).collect();
    let neg = r.neg_plines.into_iter().map(|p| ccw(p.pline)).collect();
    (r.result_info, pos, neg)
}

fn largest(plines: Vec<Pline>) -> Option<Pline> {
    plines.into_iter().max_by(|a, b| a.area().total_cmp(&b.area()))
}

fn contains(outer: &Pline, p: &Pline) -> bool {
    outer.winding_number(p.at(0).pos()) != 0
}

/// `f` minus the simple area `b`.
fn face_minus(f: &Face, b: &Pline) -> Region {
    let (_, pos, neg) = simple(&f.outer, b, BooleanOp::Not);
    if !neg.is_empty() {
        // `b` lies inside the outline: it becomes a hole, merged with the holes it touches.
        let (holes, islands) = merge_holes(&f.holes, b.clone());
        let mut out = vec![Face { outer: f.outer.clone(), holes }];
        out.extend(islands.into_iter().map(face));
        return out;
    }
    let mut out: Region = pos.into_iter().map(face).collect();
    for h in &f.holes {
        out = out.iter().flat_map(|g| face_minus(g, h)).collect();
    }
    out
}

/// Add hole `b` to `holes`, uniting it with every hole it touches. Material enclosed by the united
/// holes comes back as islands.
fn merge_holes(holes: &[Pline], mut b: Pline) -> (Vec<Pline>, Vec<Pline>) {
    let mut rest = holes.to_vec();
    let mut islands = Vec::new();
    'grow: loop {
        for i in 0..rest.len() {
            let (info, pos, neg) = simple(&b, &rest[i], BooleanOp::Or);
            if matches!(info, BooleanResultInfo::Disjoint) {
                continue;
            }
            rest.swap_remove(i);
            b = largest(pos).unwrap_or(b);
            islands.extend(neg);
            continue 'grow;
        }
        break;
    }
    rest.push(b);
    (rest, islands)
}

fn difference(r: Region, s: &Region) -> Region {
    s.iter().fold(r, |acc, g| {
        let mut out: Region = acc.iter().flat_map(|f| face_minus(f, &g.outer)).collect();
        // What lies in the holes of `g` was not subtracted.
        out.extend(intersection(&acc, &g.holes.iter().cloned().map(face).collect()));
        out
    })
}

fn intersection(r: &Region, s: &Region) -> Region {
    s.iter()
        .flat_map(|g| {
            let overlap: Region = r
                .iter()
                .flat_map(|f| {
                    let (_, pos, _) = simple(&f.outer, &g.outer, BooleanOp::And);
                    f.holes.iter().fold(pos.into_iter().map(face).collect::<Region>(), |acc, h| acc.iter().flat_map(|x| face_minus(x, h)).collect())
                })
                .collect();
            g.holes.iter().fold(overlap, |acc, h| acc.iter().flat_map(|x| face_minus(x, h)).collect())
        })
        .collect()
}

fn union(r: Region, s: &Region) -> Region {
    s.iter().fold(r, |acc, g| {
        let before = acc.clone();
        let merged = union_outline(acc, &g.outer);
        // Holes of `g` stay open where the other shapes do not cover them.
        let open = difference(g.holes.iter().cloned().map(face).collect(), &before);
        difference(merged, &open)
    })
}

/// `r` united with the simple area `b`: every face `b` touches merges into one.
fn union_outline(r: Region, b: &Pline) -> Region {
    let mut outer = b.clone();
    let mut gaps: Vec<Pline> = Vec::new();
    let mut merged: Vec<Face> = Vec::new();
    let mut rest = r;
    'grow: loop {
        for i in 0..rest.len() {
            let (info, pos, neg) = simple(&rest[i].outer, &outer, BooleanOp::Or);
            let in_a_hole = || gaps.iter().chain(merged.iter().flat_map(|m| &m.holes)).any(|h| contains(h, &rest[i].outer));
            if matches!(info, BooleanResultInfo::Disjoint) || (matches!(info, BooleanResultInfo::Pline1InsidePline2) && in_a_hole()) {
                continue;
            }
            merged.push(rest.swap_remove(i));
            outer = largest(pos).unwrap_or(outer);
            gaps.extend(neg);
            continue 'grow;
        }
        break;
    }
    // A merged face's holes stay open except where `b` or another merged face covers them.
    let mut holes: Region = gaps.into_iter().map(face).collect();
    for (i, f) in merged.iter().enumerate() {
        let mut open = difference(f.holes.iter().cloned().map(face).collect(), &vec![face(b.clone())]);
        for (j, other) in merged.iter().enumerate() {
            if i != j {
                open = difference(open, &vec![other.clone()]);
            }
        }
        holes.extend(open);
    }
    // A hole face's own holes are islands of material inside the hole.
    rest.push(Face { outer, holes: holes.iter().map(|h| h.outer.clone()).collect() });
    rest.extend(holes.into_iter().flat_map(|h| h.holes.into_iter().map(face)));
    rest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Shape, geom::Contour};
    use std::f64::consts::PI;

    fn area(contours: Vec<Contour>) -> f64 {
        Shape::Path { contours }.metrics().area.unwrap_or(0.0)
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<Contour> {
        vec![Contour::rect(x, y, w, h, 0.0)]
    }

    #[test]
    fn union_difference_intersection_of_two_squares() {
        let (a, b) = (rect(0.0, 0.0, 10.0, 10.0), rect(5.0, 5.0, 10.0, 10.0));
        let u = boolean(BoolOp::Union, &[a.clone(), b.clone()]);
        assert_eq!(u.len(), 1);
        assert!((area(u) - 175.0).abs() < 1e-9);
        assert!((area(boolean(BoolOp::Difference, &[a.clone(), b.clone()])) - 75.0).abs() < 1e-9);
        let i = boolean(BoolOp::Intersection, &[a.clone(), b]);
        assert_eq!(i[0].bbox(), [5.0, 5.0, 5.0, 5.0]);
        assert!(boolean(BoolOp::Intersection, &[a, rect(20.0, 0.0, 5.0, 5.0)]).is_empty());
    }

    #[test]
    fn holes_become_compound_contours_and_survive() {
        let plate = rect(0.0, 0.0, 40.0, 40.0);
        let hole = vec![Contour::circle(20.0, 20.0, 5.0)];
        let d = boolean(BoolOp::Difference, &[plate.clone(), hole.clone()]);
        assert_eq!(d.len(), 2, "outline plus hole");
        let expected = 1600.0 - PI * 25.0;
        assert!((area(d.clone()) - expected).abs() < 1e-6);
        // A second hole overlapping the first merges with it.
        let d2 = boolean(BoolOp::Difference, &[d.clone(), vec![Contour::circle(26.0, 20.0, 5.0)]]);
        assert_eq!(d2.len(), 2);
        assert!(area(d2) < expected);
        // A notch from the edge cuts the plate open; the hole stays.
        let notched = boolean(BoolOp::Difference, &[d.clone(), rect(-5.0, 0.0, 10.0, 10.0)]);
        assert_eq!(notched.len(), 2);
        assert!((area(notched) - (expected - 50.0)).abs() < 1e-6);
        // Uniting the plate with a disc that covers part of the hole leaves the rest of the hole open.
        let u = boolean(BoolOp::Union, &[d.clone(), vec![Contour::circle(20.0, 25.0, 3.0)]]);
        assert_eq!(u.len(), 2);
        let covered = PI * 25.0 - area(boolean(BoolOp::Difference, &[hole.clone(), vec![Contour::circle(20.0, 25.0, 3.0)]]));
        assert!((area(u) - (expected + covered)).abs() < 1e-6);
        // Intersecting with a smaller square keeps the hole that lies inside it.
        let i = boolean(BoolOp::Intersection, &[d, rect(10.0, 10.0, 20.0, 20.0)]);
        assert_eq!(i.len(), 2);
        assert!((area(i) - (400.0 - PI * 25.0)).abs() < 1e-6);
    }

    #[test]
    fn union_of_a_frame_encloses_a_hole() {
        // Four bars that touch end to end make a frame: one outline and one hole.
        let bars = [rect(0.0, 0.0, 30.0, 5.0), rect(25.0, 0.0, 5.0, 30.0), rect(0.0, 25.0, 30.0, 5.0), rect(0.0, 0.0, 5.0, 30.0)];
        let u = boolean(BoolOp::Union, &bars);
        assert_eq!(u.len(), 2);
        assert!((area(u) - (900.0 - 400.0)).abs() < 1e-6);
    }
}
