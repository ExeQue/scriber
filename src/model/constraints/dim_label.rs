//! Where a dimension's line and label go, from its `offset` and `shift`, and back: the `offset`
//! and `shift` that put the label at a point (dragging the label, as AutoCAD's text grip).

use super::Kind;
use super::pick::DimPlace;
use crate::model::Point;
use crate::model::geom::distance;

/// How far past the circle a radius or diameter label sits, as a share of the radius.
const LEADER: f64 = 1.25;

fn add(p: Point, v: Point, k: f64) -> Point {
    [p[0] + v[0] * k, p[1] + v[1] * k]
}

fn dir(degrees: f64) -> Point {
    let a = degrees.to_radians();
    [a.cos(), a.sin()]
}

fn unit(a: Point, b: Point) -> Point {
    let len = distance(a, b).max(1e-9);
    [(b[0] - a[0]) / len, (b[1] - a[1]) / len]
}

/// The ends of a linear dimension's line, `offset` from the measured points `p`, `q`.
pub fn dim_line(p: Point, q: Point, kind: Kind, offset: f64) -> (Point, Point) {
    match kind {
        Kind::DimHorizontal => ([p[0], p[1] + offset], [q[0], p[1] + offset]),
        Kind::DimVertical => ([p[0] + offset, p[1]], [p[0] + offset, q[1]]),
        _ => {
            let u = unit(p, q);
            let n = [u[1], -u[0]];
            (add(p, n, offset), add(q, n, offset))
        }
    }
}

/// An angular dimension's arc: start angle and sweep (radians, |sweep| ≤ π) from ray `u` to `v`.
pub fn arc_angles(u: Point, v: Point) -> (f64, f64) {
    let a0 = u[1].atan2(u[0]);
    let mut sweep = v[1].atan2(v[0]) - a0;
    if sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    } else if sweep < -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    (a0, sweep)
}

/// Where the label goes.
pub fn label_point(place: DimPlace, kind: Kind, offset: f64, shift: f64) -> Point {
    match place {
        DimPlace::Linear(p, q) => {
            let (a, b) = dim_line(p, q, kind, offset);
            add([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], unit(a, b), shift)
        }
        DimPlace::Angle(i, u, v) => {
            let (a0, sweep) = arc_angles(u, v);
            add(i, dir((a0 + sweep / 2.0).to_degrees() + shift), offset.abs().max(2.0))
        }
        DimPlace::Circle(m, r) => add(m, dir(offset), r * LEADER + shift),
    }
}

/// The `offset` and `shift` that put the label at `at`.
pub fn label_through(place: DimPlace, kind: Kind, at: Point) -> (f64, f64) {
    match place {
        DimPlace::Linear(p, q) => {
            let offset = super::pick::offset_through(place, kind, at);
            let (a, b) = dim_line(p, q, kind, offset);
            let u = unit(a, b);
            (offset, (at[0] - (a[0] + b[0]) / 2.0) * u[0] + (at[1] - (a[1] + b[1]) / 2.0) * u[1])
        }
        DimPlace::Angle(i, u, v) => {
            let (a0, sweep) = arc_angles(u, v);
            let turn = (at[1] - i[1]).atan2(at[0] - i[0]) - (a0 + sweep / 2.0);
            let turn = (turn + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
            (distance(i, at).max(2.0), turn.to_degrees())
        }
        DimPlace::Circle(m, r) => ((at[1] - m[1]).atan2(at[0] - m[0]).to_degrees(), distance(m, at) - r * LEADER),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_round_trip() {
        let places = [
            (DimPlace::Linear([0.0, 0.0], [40.0, 10.0]), Kind::DimHorizontal),
            (DimPlace::Linear([0.0, 0.0], [40.0, 10.0]), Kind::DimVertical),
            (DimPlace::Linear([5.0, 5.0], [30.0, 25.0]), Kind::DimAligned),
            (DimPlace::Angle([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]), Kind::DimAngular),
            (DimPlace::Circle([50.0, 50.0], 10.0), Kind::DimRadial),
        ];
        for (place, kind) in places {
            for at in [[12.0, -7.0], [33.0, 41.0], [-3.0, 18.0]] {
                let (offset, shift) = label_through(place, kind, at);
                let back = label_point(place, kind, offset, shift);
                assert!(distance(back, at) < 1e-9, "{kind:?}: {at:?} -> {back:?}");
            }
        }
        // No shift: the middle of the dimension line, as before labels could move.
        assert_eq!(label_point(DimPlace::Linear([0.0, 0.0], [40.0, 0.0]), Kind::DimHorizontal, 5.0, 0.0), [20.0, 5.0]);
    }
}
