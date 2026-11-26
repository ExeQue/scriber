//! Live previews for commands that change existing geometry: what FILLET or CHAMFER would do
//! at the corner (or with the object) under the pointer, before clicking.

use super::{App, Cmd, Step};
use crate::model::{Document, Point, Shape};

impl App {
    /// The shapes the running command would produce if the user clicked at `p` now.
    pub(in crate::gui) fn op_preview(&self, p: Point) -> Vec<Shape> {
        let Some((cmd @ (Cmd::Fillet | Cmd::Chamfer), Step::Pick)) = self.cmdline.active else { return Vec::new() };
        let d = self.doc.lock().unwrap();
        let Some(id) = self.hit(&d, p) else { return Vec::new() };
        let m = &self.cmdline.modify;
        let run = |t: &mut Document, ids: &[u64], at: Option<Point>| match cmd {
            Cmd::Fillet => t.fillet(ids, m.radius, at),
            _ => t.chamfer(ids, m.chamfer, m.chamfer, at),
        };
        // Try it on a copy, the same way a click would (see `pick_step`).
        let mut trial = Document::default();
        trial.s = d.s.clone();
        let done = match m.target {
            Some(first) if first != id => {
                let a = (first, m.target_at.unwrap_or(p));
                super::modify::two_objects(&mut trial, cmd, a, (id, p), m.radius, m.chamfer).is_ok()
            }
            _ => {
                run(&mut trial, &[id], Some(p)).is_ok()
                    || self.other_end_at(&d, id, p).is_some_and(|other| run(&mut trial, &[id, other], None).is_ok())
            }
        };
        if !done {
            return Vec::new();
        }
        trial.s.items.into_iter().filter(|t| d.item(t.id) != Some(t)).map(|t| t.shape).collect()
    }
}
