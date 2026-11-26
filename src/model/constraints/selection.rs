//! Constraints for what is already selected, so a constraint command needs no picks: a line, arc
//! or circle stands for itself; two items to join use their closest pair of ends or centres.

use super::attach::points;
use super::geo::path_contours;
use super::pick::nearest_point;
use super::{Kind, Ref};
use crate::model::State;

/// The single line, arc or circle an item is (None for polylines, rectangles and other shapes).
fn single(s: &State, id: u64) -> Option<Ref> {
    let [c] = path_contours(s, id).ok()? else { return None };
    (c.as_circle().is_some() || (!c.closed && c.vertices.len() == 2)).then_some(Ref::Segment { item: id, contour: 0, segment: 0 })
}

fn is_line(s: &State, r: &Ref) -> bool {
    let Ok([c]) = path_contours(s, r.item()) else { return false };
    c.as_circle().is_none() && c.vertices[0].bulge == 0.0
}

/// The reference sets the constraint `kind` takes for the selected `ids` (one set per constraint to
/// add), or what it needs selected instead (then the command asks for picks).
pub fn selection_refs(s: &State, kind: Kind, ids: &[u64]) -> Result<Vec<Vec<Ref>>, String> {
    let singles: Option<Vec<Ref>> = ids.iter().map(|id| single(s, *id)).collect();
    let lines = || singles.as_ref().filter(|v| v.iter().all(|r| is_line(s, r)));
    let no = |what: &str| Err(what.to_string());
    Ok(match (kind, ids.len()) {
        (_, 0) => return no("something"),
        (Kind::Fix, _) => ids.iter().map(|id| vec![Ref::Item { item: *id }]).collect(),
        (Kind::Horizontal | Kind::Vertical, _) => match lines() {
            Some(v) => v.iter().map(|r| vec![*r]).collect(),
            None => return no("only lines"),
        },
        (Kind::DimRadial | Kind::DimDiameter, 1) => match singles.filter(|v| !is_line(s, &v[0])) {
            Some(v) => vec![v],
            None => return no("one arc or circle"),
        },
        (Kind::DimAligned | Kind::DimHorizontal | Kind::DimVertical, 1) | (Kind::DimAngular, 2) => match lines() {
            Some(v) => vec![v.clone()],
            None => return no(if kind == Kind::DimAngular { "two lines" } else { "one line" }),
        },
        (Kind::Coincident, 2) => {
            // The closest pair of a vertex or centre of each.
            let mut best: Option<(Ref, Ref, f64)> = None;
            for (ra, p) in points(s, ids[0]) {
                if let Some((rb, d)) = nearest_point(s, ids[1], p)
                    && best.is_none_or(|x| d < x.2)
                {
                    best = Some((ra, rb, d));
                }
            }
            let Some((ra, rb, _)) = best else { return no("two lines, arcs or polylines") };
            vec![vec![ra, rb]]
        }
        (Kind::Symmetric, 3) => match singles {
            Some(v) if is_line(s, &v[2]) => vec![v],
            _ => return no("two objects and then the symmetry line"),
        },
        (Kind::Parallel | Kind::Perpendicular | Kind::Collinear | Kind::Equal | Kind::Tangent | Kind::Concentric | Kind::Smooth, 2..) => match singles {
            // AutoCAD's multiple mode: each one to the first.
            Some(v) => v[1..].iter().map(|r| vec![v[0], *r]).collect(),
            None => return no("lines, arcs or circles (not polylines)"),
        },
        _ => return no("a different selection"),
    })
}
