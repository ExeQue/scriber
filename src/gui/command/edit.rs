//! The editing commands: ERASE, MOVE, COPY, ROTATE, SCALE and MIRROR. Each works on the
//! selection (asking for one when nothing is selected) and is one undo step.

use super::parse::{Entry, is_option};
use super::{App, Cmd, Step};
use crate::gui::tools::Tool;
use crate::model::geom::distance;
use crate::model::{Document, Point, Transform};

impl App {
    pub(super) fn start_edit(&mut self, cmd: Cmd) {
        self.begin(cmd, Step::Select);
        if !self.doc.lock().unwrap().selection.is_empty() {
            self.selected(cmd);
        }
    }

    /// The objects are chosen: ERASE is done, the others ask for points.
    fn selected(&mut self, cmd: Cmd) {
        if cmd == Cmd::Erase {
            self.edit(|d, ids| d.delete(ids));
            return self.end();
        }
        self.set_step(Step::Base);
        self.tool = Tool::Pick;
        self.draft.clear();
    }

    pub(super) fn edit_step(&mut self, cmd: Cmd, step: Step, entry: Entry, pointer: Option<Point>) -> Result<(), String> {
        if step == Step::Select {
            if entry != Entry::Empty {
                return Err("Click the objects (or drag a box around them), then press Enter.".into());
            }
            if self.doc.lock().unwrap().selection.is_empty() {
                self.end();
                return Err("Nothing selected.".into());
            }
            self.selected(cmd);
            return Ok(());
        }
        if step == Step::EraseSource {
            let erase = match &entry {
                Entry::Empty => false,
                Entry::Word(w) if is_option(w, "Yes") => true,
                Entry::Word(w) if is_option(w, "No") => false,
                _ => return Err("Answer Yes or No.".into()),
            };
            self.mirror(erase);
            self.end();
            return Ok(());
        }
        if entry == Entry::Empty {
            self.end();
            return Ok(());
        }
        if step == Step::Base {
            let p = self.point_of(&entry, pointer).ok_or("Specify a point.")?;
            self.cmdline.last_point = Some(p);
            self.draft = vec![p];
            self.set_step(Step::Second);
            return Ok(());
        }
        let base = self.draft[0];
        match (cmd, &entry) {
            (Cmd::Rotate, _) => {
                // Typed angles and angles to a point are counter-clockwise on screen; the model turns clockwise.
                let angle = match entry {
                    Entry::Number(a) => a,
                    Entry::Point(p) => (base[1] - p[1]).atan2(p[0] - base[0]).to_degrees(),
                    _ => return Err("Enter an angle or click a point.".into()),
                };
                self.edit(|d, ids| d.transform(ids, &Transform { rotate: -angle, pivot: Some(base), ..Default::default() }));
                self.end();
            }
            (Cmd::Scale, _) => {
                let f = match entry {
                    Entry::Number(f) => f,
                    Entry::Point(p) => distance(base, p),
                    _ => 0.0,
                };
                if f <= 0.0 {
                    return Err("Enter a scale factor above 0.".into());
                }
                self.edit(|d, ids| d.transform(ids, &Transform { scale_x: f, scale_y: f, pivot: Some(base), ..Default::default() }));
                self.end();
            }
            _ => {
                let p = self.point_of(&entry, pointer).ok_or("Specify a point.")?;
                self.cmdline.last_point = Some(p);
                let (dx, dy) = (p[0] - base[0], p[1] - base[1]);
                match cmd {
                    Cmd::Mirror if dx == 0.0 && dy == 0.0 => return Err("The mirror line needs two different points.".into()),
                    Cmd::Mirror => {
                        self.draft.push(p);
                        self.set_step(Step::EraseSource);
                    }
                    // COPY keeps placing copies until Enter.
                    Cmd::Copy => self.edit(|d, ids| _ = d.duplicate(ids, dx, dy)),
                    _ => {
                        self.edit(|d, ids| d.transform(ids, &Transform::translate(dx, dy)));
                        self.end();
                    }
                }
            }
        }
        Ok(())
    }

    /// Mirror the selection over the line through the two placed points, or a copy of it.
    fn mirror(&mut self, erase_source: bool) {
        let [a, b] = [self.draft[0], self.draft[1]];
        let t = mirror_over(a, b);
        self.edit(|d, ids| {
            let ids = if erase_source { ids.to_vec() } else { d.duplicate(ids, 0.0, 0.0) };
            d.transform(&ids, &t);
        });
    }

    /// Apply an edit to the selection. The first edit of a command makes its undo step.
    fn edit(&mut self, f: impl FnOnce(&mut Document, &[u64])) {
        let mut d = self.doc.lock().unwrap();
        let ids = d.selection.clone();
        if !self.cmdline.checkpointed {
            d.checkpoint();
            self.cmdline.checkpointed = true;
        }
        f(&mut d, &ids);
    }
}

/// The reflection over the line through `a` and `b`: flip top-bottom, then turn by twice the line's angle.
fn mirror_over(a: Point, b: Point) -> Transform {
    let angle = (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees();
    Transform { mirror_y: true, rotate: 2.0 * angle, pivot: Some(a), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_reflects_over_the_line() {
        let close = |p: Point, q: Point| (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9;
        // Diagonal through the origin swaps x and y.
        let t = mirror_over([0.0, 0.0], [1.0, 1.0]);
        assert!(close(t.apply([3.0, 1.0], [0.0, 0.0]), [1.0, 3.0]));
        // Vertical line x = 5.
        let t = mirror_over([5.0, 0.0], [5.0, 10.0]);
        assert!(close(t.apply([2.0, 4.0], [5.0, 0.0]), [8.0, 4.0]));
    }
}
