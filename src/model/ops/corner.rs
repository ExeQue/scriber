//! Corner editing shared by fillet and chamfer: a sharp corner between two straight segments is
//! replaced by a new segment (an arc for fillet, a line for chamfer).

use super::EPS;
use crate::model::chain::reversed;
use crate::model::geom::{Contour, Point, Vertex, distance};

/// A sharp corner between two straight segments.
pub(super) struct Corner {
    /// Angle between the two segments, in radians (small = sharp, π would be straight).
    pub angle: f64,
    /// +1 or -1 depending on which way the path turns; the sign of a fillet arc's bulge.
    pub turn: f64,
}

/// How a corner is cut: how far back along the incoming segment, how far ahead along the outgoing one,
/// and the bulge of the segment that replaces the corner.
pub(super) struct Cut {
    pub back: f64,
    pub ahead: f64,
    pub bulge: f64,
}

fn pt(v: &Vertex) -> Point {
    [v.x, v.y]
}

/// The corner at vertex `i`, if it is a sharp corner between two straight segments.
fn corner_at(c: &Contour, i: usize) -> Option<Corner> {
    let v = &c.vertices;
    let n = v.len();
    if n < 3 || (!c.closed && (i == 0 || i >= n - 1)) {
        return None;
    }
    let (prev, next) = (v[(i + n - 1) % n], v[(i + 1) % n]);
    if prev.bulge.abs() > EPS || v[i].bulge.abs() > EPS {
        return None;
    }
    let d1 = [v[i].x - prev.x, v[i].y - prev.y];
    let d2 = [next.x - v[i].x, next.y - v[i].y];
    let (l1, l2) = (d1[0].hypot(d1[1]), d2[0].hypot(d2[1]));
    let cross = (d1[0] * d2[1] - d1[1] * d2[0]) / (l1 * l2);
    if l1 < EPS || l2 < EPS || cross.abs() < 1e-9 {
        return None;
    }
    let dot = (d1[0] * d2[0] + d1[1] * d2[1]) / (l1 * l2);
    Some(Corner { angle: (-dot).clamp(-1.0, 1.0).acos(), turn: cross.signum() })
}

/// Cut the corner at vertex `which`, or every sharp corner when `which` is None (contours without
/// sharp corners come back unchanged). Fails when a cut needs more of a side than there is.
pub(super) fn cut_corners(c: &Contour, which: Option<usize>, cut: impl Fn(&Corner) -> Cut) -> Result<Contour, String> {
    let v = &c.vertices;
    let n = v.len();
    let cuts: Vec<Option<Cut>> = match which {
        Some(i) => {
            let at = v.get(i).map(pt).ok_or(format!("the contour has no vertex {i}"))?;
            let k = corner_at(c, i).ok_or(format!("({:.3}, {:.3}) is not a sharp corner between two straight segments", at[0], at[1]))?;
            (0..n).map(|j| (j == i).then(|| cut(&k))).collect()
        }
        None => (0..n).map(|j| corner_at(c, j).map(|k| cut(&k))).collect(),
    };
    // Each side must hold the cuts at both of its ends.
    for (i, k) in cuts.iter().enumerate() {
        let Some(k) = k else { continue };
        let (prev, next) = ((i + n - 1) % n, (i + 1) % n);
        let sides = [(k.back, prev, cuts[prev].as_ref().map_or(0.0, |p| p.ahead)), (k.ahead, next, cuts[next].as_ref().map_or(0.0, |p| p.back))];
        for (need, other, theirs) in sides {
            let len = distance(pt(&v[i]), pt(&v[other]));
            if need + theirs > len + 1e-9 {
                let shared = if theirs > 0.0 { format!(" (plus {theirs:.3} mm for the next corner)") } else { String::new() };
                return Err(format!(
                    "the corner at ({:.3}, {:.3}) needs {need:.3} mm of a side that is only {len:.3} mm long{shared}",
                    v[i].x, v[i].y
                ));
            }
        }
    }
    let mut out: Vec<Vertex> = Vec::with_capacity(n * 2);
    let mut push = |p: Vertex| match out.last_mut() {
        // A cut that uses a whole side lands on the next point: keep one vertex there.
        Some(last) if distance(pt(last), pt(&p)) < EPS => *last = p,
        _ => out.push(p),
    };
    for (i, k) in cuts.iter().enumerate() {
        let Some(k) = k else {
            push(v[i]);
            continue;
        };
        let along = |j: usize, d: f64| {
            let len = distance(pt(&v[i]), pt(&v[j]));
            [v[i].x + (v[j].x - v[i].x) * d / len, v[i].y + (v[j].y - v[i].y) * d / len]
        };
        let (a, b) = (along((i + n - 1) % n, k.back), along((i + 1) % n, k.ahead));
        push(Vertex::new(a[0], a[1], k.bulge));
        push(Vertex::new(b[0], b[1], v[i].bulge));
    }
    if c.closed && out.len() > 1 && distance(pt(&out[0]), pt(out.last().unwrap())) < EPS {
        out.pop();
    }
    Ok(Contour { vertices: out, closed: c.closed })
}

/// Join two open contours where their nearest ends meet: both end segments must be straight and are
/// extended or shortened to the crossing of their lines. Returns the joined contour and its corner vertex.
pub fn join_ends(a: &Contour, b: &Contour) -> Result<(Contour, usize), String> {
    if a.closed || b.closed || a.vertices.len() < 2 || b.vertices.len() < 2 {
        return Err("joining needs two open contours".into());
    }
    // Pick the pair of ends closest to each other; `a` must end there and `b` start there.
    let ends = |c: &Contour| [pt(&c.vertices[0]), pt(c.vertices.last().unwrap())];
    let (ea, eb) = (ends(a), ends(b));
    let (ia, ib) = [(0, 0), (0, 1), (1, 0), (1, 1)]
        .into_iter()
        .min_by(|x, y| distance(ea[x.0], eb[x.1]).total_cmp(&distance(ea[y.0], eb[y.1])))
        .unwrap();
    let a = if ia == 0 { reversed(a) } else { a.clone() };
    let b = if ib == 1 { reversed(b) } else { b.clone() };
    let n = a.vertices.len();
    let (far_a, end_a) = (a.vertices[n - 2], a.vertices[n - 1]);
    let (end_b, far_b) = (b.vertices[0], b.vertices[1]);
    if far_a.bulge.abs() > EPS || end_b.bulge.abs() > EPS {
        return Err("the ends to join must be straight segments".into());
    }
    let d1 = [end_a.x - far_a.x, end_a.y - far_a.y];
    let d2 = [end_b.x - far_b.x, end_b.y - far_b.y];
    let cross = d1[0] * d2[1] - d1[1] * d2[0];
    if cross.abs() < 1e-9 * d1[0].hypot(d1[1]) * d2[0].hypot(d2[1]) {
        return Err("the two ends are parallel, so they never meet".into());
    }
    let w = [far_b.x - far_a.x, far_b.y - far_a.y];
    let t = (w[0] * d2[1] - w[1] * d2[0]) / cross;
    let s = (w[0] * d1[1] - w[1] * d1[0]) / cross;
    if t <= EPS || s <= EPS {
        return Err("the ends meet behind one of the segments; pick ends that point towards each other".into());
    }
    let p = Vertex::new(far_a.x + t * d1[0], far_a.y + t * d1[1], 0.0);
    let mut vertices = a.vertices[..n - 1].to_vec();
    vertices.push(p);
    vertices.extend_from_slice(&b.vertices[1..]);
    Ok((Contour { vertices, closed: false }, n - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_extends_two_lines_to_their_crossing() {
        // A horizontal line ending short of x = 10 and a vertical one starting below y = 0, drawn the "wrong" way round.
        let a = Contour::line([0.0, 0.0], [8.0, 0.0]);
        let b = Contour::line([10.0, 10.0], [10.0, 3.0]);
        let (j, corner) = join_ends(&a, &b).unwrap();
        assert_eq!(corner, 1);
        let pts: Vec<Point> = j.vertices.iter().map(pt).collect();
        assert_eq!(pts, vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]);
        assert!(join_ends(&a, &Contour::line([0.0, 5.0], [8.0, 5.0])).unwrap_err().contains("parallel"));
    }
}
