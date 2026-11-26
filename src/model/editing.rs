//! The AutoCAD editing commands of the UX pack: STRETCH, EXPLODE, ALIGN, MATCHPROP, BREAK, EXTEND,
//! LENGTHEN, GROUP/UNGROUP and select similar / quick select (JOIN lives with the chains in chain.rs).
//! Callers checkpoint first; on an error nothing changes.

use super::geom::{BBox, Contour, Point, Transform, distance, segment_distance};
use super::{Document, Item, Op, Shape, guides::Guide, ops};

/// Kinds of shapes, as select similar and quick select tell them apart.
pub const KINDS: [&str; 10] = ["line", "arc", "circle", "polyline", "closed shape", "compound path", "text", "dimension", "image", "guide"];

/// What kind of shape this is, in plain words (one of `KINDS`).
pub fn shape_kind(shape: &Shape) -> &'static str {
    match shape {
        Shape::Path { contours } => match contours.as_slice() {
            [c] if c.as_circle().is_some() => "circle",
            [c] if c.closed => "closed shape",
            [c] if c.vertices.len() == 2 && c.vertices[0].bulge == 0.0 => "line",
            [c] if c.vertices.len() == 2 => "arc",
            [_] | [] => "polyline",
            _ => "compound path",
        },
        Shape::Text { .. } => "text",
        Shape::Dimension { .. } => "dimension",
        Shape::Image { .. } => "image",
        Shape::Guide(_) => "guide",
    }
}

/// Index of the contour of `contours` that passes nearest `p`.
fn nearest_contour(contours: &[Contour], p: Point) -> Option<usize> {
    let dist = |c: &Contour| c.flatten(0.05).windows(2).map(|s| segment_distance(p, s[0], s[1])).fold(f64::MAX, f64::min);
    (0..contours.len()).min_by(|a, b| dist(&contours[*a]).total_cmp(&dist(&contours[*b])))
}

/// The shape with its points inside `w` moved by `d`.
fn stretched(shape: &Shape, w: BBox, d: Point) -> Shape {
    let inside = |p: Point| ops::inside(w, p);
    let moved = |p: Point| if inside(p) { [p[0] + d[0], p[1] + d[1]] } else { p };
    let whole = |p: Point| if inside(p) { shape.transformed(&Transform::translate(d[0], d[1]), [0.0, 0.0]) } else { shape.clone() };
    match shape {
        Shape::Path { contours } => Shape::Path { contours: contours.iter().map(|c| ops::stretch(c, w, d)).collect() },
        Shape::Dimension { x1, y1, x2, y2, offset } => {
            let ([x1, y1], [x2, y2]) = (moved([*x1, *y1]), moved([*x2, *y2]));
            Shape::Dimension { x1, y1, x2, y2, offset: *offset }
        }
        Shape::Guide(Guide::Centerline { x1, y1, x2, y2, of }) => {
            let ([x1, y1], [x2, y2]) = (moved([*x1, *y1]), moved([*x2, *y2]));
            Shape::Guide(Guide::Centerline { x1, y1, x2, y2, of: of.clone() })
        }
        Shape::Guide(g) => whole(g.anchor()),
        Shape::Text { x, y, .. } => whole([*x, *y]),
        Shape::Image { x, y, w: iw, h, .. } => whole([x + iw / 2.0, y + h / 2.0]),
    }
}

impl Document {
    /// STRETCH: move every point of the items (empty = all) that lies inside `window` by (dx, dy).
    /// Returns the ids of the items that changed.
    pub fn stretch(&mut self, ids: &[u64], window: BBox, dx: f64, dy: f64) -> Result<Vec<u64>, String> {
        let mut changed = Vec::new();
        for item in self.s.items.iter_mut().filter(|i| ids.is_empty() || ids.contains(&i.id)) {
            let new = stretched(&item.shape, window, [dx, dy]);
            if new != item.shape {
                item.shape = new;
                changed.push(item.id);
            }
        }
        if changed.is_empty() {
            return Err("no points of the shapes are inside the window".into());
        }
        self.touch();
        Ok(changed)
    }

    /// A new item like `base` (name, op, fill, group) with another shape, right after it in the list.
    fn spawn(&mut self, base: &Item, shape: Shape) -> u64 {
        let id = self.s.next_id;
        self.s.next_id += 1;
        let at = self.s.items.iter().position(|i| i.id == base.id).map_or(self.s.items.len(), |n| n + 1);
        self.s.items.insert(at, Item { id, shape, ..base.clone() });
        if let Some(g) = self.s.groups.iter_mut().find(|g| g.contains(&base.id)) {
            g.push(id);
        }
        id
    }

    /// EXPLODE: a compound path becomes one item per contour, a single path one item per segment and
    /// a text its letter outlines (one compound path). Returns the new ids.
    pub fn explode(&mut self, ids: &[u64]) -> Result<Vec<u64>, String> {
        let mut new = Vec::new();
        for &id in ids {
            let item = self.item(id).ok_or(format!("no item with id {id}"))?.clone();
            let shapes: Vec<Shape> = match &item.shape {
                Shape::Path { contours } if contours.len() > 1 => contours.iter().cloned().map(Shape::path).collect(),
                Shape::Path { contours } if contours.first().is_some_and(|c| c.as_circle().is_none()) => ops::segments(&contours[0]).into_iter().map(Shape::path).collect(),
                Shape::Text { .. } => vec![Shape::Path { contours: item.shape.laser_contours().into_owned() }],
                _ => continue,
            };
            if (shapes.len() < 2 && !matches!(item.shape, Shape::Text { .. })) || shapes.iter().all(|s| s.contours().is_empty()) {
                continue;
            }
            for s in shapes.into_iter().rev() {
                new.push(self.spawn(&item, s));
            }
            self.delete(&[id]);
        }
        if new.is_empty() {
            return Err("nothing to explode: pick compound paths, polylines or text (single lines, arcs and circles are already one piece)".into());
        }
        self.selection = new.clone();
        Ok(new)
    }

    /// ALIGN: move the items so `source.0` lands on `dest.0`; with second points also turn them so the
    /// line source.0→1 lies along dest.0→1, and with `scale` size them so it has the same length.
    pub fn align(&mut self, ids: &[u64], source: (Point, Option<Point>), dest: (Point, Option<Point>), scale: bool) -> Result<(), String> {
        let (s1, d1) = (source.0, dest.0);
        let mut t = Transform { dx: d1[0] - s1[0], dy: d1[1] - s1[1], pivot: Some(s1), ..Transform::default() };
        if let (Some(s2), Some(d2)) = (source.1, dest.1) {
            let (ls, ld) = (distance(s1, s2), distance(d1, d2));
            if ls < 1e-9 || ld < 1e-9 {
                return Err("the two source points (and the two destination points) must differ".into());
            }
            let ang = |a: Point, b: Point| (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees();
            t.rotate = ang(d1, d2) - ang(s1, s2);
            if scale {
                t.scale_x = ld / ls;
                t.scale_y = ld / ls;
            }
        }
        self.transform(ids, &t);
        Ok(())
    }

    /// MATCHPROP: give `targets` the laser operation of `source`, and its font when both are text.
    pub fn match_properties(&mut self, source: u64, targets: &[u64]) -> Result<(), String> {
        let src = self.item(source).ok_or(format!("no item with id {source}"))?.clone();
        for item in self.s.items.iter_mut().filter(|i| targets.contains(&i.id) && i.id != source) {
            item.op = src.op;
            if let (Shape::Text { font, bold, italic, .. }, Shape::Text { font: f, bold: b, italic: i, .. }) = (&src.shape, &mut item.shape) {
                (*f, *b, *i) = (font.clone(), *bold, *italic);
            }
        }
        self.touch();
        Ok(())
    }

    /// The path item `id`, its contours and the index of the one nearest `at`.
    fn contour_at(&self, id: u64, at: Point) -> Result<(Item, Vec<Contour>, usize), String> {
        let item = self.paths(&[id])?.remove(0);
        let contours = item.shape.contours().to_vec();
        let n = nearest_contour(&contours, at).ok_or("the path is empty")?;
        Ok((item, contours, n))
    }

    /// BREAK: cut the path at the point nearest `at` (with `at2`: take out the piece between the two).
    /// A single-contour path that falls apart becomes several items. Returns the ids of the pieces.
    pub fn break_item(&mut self, id: u64, at: Point, at2: Option<Point>) -> Result<Vec<u64>, String> {
        let (item, mut contours, n) = self.contour_at(id, at)?;
        let pieces = ops::break_at(&contours[n], at, at2)?;
        if pieces.is_empty() {
            self.delete(&[id]);
            return Ok(Vec::new());
        }
        if contours.len() > 1 {
            contours.splice(n..=n, pieces);
            self.item_mut(id).unwrap().shape = Shape::Path { contours };
            self.touch();
            return Ok(vec![id]);
        }
        let mut ids = vec![id];
        self.item_mut(id).unwrap().shape = Shape::path(pieces[0].clone());
        for p in pieces[1..].iter().rev() {
            ids.push(self.spawn(&item, Shape::path(p.clone())));
        }
        self.touch();
        Ok(ids)
    }

    /// EXTEND: grow the open end of path `id` nearest `at` to the nearest of the `boundaries` items
    /// (empty = every other path and every guide line).
    pub fn extend(&mut self, id: u64, at: Point, boundaries: &[u64]) -> Result<(), String> {
        let (_, mut contours, n) = self.contour_at(id, at)?;
        let mut edges: Vec<Contour> = contours.iter().enumerate().filter(|(i, _)| *i != n).map(|(_, c)| c.clone()).collect();
        for item in self.s.items.iter().filter(|i| i.id != id && (boundaries.is_empty() || boundaries.contains(&i.id))) {
            match &item.shape {
                Shape::Path { contours } => edges.extend(contours.iter().cloned()),
                Shape::Guide(g) => edges.extend(g.outline().iter().map(|s| Contour::line(s[0], s[1]))),
                _ => {}
            }
        }
        contours[n] = ops::extend(&contours[n], at, &edges)?;
        self.item_mut(id).unwrap().shape = Shape::Path { contours };
        self.touch();
        Ok(())
    }

    /// LENGTHEN: change the open end of path `id` nearest `at` by `delta` mm, or to a `total` length.
    pub fn lengthen(&mut self, id: u64, at: Point, delta: Option<f64>, total: Option<f64>) -> Result<(), String> {
        let (_, mut contours, n) = self.contour_at(id, at)?;
        let delta = match (delta, total) {
            (Some(d), None) => d,
            (None, Some(t)) if t > 0.0 => t - contours[n].length(),
            _ => return Err("give either a change in length (delta) or a total length above 0".into()),
        };
        contours[n] = ops::lengthen(&contours[n], at, delta)?;
        self.item_mut(id).unwrap().shape = Shape::Path { contours };
        self.touch();
        Ok(())
    }

    // ---------- groups ----------

    /// GROUP: the items become one group (leaving any group they were in). Returns the group's index.
    pub fn group(&mut self, ids: &[u64]) -> Result<usize, String> {
        let ids = self.resolve(ids)?;
        if ids.len() < 2 {
            return Err("a group needs two or more items".into());
        }
        self.leave_groups(&ids);
        self.s.groups.push(ids);
        self.touch();
        Ok(self.s.groups.len() - 1)
    }

    /// UNGROUP: dissolve every group that holds one of the items.
    pub fn ungroup(&mut self, ids: &[u64]) -> Result<(), String> {
        let before = self.s.groups.len();
        self.s.groups.retain(|g| !g.iter().any(|id| ids.contains(id)));
        if self.s.groups.len() == before {
            return Err("none of these items is in a group".into());
        }
        self.touch();
        Ok(())
    }

    /// The ids plus every other member of their groups (what clicking a grouped item selects).
    // For select.rs: a click on a grouped item selects its group (not wired in yet).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn expand_groups(&self, ids: &[u64]) -> Vec<u64> {
        let mut out = ids.to_vec();
        for g in self.s.groups.iter().filter(|g| g.iter().any(|id| ids.contains(id))) {
            out.extend(g.iter().filter(|id| !ids.contains(id)));
        }
        out
    }

    /// Take the items out of their groups; groups left with fewer than two items go.
    pub(super) fn leave_groups(&mut self, ids: &[u64]) {
        for g in &mut self.s.groups {
            g.retain(|id| !ids.contains(id));
        }
        self.s.groups.retain(|g| g.len() >= 2);
    }

    /// After copying (old id, new id) pairs: the copies of a group's members form a group of their own.
    pub(super) fn copy_groups(&mut self, pairs: &[(u64, u64)]) {
        let copies: Vec<Vec<u64>> = self
            .s
            .groups
            .iter()
            .map(|g| pairs.iter().filter(|(old, _)| g.contains(old)).map(|(_, new)| *new).collect::<Vec<u64>>())
            .filter(|g| g.len() >= 2)
            .collect();
        self.s.groups.extend(copies);
    }

    // ---------- selecting by properties ----------

    /// Select similar: every item with the same laser operation and kind of shape as one of `ids`.
    pub fn similar(&self, ids: &[u64]) -> Vec<u64> {
        let keys: Vec<(Op, &str)> = self.s.items.iter().filter(|i| ids.contains(&i.id)).map(|i| (i.op, shape_kind(&i.shape))).collect();
        self.s.items.iter().filter(|i| keys.contains(&(i.op, shape_kind(&i.shape)))).map(|i| i.id).collect()
    }

    /// Quick select: the items with this operation, kind (see `KINDS`) and size (the longer side of the
    /// bounding box, mm, inclusive); `None` = any.
    pub fn quick_select(&self, op: Option<Op>, kind: Option<&str>, min: Option<f64>, max: Option<f64>) -> Vec<u64> {
        self.s
            .items
            .iter()
            .filter(|i| {
                let b = i.shape.metrics().bbox;
                let size = b[2].max(b[3]);
                op.is_none_or(|o| i.op == o) && kind.is_none_or(|k| shape_kind(&i.shape) == k) && min.is_none_or(|m| size >= m) && max.is_none_or(|m| size <= m)
            })
            .map(|i| i.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(d: &mut Document, a: Point, b: Point) -> u64 {
        d.add("line", Op::Cut, false, Shape::path(Contour::line(a, b)))
    }

    #[test]
    fn stretch_moves_only_enclosed_points() {
        let mut d = Document::new(100.0, 100.0);
        let r = d.add("r", Op::Cut, false, Shape::path(Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0)));
        let far = d.add("far", Op::Cut, false, Shape::path(Contour::rect(50.0, 50.0, 10.0, 10.0, 0.0)));
        let dim = d.add("dim", Op::Annotate, false, Shape::Dimension { x1: 0.0, y1: 20.0, x2: 10.0, y2: 20.0, offset: 5.0 });
        let changed = d.stretch(&[], [8.0, -1.0, 5.0, 30.0], 5.0, 0.0).unwrap();
        assert_eq!(changed, vec![r, dim]);
        assert_eq!(d.bbox(&[r]).unwrap(), [0.0, 0.0, 15.0, 10.0]);
        assert_eq!(d.bbox(&[far]).unwrap(), [50.0, 50.0, 10.0, 10.0]);
        assert!(matches!(d.item(dim).unwrap().shape, Shape::Dimension { x1: 0.0, x2: 15.0, .. }));
        assert!(d.stretch(&[], [90.0, 90.0, 1.0, 1.0], 1.0, 0.0).is_err());
    }

    #[test]
    fn explode_and_join_round_trip() {
        let mut d = Document::new(100.0, 100.0);
        let r = d.add("r", Op::Score, false, Shape::path(Contour::rect(0.0, 0.0, 20.0, 10.0, 2.0)));
        let parts = d.explode(&[r]).unwrap();
        assert_eq!(parts.len(), 8);
        assert!(d.s.items.iter().all(|i| i.op == Op::Score && shape_kind(&i.shape) != "closed shape"));
        let joined = d.join(&parts).unwrap();
        assert_eq!(joined.len(), 1);
        let c = &d.item(joined[0]).unwrap().shape.contours()[0];
        assert!(c.closed && c.vertices.len() == 8);
        assert!((c.area() - Contour::rect(0.0, 0.0, 20.0, 10.0, 2.0).area()).abs() < 1e-9);
        // A compound path explodes into its contours; a lone line does not explode.
        let plate = d.add("p", Op::Cut, false, Shape::Path { contours: vec![Contour::rect(50.0, 50.0, 20.0, 20.0, 0.0), Contour::circle(60.0, 60.0, 3.0)] });
        assert_eq!(d.explode(&[plate]).unwrap().len(), 2);
        let l = line(&mut d, [0.0, 50.0], [10.0, 50.0]);
        assert!(d.explode(&[l]).is_err());
        assert!(d.join(&[l]).is_err());
    }

    #[test]
    fn align_maps_the_points() {
        let mut d = Document::new(100.0, 100.0);
        let l = line(&mut d, [0.0, 0.0], [10.0, 0.0]);
        d.align(&[l], ([0.0, 0.0], Some([10.0, 0.0])), ([50.0, 50.0], Some([50.0, 70.0])), true).unwrap();
        let v = &d.item(l).unwrap().shape.contours()[0].vertices;
        assert!(distance([v[0].x, v[0].y], [50.0, 50.0]) < 1e-9 && distance([v[1].x, v[1].y], [50.0, 70.0]) < 1e-9, "{v:?}");
        // Without scaling the length stays; with one pair it only moves.
        d.align(&[l], ([50.0, 50.0], Some([50.0, 70.0])), ([0.0, 0.0], Some([40.0, 0.0])), false).unwrap();
        let v = &d.item(l).unwrap().shape.contours()[0].vertices;
        assert!(distance([v[1].x, v[1].y], [20.0, 0.0]) < 1e-9, "{v:?}");
        d.align(&[l], ([0.0, 0.0], None), ([5.0, 5.0], None), false).unwrap();
        assert_eq!(d.bbox(&[l]).unwrap(), [5.0, 5.0, 20.0, 0.0]);
    }

    #[test]
    fn break_extend_and_lengthen_items() {
        let mut d = Document::new(100.0, 100.0);
        let l = line(&mut d, [0.0, 0.0], [30.0, 0.0]);
        let ids = d.break_item(l, [10.0, 0.0], Some([20.0, 0.0])).unwrap();
        assert_eq!(ids.len(), 2);
        assert!((d.item(ids[1]).unwrap().shape.metrics().length - 10.0).abs() < 1e-9);
        let wall = d.add("wall", Op::NoLine, false, Shape::Guide(Guide::Xline { x: 50.0, y: 0.0, angle: 90.0 }));
        d.extend(ids[1], [29.0, 0.0], &[]).unwrap();
        assert!((d.item(ids[1]).unwrap().shape.metrics().length - 30.0).abs() < 1e-9, "extended to the guide line");
        d.lengthen(ids[0], [9.0, 0.0], None, Some(4.0)).unwrap();
        assert!((d.item(ids[0]).unwrap().shape.metrics().length - 4.0).abs() < 1e-9);
        assert!(d.lengthen(wall, [50.0, 0.0], Some(1.0), None).is_err());
    }

    #[test]
    fn groups_matchprop_and_selection_by_properties() {
        let mut d = Document::new(100.0, 100.0);
        let a = line(&mut d, [0.0, 0.0], [10.0, 0.0]);
        let b = line(&mut d, [0.0, 5.0], [40.0, 5.0]);
        let c = d.add("c", Op::Score, false, Shape::path(Contour::circle(50.0, 50.0, 5.0)));
        d.group(&[a, c]).unwrap();
        assert_eq!(d.expand_groups(&[c]), vec![c, a]);
        let copies = d.duplicate(&[a, c], 1.0, 1.0);
        assert_eq!(d.s.groups.len(), 2);
        assert_eq!(d.expand_groups(&[copies[0]]).len(), 2);
        d.delete(&[a]);
        assert_eq!(d.s.groups.len(), 1, "a group of one goes");
        d.ungroup(&copies).unwrap();
        assert!(d.s.groups.is_empty());

        assert_eq!(d.similar(&[b]), vec![b, copies[0]], "open lines with the same operation");
        d.match_properties(c, &[b]).unwrap();
        assert_eq!(d.item(b).unwrap().op, Op::Score);
        assert_eq!(d.quick_select(Some(Op::Score), Some("line"), Some(20.0), None), vec![b]);
        assert_eq!(d.quick_select(None, Some("circle"), None, None).len(), 2);
    }
}
