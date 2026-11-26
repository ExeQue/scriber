//! The document and every command that changes it. The GUI and the MCP server both go through here,
//! so they can do the same things and share one undo history.

mod file;
pub mod geom;
pub mod item;

pub use geom::{BBox, Contour, Point, Transform};
pub use item::{Item, Op, Shape};

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
}

impl Default for State {
    fn default() -> Self {
        Self { width: 300.0, height: 200.0, notes: String::new(), items: Vec::new(), images: BTreeMap::new(), next_id: 1 }
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
}

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
}
