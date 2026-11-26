//! The document and every command that changes it. The GUI and the MCP server both go through here,
//! so they can do the same things and share one undo history.

mod chain;
pub mod constraints;
mod file;
pub mod geom;
pub mod item;
pub mod ops;

pub use geom::{BBox, Contour, Point, Transform};
pub use item::{Item, Op, Shape};
pub use constraints::{Constraint, Kind, Ref};
pub use ops::BoolOp;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const UNDO_LIMIT: usize = 200;

/// The document as shared between the GUI and MCP connections.
pub type Shared = Arc<Mutex<Document>>;

/// A greyscale image stored as PNG bytes.
#[derive(Debug)]
pub struct Img {
    pub png: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

/// Everything undo restores.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub notes: String,
    pub items: Vec<Item>,
    /// Stored as separate files in the .scriber archive; Arc keeps undo snapshots cheap.
    #[serde(skip)]
    pub images: BTreeMap<u64, Arc<Img>>,
    pub next_id: u64,
    /// Geometric and dimensional constraints (see `constraints`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Constraint>,
}

impl Default for State {
    fn default() -> Self {
        Self { width: 300.0, height: 200.0, notes: String::new(), items: Vec::new(), images: BTreeMap::new(), next_id: 1, constraints: Vec::new() }
    }
}

#[derive(Default)]
pub struct Document {
    pub s: State,
    undo: Vec<State>,
    redo: Vec<State>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub selection: Vec<u64>,
    /// Bumped on every change, so views know when to refresh.
    pub rev: u64,
    /// The last state the constraint solver settled (see `settle`).
    solved: Option<State>,
    /// References the next `settle` should keep in place (the first object of a new constraint).
    hold: Vec<Ref>,
}

impl Document {
    pub fn new(width: f64, height: f64) -> Self {
        Self { s: State { width, height, ..State::default() }, ..Self::default() }
    }

    // ---------- history ----------

    /// Call before every change: saves an undo step.
    pub fn checkpoint(&mut self) {
        self.push_undo(self.s.clone());
    }

    /// Save `prev` as the undo step for a change that has already happened.
    pub fn push_undo(&mut self, prev: State) {
        self.undo.push(prev);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.touch();
    }

    /// Mark a change without a new undo step (e.g. while dragging).
    pub fn touch(&mut self) {
        self.dirty = true;
        self.rev += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.s, prev));
        self.after_history();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.s, next));
        self.after_history();
        true
    }

    fn after_history(&mut self) {
        let ids: Vec<u64> = self.s.items.iter().map(|i| i.id).collect();
        self.selection.retain(|id| ids.contains(id));
        self.touch();
    }

    // ---------- lookup ----------

    pub fn item(&self, id: u64) -> Option<&Item> {
        self.s.items.iter().find(|i| i.id == id)
    }

    pub fn item_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.s.items.iter_mut().find(|i| i.id == id)
    }

    /// Check that every id exists.
    pub fn resolve(&self, ids: &[u64]) -> Result<Vec<u64>, String> {
        match ids.iter().find(|id| self.item(**id).is_none()) {
            Some(bad) => Err(format!("no item with id {bad}")),
            None => Ok(ids.to_vec()),
        }
    }

    /// Union of the items' bounding boxes.
    pub fn bbox(&self, ids: &[u64]) -> Option<BBox> {
        let boxes: Vec<BBox> = self.s.items.iter().filter(|i| ids.contains(&i.id)).map(|i| i.shape.metrics().bbox).collect();
        geom::union(&boxes)
    }

    // ---------- commands (callers checkpoint first) ----------

    pub fn add(&mut self, name: &str, op: Op, fill: bool, shape: Shape) -> u64 {
        let id = self.new_id();
        self.s.items.push(Item { id, name: name.into(), op, fill, shape, note: String::new() });
        self.touch();
        id
    }

    fn new_id(&mut self) -> u64 {
        let id = self.s.next_id;
        self.s.next_id += 1;
        id
    }

    pub fn delete(&mut self, ids: &[u64]) {
        self.s.items.retain(|i| !ids.contains(&i.id));
        let values = constraints::parameter_values(&self.s).unwrap_or_default();
        self.s.constraints.retain(|c| c.refs.iter().all(|r| !ids.contains(&r.item())));
        constraints::freeze_orphans(&mut self.s, &values);
        self.selection.retain(|id| !ids.contains(id));
        // Drop images nothing points at any more (undo snapshots keep their own reference).
        let used: Vec<u64> = self.s.items.iter().filter_map(|i| if let Shape::Image { image, .. } = i.shape { Some(image) } else { None }).collect();
        self.s.images.retain(|id, _| used.contains(id));
        self.touch();
    }

    pub fn duplicate(&mut self, ids: &[u64], dx: f64, dy: f64) -> Vec<u64> {
        let copies: Vec<Item> = self.s.items.iter().filter(|i| ids.contains(&i.id)).cloned().collect();
        copies
            .into_iter()
            .map(|mut item| {
                item.id = self.new_id();
                item.shape = item.shape.transformed(&Transform::translate(dx, dy), [0.0, 0.0]);
                let id = item.id;
                self.s.items.push(item);
                id
            })
            .collect()
    }

    pub fn transform(&mut self, ids: &[u64], t: &Transform) {
        let pivot = t.pivot.unwrap_or_else(|| {
            let [x, y, w, h] = self.bbox(ids).unwrap_or([0.0; 4]);
            [x + w / 2.0, y + h / 2.0]
        });
        for item in self.s.items.iter_mut().filter(|i| ids.contains(&i.id)) {
            item.shape = item.shape.transformed(t, pivot);
        }
        self.touch();
    }

    /// Merge paths into one compound path (keeps the first item's name, op and fill). Returns its id.
    pub fn combine(&mut self, ids: &[u64]) -> Result<u64, String> {
        let paths: Vec<&Item> = self.s.items.iter().filter(|i| ids.contains(&i.id)).collect();
        if paths.len() < 2 || paths.iter().any(|i| !matches!(i.shape, Shape::Path { .. })) {
            return Err("combine needs two or more paths (not text, images or dimensions)".into());
        }
        let first = paths[0].clone();
        let contours = paths.iter().flat_map(|i| i.shape.contours().to_vec()).collect();
        let keep = first.id;
        let rest: Vec<u64> = ids.iter().copied().filter(|id| *id != keep).collect();
        self.item_mut(keep).unwrap().shape = Shape::Path { contours };
        self.delete(&rest);
        self.selection = vec![keep];
        Ok(keep)
    }

    /// Split a compound path into one item per contour. Returns the new ids.
    pub fn break_apart(&mut self, id: u64) -> Result<Vec<u64>, String> {
        let item = self.item(id).ok_or(format!("no item with id {id}"))?.clone();
        let contours = item.shape.contours();
        if contours.len() < 2 {
            return Err("this item has only one contour".into());
        }
        let mut new = Vec::new();
        for c in contours {
            let nid = self.add(&item.name, item.op, item.fill, Shape::path(c.clone()));
            new.push(nid);
        }
        self.delete(&[id]);
        self.selection = new.clone();
        Ok(new)
    }

    /// Store a greyscale image and place it `width` mm wide (default 10 px/mm, capped to the material).
    pub fn place_image(&mut self, img: Img, at: Point, width: Option<f64>, name: &str) -> u64 {
        let aspect = img.h as f64 / img.w.max(1) as f64;
        let max_w = (self.s.width - at[0]).max(10.0).min((self.s.height - at[1]).max(10.0) / aspect);
        let w = width.unwrap_or_else(|| (img.w as f64 / 10.0).min(max_w));
        let image = self.new_id();
        self.s.images.insert(image, Arc::new(img));
        self.add(name, Op::Engrave, true, Shape::Image { image, x: at[0], y: at[1], w, h: w * aspect, rotation: 0.0 })
    }

    // ---------- CAD operations (callers checkpoint first; on an error nothing changes) ----------

    /// Copies of the items, in the order of `ids` (repeats skipped), all of which must be paths.
    fn paths(&self, ids: &[u64]) -> Result<Vec<Item>, String> {
        ids.iter()
            .enumerate()
            .filter(|(n, id)| !ids[..*n].contains(id))
            .map(|(_, id)| id)
            .map(|id| match self.item(*id) {
                Some(i) if matches!(i.shape, Shape::Path { .. }) => Ok(i.clone()),
                Some(_) => Err(format!("item {id} is not a path (text, images and dimensions cannot be edited this way)")),
                None => Err(format!("no item with id {id}")),
            })
            .collect()
    }

    /// Round corners with `radius`: the corner nearest `at`, or every sharp corner of the items.
    /// Two open items are joined where their ends meet and that corner is rounded (radius 0 just joins them).
    pub fn fillet(&mut self, ids: &[u64], radius: f64, at: Option<Point>) -> Result<(), String> {
        self.on_chains(ids, at, |d, ids| d.corners(ids, at, radius == 0.0, |c, i| ops::fillet(c, i, radius)))
    }

    /// Cut corners off, `d1` mm along the incoming side and `d2` along the outgoing one (for two
    /// joined items: along the first and the second); otherwise like `fillet`.
    pub fn chamfer(&mut self, ids: &[u64], d1: f64, d2: f64, at: Option<Point>) -> Result<(), String> {
        self.on_chains(ids, at, |d, ids| d.corners(ids, at, d1 == 0.0 && d2 == 0.0, |c, i| ops::chamfer(c, i, d1, d2)))
    }

    /// Run a corner op with touching lines joined into one figure first, all or nothing.
    /// Two picked objects without a corner point keep the two-object behaviour (they get joined there).
    fn on_chains(&mut self, ids: &[u64], at: Option<Point>, op: impl FnOnce(&mut Self, &[u64]) -> Result<(), String>) -> Result<(), String> {
        let before = self.s.clone();
        let ids = if ids.len() == 2 && at.is_none() { ids.to_vec() } else { self.join_chains(ids) };
        let result = op(self, &ids);
        if result.is_err() {
            self.s = before;
        }
        result
    }

    fn corners(&mut self, ids: &[u64], at: Option<Point>, just_join: bool, cut: impl Fn(&Contour, Option<usize>) -> Result<Contour, String>) -> Result<(), String> {
        let items = self.paths(ids)?;
        if let [a, b] = items.as_slice()
            && let ([ca], [cb]) = (a.shape.contours(), b.shape.contours())
            && !ca.closed
            && !cb.closed
        {
            let (joined, corner) = ops::join_ends(ca, cb)?;
            let joined = if just_join { joined } else { cut(&joined, Some(corner))? };
            self.item_mut(a.id).unwrap().shape = Shape::path(joined);
            self.delete(&[b.id]);
            return Ok(());
        }
        // (item index, contour index, vertex index) of the corner nearest `at`.
        let target = at.and_then(|p| {
            let corners = items.iter().enumerate().flat_map(|(ii, it)| {
                it.shape.contours().iter().enumerate().flat_map(move |(ci, c)| c.vertices.iter().enumerate().map(move |(vi, v)| ((ii, ci, vi), geom::distance(p, [v.x, v.y]))))
            });
            corners.min_by(|a, b| a.1.total_cmp(&b.1)).map(|c| c.0)
        });
        let mut shapes = Vec::new();
        for (ii, item) in items.iter().enumerate() {
            let contours: Vec<Contour> = item
                .shape
                .contours()
                .iter()
                .enumerate()
                .map(|(ci, c)| match target {
                    Some((ti, tc, tv)) if (ti, tc) == (ii, ci) => cut(c, Some(tv)),
                    Some(_) => Ok(c.clone()),
                    None => cut(c, None),
                })
                .collect::<Result<_, _>>()?;
            shapes.push(Shape::Path { contours });
        }
        if items.iter().zip(&shapes).all(|(i, s)| i.shape == *s) {
            return Err("there are no sharp corners between straight segments to change".into());
        }
        for (item, shape) in items.iter().zip(shapes) {
            self.item_mut(item.id).unwrap().shape = shape;
        }
        self.touch();
        Ok(())
    }

    /// Add offset copies of the items, `distance` mm outward (negative = inward; open contours: to
    /// the left on screen). Returns the new ids.
    pub fn offset(&mut self, ids: &[u64], distance: f64) -> Result<Vec<u64>, String> {
        if distance == 0.0 || !distance.is_finite() {
            return Err("the offset distance must not be 0".into());
        }
        let mut results = Vec::new();
        let mut done: Vec<u64> = Vec::new();
        for item in self.paths(ids)? {
            if done.contains(&item.id) {
                continue;
            }
            // Lines that touch end to end are offset as one figure.
            let chain = self.chain(item.id);
            done.extend(&chain);
            let source = match self.chain_contour(&chain) {
                Some(c) if chain.len() > 1 => vec![c],
                _ => item.shape.contours().to_vec(),
            };
            let contours = ops::offset(&source, distance);
            if contours.is_empty() {
                return Err(format!("offsetting item {} by {distance} mm leaves nothing: it is too small", item.id));
            }
            results.push((item, contours));
        }
        Ok(results.into_iter().map(|(i, contours)| self.add(&i.name, i.op, i.fill, Shape::Path { contours })).collect())
    }

    /// Union, difference (the first item minus the others) or intersection of closed paths. The result
    /// replaces the first item (keeping its name, op and fill); the others are deleted. Returns its id.
    pub fn boolean(&mut self, ids: &[u64], op: BoolOp) -> Result<u64, String> {
        let items = self.paths(ids)?;
        if items.len() < 2 {
            return Err("boolean operations need two or more paths".into());
        }
        if let Some(open) = items.iter().find(|i| i.shape.contours().iter().any(|c| !c.closed)) {
            return Err(format!("item {} is not closed; boolean operations work on closed shapes", open.id));
        }
        let inputs: Vec<Vec<Contour>> = items.iter().map(|i| i.shape.contours().to_vec()).collect();
        let contours = ops::boolean(op, &inputs);
        if contours.is_empty() {
            return Err("the result would be empty (nothing is left)".into());
        }
        let keep = items[0].id;
        self.item_mut(keep).unwrap().shape = Shape::Path { contours };
        let rest: Vec<u64> = items[1..].iter().map(|i| i.id).collect();
        self.delete(&rest);
        self.selection = vec![keep];
        Ok(keep)
    }

    /// Copy the items into a grid of `rows` × `columns`, `dx` mm between columns and `dy` between rows.
    /// Returns the ids of the copies.
    pub fn array_rectangular(&mut self, ids: &[u64], rows: u32, columns: u32, dx: f64, dy: f64) -> Result<Vec<u64>, String> {
        let cells = rows as u64 * columns as u64;
        if !(2..=MAX_COPIES).contains(&cells) {
            return Err(format!("rows × columns must be between 2 and {MAX_COPIES}"));
        }
        Ok(ops::grid_offsets(rows, columns, dx, dy).into_iter().flat_map(|[x, y]| self.duplicate(ids, x, y)).collect())
    }

    /// Copy the items around `centre`: `count` items in total (original included) spread over `angle`
    /// degrees (360 = evenly round). `rotate` turns the copies with the angle; otherwise they only move.
    pub fn array_polar(&mut self, ids: &[u64], count: u32, centre: Point, angle: f64, rotate: bool) -> Result<Vec<u64>, String> {
        if !(2..=MAX_COPIES).contains(&(count as u64)) {
            return Err(format!("count must be between 2 and {MAX_COPIES}"));
        }
        if angle == 0.0 || !angle.is_finite() {
            return Err("the angle must not be 0".into());
        }
        let [x, y, w, h] = self.bbox(ids).ok_or("there is nothing to copy")?;
        let mid = [x + w / 2.0, y + h / 2.0];
        let mut new = Vec::new();
        for a in ops::polar_angles(count, angle) {
            let copies = self.duplicate(ids, 0.0, 0.0);
            let t = if rotate {
                Transform { rotate: a, pivot: Some(centre), ..Transform::default() }
            } else {
                let p = geom::rotate_point(mid, centre, a);
                Transform::translate(p[0] - mid[0], p[1] - mid[1])
            };
            self.transform(&copies, &t);
            new.extend(copies);
        }
        Ok(new)
    }

    /// Trim the contour of item `id` nearest `at` back to the nearest crossings on either side of `at`, with
    /// the `edges` items (empty = every other path). An item left with nothing is deleted.
    pub fn trim(&mut self, id: u64, at: Point, edges: &[u64]) -> Result<(), String> {
        let item = self.paths(&[id])?.remove(0);
        let edge_ids: Vec<u64> = if edges.is_empty() {
            self.s.items.iter().filter(|i| i.id != id && matches!(i.shape, Shape::Path { .. })).map(|i| i.id).collect()
        } else {
            edges.iter().copied().filter(|e| *e != id).collect()
        };
        let edges: Vec<Contour> = self.paths(&edge_ids)?.iter().flat_map(|i| i.shape.contours().to_vec()).collect();
        let contours = ops::trim(item.shape.contours(), &edges, at)?;
        if contours.is_empty() {
            self.delete(&[id]);
        } else {
            self.item_mut(id).unwrap().shape = Shape::Path { contours };
            self.touch();
        }
        Ok(())
    }

    /// Fillet two objects, each picked at a point (AutoCAD FILLET): lines, arcs, circles and end
    /// segments of open polylines; lines touching end to end count as one polyline. The picks choose
    /// the kept sides and which of the possible arcs is meant; lines and arcs are trimmed or extended
    /// to the arc, circles stay. Radius 0 makes a sharp corner; parallel lines get a half-circle cap.
    /// Two open paths are joined into one. Returns the ids of the new and changed items.
    pub fn fillet_objects(&mut self, a: (u64, Point), b: (u64, Point), radius: f64) -> Result<Vec<u64>, String> {
        self.edit_objects(a, b, "fillet", |pa, pb| ops::fillet_objects(pa, pb, radius))
    }

    /// Chamfer two objects, each picked at a point (AutoCAD CHAMFER): lines, arcs and end segments of
    /// open polylines, `d1` mm from the corner along the first and `d2` along the second (along the
    /// arc for arcs). 0 and 0 makes a sharp corner. Otherwise like `fillet_objects`.
    pub fn chamfer_objects(&mut self, a: (u64, Point), b: (u64, Point), d1: f64, d2: f64) -> Result<Vec<u64>, String> {
        self.edit_objects(a, b, "chamfer", |pa, pb| ops::chamfer_objects(pa, pb, d1, d2))
    }

    /// Join each pick's chain into one path, apply `op` and write its result back; all or nothing.
    fn edit_objects(&mut self, a: (u64, Point), b: (u64, Point), name: &str, op: impl FnOnce(ops::Pick, ops::Pick) -> Result<ops::Edit, String>) -> Result<Vec<u64>, String> {
        self.paths(&[a.0, b.0])?;
        let before = self.s.clone();
        let ia = self.join_chains(&[a.0])[0];
        // Picks on one chain are on the same path now.
        let ib = self.join_chains(&[b.0]).first().copied().unwrap_or(ia);
        let (ta, tb) = (self.item(ia).unwrap().clone(), self.item(ib).unwrap().clone());
        let edit = if ia == ib { op((ta.shape.contours(), a.1), (ta.shape.contours(), b.1)) } else { op((ta.shape.contours(), a.1), (tb.shape.contours(), b.1)) };
        let edit = match edit {
            Ok(e) => e,
            Err(e) => {
                self.s = before;
                return Err(e);
            }
        };
        let mut ids = Vec::new();
        match edit {
            ops::Edit::Joined(c) => {
                self.item_mut(ia).unwrap().shape = Shape::path(c);
                self.delete(&[ib]);
                ids.push(ia);
            }
            ops::Edit::Trimmed { a: na, b: nb, bridge } => {
                for (id, contours) in [(ia, na), (ib, nb)] {
                    if let Some(contours) = contours {
                        self.item_mut(id).unwrap().shape = Shape::Path { contours };
                        ids.push(id);
                    }
                }
                if let Some(c) = bridge {
                    ids.push(self.add(name, ta.op, false, Shape::path(c)));
                }
            }
        }
        self.touch();
        Ok(ids)
    }
}

/// Most copies one array may make.
const MAX_COPIES: u64 = 10_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_and_transform() {
        let mut d = Document::new(100.0, 100.0);
        d.checkpoint();
        let id = d.add("box", Op::Cut, false, Shape::path(Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0)));
        d.checkpoint();
        d.transform(&[id], &Transform { dx: 5.0, rotate: 90.0, ..Default::default() });
        let b = d.bbox(&[id]).unwrap();
        assert!((b[0] - 5.0).abs() < 1e-9 && (b[2] - 10.0).abs() < 1e-9);
        assert!(d.undo());
        assert_eq!(d.bbox(&[id]).unwrap(), [0.0, 0.0, 10.0, 10.0]);
        assert!(d.undo());
        assert!(d.s.items.is_empty());
        assert!(d.redo());
        assert_eq!(d.s.items.len(), 1);
    }

    #[test]
    fn combine_and_break_apart() {
        let mut d = Document::new(100.0, 100.0);
        let a = d.add("plate", Op::Cut, true, Shape::path(Contour::rect(0.0, 0.0, 40.0, 40.0, 0.0)));
        let b = d.add("hole", Op::Score, false, Shape::path(Contour::circle(20.0, 20.0, 5.0)));
        let id = d.combine(&[a, b]).unwrap();
        assert_eq!(id, a);
        assert_eq!(d.s.items.len(), 1);
        let item = d.item(a).unwrap();
        assert!(item.fill && item.op == Op::Cut && item.shape.contours().len() == 2);
        let parts = d.break_apart(a).unwrap();
        assert_eq!(parts.len(), 2);
        assert!(d.item(a).is_none());
        assert!(d.combine(&[parts[0]]).is_err());
    }

    #[test]
    fn cad_operations_on_items() {
        let mut d = Document::new(200.0, 200.0);
        // Two lines meeting at (10, 0): filleting them joins them into one rounded path.
        let a = d.add("a", Op::Cut, false, Shape::path(Contour::line([0.0, 0.0], [8.0, 0.0])));
        let b = d.add("b", Op::Cut, false, Shape::path(Contour::line([10.0, 10.0], [10.0, 2.0])));
        d.fillet(&[a, b], 2.0, None).unwrap();
        assert!(d.item(b).is_none());
        let joined = &d.item(a).unwrap().shape.contours()[0];
        assert_eq!(joined.vertices.len(), 4);
        assert!((joined.length() - (8.0 + 8.0 + std::f64::consts::PI)).abs() < 1e-9);

        // One corner of a square, picked near it; a radius too big is refused and changes nothing.
        let sq = d.add("sq", Op::Cut, false, Shape::path(Contour::rect(50.0, 50.0, 10.0, 10.0, 0.0)));
        assert!(d.fillet(&[sq], 11.0, Some([60.0, 50.0])).unwrap_err().contains("only 10.000 mm long"));
        d.chamfer(&[sq], 2.0, 2.0, Some([59.0, 51.0])).unwrap();
        assert!((d.item(sq).unwrap().shape.metrics().area.unwrap() - 98.0).abs() < 1e-9);
        let circle = d.add("circle", Op::Cut, false, Shape::path(Contour::circle(0.0, 0.0, 5.0)));
        assert!(d.fillet(&[circle], 1.0, None).unwrap_err().contains("no sharp corners"));
        d.delete(&[circle]);

        // Boolean keeps the first item, offset and arrays add new ones.
        let hole = d.add("hole", Op::Cut, false, Shape::path(Contour::circle(55.0, 55.0, 2.0)));
        assert_eq!(d.boolean(&[sq, hole], BoolOp::Difference).unwrap(), sq);
        assert!(d.item(hole).is_none() && d.item(sq).unwrap().shape.contours().len() == 2);
        let off = d.offset(&[sq], 1.0).unwrap();
        let ob = d.bbox(&off).unwrap();
        assert!(ob.iter().zip([49.0, 49.0, 12.0, 12.0]).all(|(v, e)| (v - e).abs() < 1e-9), "{ob:?}");
        assert_eq!(d.item(off[0]).unwrap().shape.contours().len(), 2, "the hole is offset too");
        assert_eq!(d.array_rectangular(&[sq], 2, 2, 20.0, 30.0).unwrap().len(), 3);
        let ring = d.array_polar(&[sq], 4, [100.0, 55.0], 360.0, false).unwrap();
        // Copies centred 45 mm from the centre at 90°, 180° and 270° (clockwise on screen), not rotated.
        let rb = d.bbox(&ring).unwrap();
        assert!(rb.iter().zip([95.0, 5.0, 55.0, 100.0]).all(|(v, e)| (v - e).abs() < 1e-9), "{rb:?}");
        assert!(d.array_polar(&[sq], 1, [0.0, 0.0], 360.0, true).is_err());

        // Trim the middle of a line crossed by two others.
        let line = d.add("line", Op::Cut, false, Shape::path(Contour::line([0.0, 150.0], [30.0, 150.0])));
        let e1 = d.add("e1", Op::Cut, false, Shape::path(Contour::line([10.0, 140.0], [10.0, 160.0])));
        let e2 = d.add("e2", Op::Cut, false, Shape::path(Contour::line([20.0, 140.0], [20.0, 160.0])));
        d.trim(line, [15.0, 150.0], &[e1, e2]).unwrap();
        assert!((d.item(line).unwrap().shape.metrics().length - 20.0).abs() < 1e-9);
    }
}
