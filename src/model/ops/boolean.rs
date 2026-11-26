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

#[cfg(test)]
mod circle_tests {
    use super::*;
    use crate::model::Shape;
    use crate::model::geom::distance;
    use std::f64::consts::PI;

    /// (centre x, centre y, radius).
    type Disc = (f64, f64, f64);

    /// Exact area and outline length of the union of discs: the arcs of each circle outside all the
    /// others, with the area from Green's theorem along them.
    fn expected(ds: &[Disc]) -> (f64, f64) {
        let (mut area, mut length) = (0.0, 0.0);
        for (i, &(cx, cy, r)) in ds.iter().enumerate() {
            // Where the other circles cross this one, as angles.
            let mut cuts = vec![0.0, 2.0 * PI];
            for (j, &(ox, oy, or)) in ds.iter().enumerate() {
                let d = (ox - cx).hypot(oy - cy);
                if i == j || d >= r + or || d <= (r - or).abs() {
                    continue;
                }
                let (base, half) = ((oy - cy).atan2(ox - cx), ((d * d + r * r - or * or) / (2.0 * d * r)).acos());
                cuts.extend([base - half, base + half].map(|t| t.rem_euclid(2.0 * PI)));
            }
            cuts.sort_by(f64::total_cmp);
            for w in cuts.windows(2) {
                let (t0, t1) = (w[0], w[1]);
                let m = (t0 + t1) / 2.0;
                let p = (cx + r * m.cos(), cy + r * m.sin());
                if ds.iter().enumerate().any(|(j, o)| j != i && (p.0 - o.0).hypot(p.1 - o.1) < o.2) {
                    continue;
                }
                length += r * (t1 - t0);
                area += 0.5 * (r * r * (t1 - t0) + cx * r * (t1.sin() - t0.sin()) - cy * r * (t1.cos() - t0.cos()));
            }
        }
        (area, length)
    }

    fn check(name: &str, cs: &[Contour], area: f64, length: f64) {
        assert!(!cs.is_empty(), "{name}: empty");
        for c in cs {
            assert!(c.closed, "{name}: open {c:?}");
            assert!(c.vertices.iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.bulge.is_finite()), "{name}: {c:?}");
            let n = c.vertices.len();
            for k in 0..n {
                let (a, b) = (c.vertices[k], c.vertices[(k + 1) % n]);
                assert!(distance([a.x, a.y], [b.x, b.y]) > 1e-6, "{name}: degenerate segment {k} in {c:?}");
            }
        }
        let got_len: f64 = cs.iter().map(|c| c.length()).sum();
        let shape = Shape::Path { contours: cs.to_vec() };
        let got_area = shape.metrics().area.unwrap_or(0.0);
        // The drawn outline follows the arcs, closed, with no jumps.
        let drawn = shape.outline(0.001);
        let flat: f64 = drawn.iter().map(|p| p.windows(2).map(|w| distance(w[0], w[1])).sum::<f64>()).sum();
        assert!((got_area - area).abs() < 1e-6 * area.max(1.0), "{name}: area {got_area} vs {area}\n{cs:?}");
        assert!((got_len - length).abs() < 1e-6 * length.max(1.0), "{name}: length {got_len} vs {length}\n{cs:?}");
        assert!((flat - length).abs() < 1e-3 * length, "{name}: drawn length {flat} vs {length}\n{cs:?}");
    }

    fn discs(ds: &[Disc]) -> Vec<Vec<Contour>> {
        ds.iter().map(|d| vec![Contour::circle(d.0, d.1, d.2)]).collect()
    }

    #[test]
    fn union_of_two_circles_is_one_arc_per_circle() {
        // Both pieces sweep over a half circle; each stays one arc between the two crossings.
        for (big, small) in [((0.0, 0.0, 30.0), (35.0, 5.0, 12.0)), ((0.0, 0.0, 10.0), (12.0, 0.0, 10.0))] {
            let u = boolean(BoolOp::Union, &discs(&[big, small]));
            assert_eq!(u.len(), 1);
            assert_eq!(u[0].vertices.len(), 2, "one vertex per crossing: {u:?}");
            let (area, length) = expected(&[big, small]);
            check("union", &u, area, length);
        }
    }

    #[test]
    fn union_of_overlapping_circles_keeps_whole_arcs() {
        let cases: &[(&str, &[Disc])] = &[
            ("two equal", &[(0.0, 0.0, 10.0), (12.0, 0.0, 10.0)]),
            ("two different", &[(0.0, 0.0, 30.0), (35.0, 5.0, 12.0)]),
            ("two diagonal", &[(10.0, 10.0, 20.0), (35.0, 32.0, 25.0)]),
            ("near tangent", &[(0.0, 0.0, 10.0), (19.999, 0.0, 10.0)]),
            ("near internal", &[(0.0, 0.0, 10.0), (4.999, 0.0, 5.0)]),
            ("big, big, small", &[(50.0, 50.0, 40.0), (110.0, 60.0, 35.0), (25.0, 95.0, 15.0)]),
            ("chain", &[(0.0, 0.0, 10.0), (15.0, 0.0, 8.0), (27.0, 3.0, 6.0)]),
            ("small bites", &[(100.0, 100.0, 50.0), (150.0, 100.0, 5.0), (100.0, 152.0, 4.0)]),
            ("triple", &[(50.0, 50.0, 40.0), (100.0, 50.0, 40.0), (75.0, 90.0, 15.0)]),
            ("small on the crossing", &[(50.0, 50.0, 40.0), (100.0, 50.0, 40.0), (75.0, 50.0 + 1500f64.sqrt(), 8.0)]),
            ("ring", &[(0.0, 0.0, 10.0), (15.0, 0.0, 8.0), (7.5, 13.0, 8.0), (-3.0, 8.0, 3.0)]),
            ("tangent", &[(0.0, 0.0, 10.0), (20.0, 0.0, 10.0), (10.0, 10.0, 3.0)]),
        ];
        for (name, ds) in cases {
            let (area, length) = expected(ds);
            check(name, &boolean(BoolOp::Union, &discs(ds)), area, length);
            // Any order gives the same outline.
            let mut rev = ds.to_vec();
            rev.reverse();
            check(name, &boolean(BoolOp::Union, &discs(&rev)), area, length);
        }
    }

    #[test]
    fn random_circles_union_difference_intersection() {
        let mut seed = 12345u64;
        let mut rnd = |lo: f64, hi: f64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            lo + (hi - lo) * ((seed >> 11) as f64 / (1u64 << 53) as f64)
        };
        for n in 0..300 {
            let count = 2 + n % 3;
            let ds: Vec<Disc> = (0..count).map(|_| (rnd(0.0, 100.0), rnd(0.0, 100.0), rnd(3.0, 45.0))).collect();
            let name = format!("case {n}: {ds:?}");
            let (area, length) = expected(&ds);
            check(&name, &boolean(BoolOp::Union, &discs(&ds)), area, length);
            // Two discs: A ∩ B and A − B add up to A.
            let two = discs(&ds[..2]);
            let a = PI * ds[0].2 * ds[0].2;
            let sum: f64 = [BoolOp::Intersection, BoolOp::Difference].iter().map(|&op| Shape::Path { contours: boolean(op, &two) }.metrics().area.unwrap_or(0.0)).sum();
            assert!((sum - a).abs() < 1e-6 * a, "{name}: {sum} vs {a}");
        }
    }

    #[test]
    fn union_of_constrained_circles_in_a_document() {
        use crate::model::{Document, Kind, Op, Ref};
        let ds: &[Disc] = &[(50.0, 50.0, 40.0), (110.0, 60.0, 35.0), (25.0, 95.0, 15.0)];
        let mut d = Document::new(300.0, 300.0);
        let ids: Vec<u64> = ds.iter().map(|c| d.add("c", Op::Cut, Shape::path(Contour::circle(c.0, c.1, c.2)))).collect();
        for &id in &ids {
            d.add_dimension(Kind::DimRadial, vec![Ref::Segment { item: id, contour: 0, segment: 0 }], None, None, 0.0).unwrap();
            d.add_constraint(Kind::Fix, vec![Ref::Centre { item: id, contour: 0, segment: 0 }]).unwrap();
        }
        d.settle(true).unwrap();
        d.boolean(&ids, BoolOp::Union).unwrap();
        d.settle(true).unwrap();
        let (area, length) = expected(ds);
        check("document", d.item(ids[0]).unwrap().shape.contours(), area, length);
    }
}
