//! Chains: separate open paths that touch end to end (e.g. lines drawn one after the other with
//! LINE) are one figure for OFFSET, FILLET and CHAMFER, as if they were a single polyline.

use super::geom::{Contour, Vertex, distance};
use super::{Document, Item, Shape};

/// Ends closer than this (mm) count as connected.
const TOUCH: f64 = 1e-6;

/// The single open contour of a path item, if that is what it is.
fn open_contour(item: &Item) -> Option<&Contour> {
    match item.shape.contours() {
        [c] if !c.closed && c.vertices.len() >= 2 => Some(c),
        _ => None,
    }
}

fn ends(c: &Contour) -> [[f64; 2]; 2] {
    let (a, b) = (c.vertices[0], c.vertices[c.vertices.len() - 1]);
    [[a.x, a.y], [b.x, b.y]]
}

/// The same contour walked the other way (arcs keep their shape: bulges move and flip sign).
pub fn reversed(c: &Contour) -> Contour {
    let n = c.vertices.len();
    let vertices = (0..n)
        .map(|k| {
            let v = c.vertices[n - 1 - k];
            let bulge = if k + 1 < n { -c.vertices[n - 2 - k].bulge } else { 0.0 };
            Vertex::new(v.x, v.y, bulge)
        })
        .collect();
    Contour { vertices, closed: c.closed }
}

impl Document {
    /// The ids of all open paths connected end to end with `id`, in order along the chain.
    /// Just `[id]` when it is not part of a chain.
    pub fn chain(&self, id: u64) -> Vec<u64> {
        let Some(start) = self.item(id).filter(|i| open_contour(i).is_some()) else { return vec![id] };
        let mut chain = vec![start.id];
        // Grow at the tail, then at the head, until no unused item touches the free end.
        for at_tail in [true, false] {
            loop {
                let end = {
                    let id = if at_tail { chain[chain.len() - 1] } else { chain[0] };
                    let c = open_contour(self.item(id).unwrap()).unwrap();
                    // The free end of the end item: the one not shared with its neighbour.
                    let [a, b] = ends(c);
                    let neighbour = if chain.len() > 1 { Some(if at_tail { chain[chain.len() - 2] } else { chain[1] }) } else { None };
                    let shared = |p: [f64; 2]| neighbour.and_then(|n| open_contour(self.item(n).unwrap())).is_some_and(|nc| ends(nc).iter().any(|q| distance(*q, p) < TOUCH));
                    match (shared(a), shared(b), at_tail) {
                        (true, _, _) => b,
                        (_, true, _) => a,
                        (_, _, true) => b,
                        _ => a,
                    }
                };
                let next = self.s.items.iter().find(|i| !chain.contains(&i.id) && open_contour(i).is_some_and(|c| ends(c).iter().any(|q| distance(*q, end) < TOUCH)));
                match next {
                    Some(i) if at_tail => chain.push(i.id),
                    Some(i) => chain.insert(0, i.id),
                    None => break,
                }
            }
        }
        chain
    }

    /// The chain as one contour, closed when its ends meet. `ids` must be in chain order.
    pub fn chain_contour(&self, ids: &[u64]) -> Option<Contour> {
        let parts: Vec<Contour> = ids.iter().map(|id| self.item(*id).and_then(open_contour).cloned()).collect::<Option<_>>()?;
        let mut joined = parts.first()?.clone();
        // Point the first piece towards the second.
        if let Some(next) = parts.get(1)
            && !ends(next).iter().any(|q| distance(*q, ends(&joined)[1]) < TOUCH)
        {
            joined = reversed(&joined);
        }
        for part in &parts[1..] {
            let tail = ends(&joined)[1];
            let [a, b] = ends(part);
            let part = if distance(a, tail) < TOUCH {
                part.clone()
            } else if distance(b, tail) < TOUCH {
                reversed(part)
            } else {
                return None;
            };
            // The tail vertex and the part's first vertex are the same point.
            joined.vertices.pop();
            joined.vertices.extend(part.vertices);
        }
        let [a, b] = ends(&joined);
        if distance(a, b) < TOUCH && joined.vertices.len() > 2 {
            joined.vertices.pop();
            joined.closed = true;
        }
        super::ops::merge_arcs(&mut joined);
        Some(joined)
    }

    /// Merge every chain among `ids` (and the items they connect to) into one path each, keeping
    /// the first item's name and op. Returns the ids that remain for the given items.
    pub fn join_chains(&mut self, ids: &[u64]) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        for &id in ids {
            if out.contains(&id) || self.item(id).is_none() {
                continue;
            }
            let chain = self.chain(id);
            if chain.len() < 2 {
                out.push(id);
                continue;
            }
            let Some(contour) = self.chain_contour(&chain) else {
                out.push(id);
                continue;
            };
            let keep = chain[0];
            self.item_mut(keep).unwrap().shape = Shape::path(contour);
            self.delete(&chain[1..]);
            out.push(keep);
        }
        out
    }

    /// JOIN: merge the given open paths that touch end to end into as few paths as possible (each
    /// keeps the first item's name and op). Unlike `join_chains` only these items take part.
    /// Returns the ids that remain.
    pub fn join(&mut self, ids: &[u64]) -> Result<Vec<u64>, String> {
        let mut pool: Vec<u64> = ids.iter().copied().filter(|id| self.item(*id).and_then(open_contour).is_some()).collect();
        let mut joined_any = false;
        'again: loop {
            for i in 0..pool.len() {
                for j in 0..pool.len() {
                    let (a, b) = (pool[i], pool[j]);
                    if i == j {
                        continue;
                    }
                    let touch = |x: u64, y: u64| {
                        let (cx, cy) = (open_contour(self.item(x).unwrap()).unwrap(), open_contour(self.item(y).unwrap()).unwrap());
                        ends(cx).iter().any(|p| ends(cy).iter().any(|q| distance(*p, *q) < TOUCH))
                    };
                    if !touch(a, b) {
                        continue;
                    }
                    let Some(c) = self.chain_contour(&[a, b]) else { continue };
                    let closed = c.closed;
                    self.item_mut(a).unwrap().shape = Shape::path(c);
                    self.delete(&[b]);
                    pool.retain(|id| *id != b && (!closed || *id != a));
                    joined_any = true;
                    continue 'again;
                }
            }
            break;
        }
        if !joined_any {
            return Err("none of these paths touch end to end, so there is nothing to join".into());
        }
        let left: Vec<u64> = ids.iter().copied().filter(|id| self.item(*id).is_some()).collect();
        self.selection = left.clone();
        Ok(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Op, Shape};

    fn line(d: &mut Document, a: [f64; 2], b: [f64; 2]) -> u64 {
        d.add("line", Op::Cut, Shape::path(Contour::line(a, b)))
    }

    #[test]
    fn finds_and_joins_a_chain_in_any_direction() {
        let mut d = Document::new(100.0, 100.0);
        let a = line(&mut d, [0.0, 0.0], [10.0, 0.0]);
        let b = line(&mut d, [10.0, 10.0], [10.0, 0.0]); // drawn backwards
        let c = line(&mut d, [10.0, 10.0], [0.0, 10.0]);
        let lone = line(&mut d, [50.0, 50.0], [60.0, 50.0]);
        let chain = d.chain(b);
        assert_eq!(chain.len(), 3);
        assert!(chain.contains(&a) && chain.contains(&c));
        assert_eq!(d.chain(lone), vec![lone]);
        let joined = d.chain_contour(&chain).unwrap();
        assert_eq!(joined.vertices.len(), 4);
        assert!(!joined.closed);
        let ids = d.join_chains(&[b]);
        assert_eq!(ids.len(), 1);
        assert_eq!(d.s.items.len(), 2);
    }

    #[test]
    fn a_closed_loop_becomes_a_closed_contour() {
        let mut d = Document::new(100.0, 100.0);
        let a = line(&mut d, [0.0, 0.0], [10.0, 0.0]);
        line(&mut d, [10.0, 0.0], [10.0, 10.0]);
        line(&mut d, [0.0, 10.0], [10.0, 10.0]);
        line(&mut d, [0.0, 10.0], [0.0, 0.0]);
        let c = d.chain_contour(&d.chain(a)).unwrap();
        assert!(c.closed);
        assert_eq!(c.vertices.len(), 4);
        assert!((c.area() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn offset_fillet_and_chamfer_see_one_figure() {
        let mut d = Document::new(100.0, 100.0);
        let a = line(&mut d, [0.0, 0.0], [20.0, 0.0]);
        let b = line(&mut d, [20.0, 20.0], [20.0, 0.0]);
        line(&mut d, [20.0, 20.0], [40.0, 20.0]);
        // One offset path for the whole chain, the lines stay.
        let new = d.offset(&[b], 2.0).unwrap();
        assert_eq!(new.len(), 1);
        assert_eq!(d.s.items.len(), 4);
        d.delete(&new);
        // A fillet at the corner between a and b joins the chain into one path with an arc.
        d.fillet(&[a], 5.0, Some([20.0, 0.0])).unwrap();
        assert_eq!(d.s.items.len(), 1);
        let c = &d.s.items[0].shape.contours()[0];
        assert!(c.vertices.iter().any(|v| v.bulge != 0.0));
        // A failing op leaves everything as it was.
        let before = d.s.items.clone();
        assert!(d.chamfer(&[d.s.items[0].id], 500.0, 500.0, Some([20.0, 20.0])).is_err());
        assert_eq!(d.s.items, before);
    }

    #[test]
    fn reversing_keeps_arcs() {
        let arc = Contour::arc(0.0, 0.0, 10.0, 0.0, 90.0);
        let r = reversed(&arc);
        assert!((r.length() - arc.length()).abs() < 1e-9);
        let (p, q) = (arc.flatten(0.01), r.flatten(0.01));
        assert!(p.iter().zip(q.iter().rev()).all(|(a, b)| distance(*a, *b) < 1e-6));
    }
}
