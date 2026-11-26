//! The CAD commands that reshape geometry: FILLET, CHAMFER, OFFSET, TRIM, ARRAY and the boolean
//! commands UNION, SUBTRACT and INTERSECT. The geometry itself lives in `model::ops`; this file
//! only asks for the inputs, AutoCAD style. Each command is one undo step.

use super::parse::{Entry, is_option};
use super::{App, Cmd, Step};
use crate::gui::tools::Tool;
use crate::model::geom::segment_distance;
use crate::model::{BoolOp, Document, Point, Shape};

/// What the modify commands remember between steps.
#[derive(Default)]
pub struct ModifyState {
    /// Fillet radius, chamfer distance and offset distance, kept for the next run (as AutoCAD does).
    pub radius: f64,
    pub chamfer: f64,
    pub offset: f64,
    /// The item picked for OFFSET, waiting for the side.
    target: Option<u64>,
    /// ARRAY: polar or rectangular, and the numbers given so far.
    polar: bool,
    args: Vec<f64>,
    centre: Option<Point>,
}

impl ModifyState {
    pub fn new() -> Self {
        Self { radius: 5.0, chamfer: 2.0, offset: 1.0, ..Self::default() }
    }
}

impl Cmd {
    pub(super) fn modifies(self) -> bool {
        matches!(self, Cmd::Fillet | Cmd::Chamfer | Cmd::Offset | Cmd::Trim | Cmd::Array | Cmd::Union | Cmd::Subtract | Cmd::Intersect)
    }
}

impl App {
    pub(super) fn start_modify(&mut self, cmd: Cmd) {
        let m = &mut self.cmdline.modify;
        m.target = None;
        m.args.clear();
        m.centre = None;
        m.polar = false;
        match cmd {
            Cmd::Fillet | Cmd::Chamfer | Cmd::Offset => self.begin(cmd, Step::Value),
            Cmd::Trim => self.pick(cmd),
            _ => {
                self.begin(cmd, Step::Choose);
                self.tool = Tool::Select;
                if !self.doc.lock().unwrap().selection.is_empty() {
                    self.chosen(cmd);
                }
            }
        }
    }

    fn pick(&mut self, cmd: Cmd) {
        self.begin(cmd, Step::Pick);
        self.tool = Tool::Pick;
    }

    /// The selection for ARRAY or a boolean is made.
    fn chosen(&mut self, cmd: Cmd) {
        let op = match cmd {
            Cmd::Union => BoolOp::Union,
            Cmd::Subtract => BoolOp::Difference,
            Cmd::Intersect => BoolOp::Intersection,
            _ => return self.set_step(Step::Value),
        };
        let result = self.apply(|d, ids| d.boolean(ids, op).map(|id| d.selection = vec![id]));
        self.end();
        if let Err(e) = result {
            self.cmdline.log(e);
        }
    }

    pub(super) fn modify_prompt(&self, cmd: Cmd, step: Step) -> String {
        let m = &self.cmdline.modify;
        match (cmd, step) {
            (_, Step::Choose) => "Select objects (Enter when done):".into(),
            (Cmd::Fillet, Step::Value) => format!("Specify fillet radius <{}>:", m.radius),
            (Cmd::Chamfer, Step::Value) => format!("Specify chamfer distance <{}>:", m.chamfer),
            (Cmd::Offset, Step::Value) => format!("Specify offset distance <{}>:", m.offset),
            (Cmd::Fillet | Cmd::Chamfer, _) => "Click near a corner, or [All] corners of the selection, or Enter to join two selected lines:".into(),
            (Cmd::Offset, Step::Pick) => "Select object to offset or <exit>:".into(),
            (Cmd::Offset, _) => "Specify point on side to offset:".into(),
            (Cmd::Trim, _) => "Select the part to trim off or <exit>:".into(),
            (Cmd::Array, _) => match (m.polar, m.args.len(), m.centre) {
                (false, 0, _) => "Enter array type [Rectangular/Polar] <Rectangular>:".into(),
                (true, _, None) => "Specify center point of array:".into(),
                (true, 0, _) => "Enter number of items <6>:".into(),
                (true, _, _) => "Specify the angle to fill (+ = ccw) <360>:".into(),
                (false, 1, _) => "Enter the number of rows <2>:".into(),
                (false, 2, _) => "Enter the number of columns <3>:".into(),
                (false, 3, _) => "Specify the distance between rows <10>:".into(),
                (false, _, _) => "Specify the distance between columns <10>:".into(),
            },
            _ => String::new(),
        }
    }

    pub(super) fn modify_step(&mut self, cmd: Cmd, step: Step, entry: Entry, pointer: Option<Point>) -> Result<(), String> {
        match step {
            Step::Choose => {
                if entry != Entry::Empty {
                    return Err("Click the objects (or drag a box around them), then press Enter.".into());
                }
                if self.doc.lock().unwrap().selection.is_empty() {
                    self.end();
                    return Err("Nothing selected.".into());
                }
                self.chosen(cmd);
                Ok(())
            }
            Step::Value if cmd == Cmd::Array => self.array_step(entry),
            Step::Value => {
                let m = &mut self.cmdline.modify;
                let slot = match cmd {
                    Cmd::Fillet => &mut m.radius,
                    Cmd::Chamfer => &mut m.chamfer,
                    _ => &mut m.offset,
                };
                match entry {
                    Entry::Empty => {}
                    Entry::Number(n) if n >= 0.0 && (cmd != Cmd::Offset || n > 0.0) => *slot = n,
                    _ => return Err("Enter a distance (or press Enter to keep the one shown).".into()),
                }
                self.pick(cmd);
                Ok(())
            }
            Step::Pick | Step::Side => self.pick_step(cmd, step, entry, pointer),
            _ => Ok(()),
        }
    }

    /// FILLET/CHAMFER corners, OFFSET objects and sides, TRIM parts: all picked by clicking.
    fn pick_step(&mut self, cmd: Cmd, step: Step, entry: Entry, _pointer: Option<Point>) -> Result<(), String> {
        let (radius, chamfer, offset) = (self.cmdline.modify.radius, self.cmdline.modify.chamfer, self.cmdline.modify.offset);
        let corner = |d: &mut Document, ids: &[u64], at: Option<Point>| match cmd {
            Cmd::Fillet => d.fillet(ids, radius, at),
            _ => d.chamfer(ids, chamfer, chamfer, at),
        };
        match (cmd, entry) {
            (_, Entry::Empty) => {
                // FILLET/CHAMFER with two lines selected joins them, as AutoCAD does with two picks.
                if matches!(cmd, Cmd::Fillet | Cmd::Chamfer) && self.doc.lock().unwrap().selection.len() == 2 {
                    self.apply(|d, ids| corner(d, ids, None))?;
                }
                self.end();
                Ok(())
            }
            (Cmd::Fillet | Cmd::Chamfer, Entry::Word(w)) if is_option(&w, "All") => {
                if self.doc.lock().unwrap().selection.is_empty() {
                    return Err("Select the shapes first, then use All.".into());
                }
                self.apply(|d, ids| corner(d, ids, None))?;
                self.end();
                Ok(())
            }
            (Cmd::Fillet | Cmd::Chamfer, Entry::Point(p)) => {
                let id = self.item_at(p)?;
                self.apply_to(&[id], |d, ids| corner(d, ids, Some(p)))
            }
            (Cmd::Trim, Entry::Point(p)) => {
                let id = self.item_at(p)?;
                self.apply_to(&[id], |d, ids| d.trim(ids[0], p, &[]))
            }
            (Cmd::Offset, Entry::Point(p)) if step == Step::Pick => {
                self.cmdline.modify.target = Some(self.item_at(p)?);
                self.set_step(Step::Side);
                Ok(())
            }
            (Cmd::Offset, Entry::Point(p)) => {
                let id = self.cmdline.modify.target.take().ok_or("Select an object first.")?;
                let side = self.offset_side(id, p);
                self.apply_to(&[id], |d, ids| d.offset(ids, side * offset).map(|_| ()))?;
                self.set_step(Step::Pick);
                Ok(())
            }
            _ => Err("Click on the drawing, or press Enter to finish.".into()),
        }
    }

    /// ARRAY: type, then the numbers for it, then the copies.
    fn array_step(&mut self, entry: Entry) -> Result<(), String> {
        let m = &mut self.cmdline.modify;
        let asked_type = !m.polar && m.args.is_empty();
        match (asked_type, entry) {
            (true, Entry::Word(w)) if is_option(&w, "Polar") => {
                m.polar = true;
                self.tool = Tool::Pick;
                return Ok(());
            }
            (true, Entry::Word(w)) if is_option(&w, "Rectangular") => m.args.push(f64::NAN),
            (true, Entry::Empty) => m.args.push(f64::NAN),
            (true, _) => return Err("Answer Rectangular or Polar.".into()),
            (false, Entry::Point(p)) if m.polar && m.centre.is_none() => {
                m.centre = Some(p);
                self.tool = Tool::Select;
                return Ok(());
            }
            (false, Entry::Number(n)) => m.args.push(n),
            (false, Entry::Empty) => {
                let default = if m.polar { [6.0, 360.0][m.args.len().min(1)] } else { [f64::NAN, 2.0, 3.0, 10.0, 10.0][m.args.len().min(4)] };
                m.args.push(default);
            }
            _ => return Err("Enter a number.".into()),
        }
        let m = &self.cmdline.modify;
        let result = match (m.polar, m.args.as_slice(), m.centre) {
            (true, [count, angle], Some(centre)) => {
                let (count, angle) = (*count as u32, *angle);
                self.apply(|d, ids| d.array_polar(ids, count, centre, angle, true).map(|_| ()))
            }
            (false, [_, rows, cols, dy, dx], _) => {
                let (rows, cols, dx, dy) = (*rows as u32, *cols as u32, *dx, *dy);
                self.apply(|d, ids| d.array_rectangular(ids, rows, cols, dx, dy).map(|_| ()))
            }
            _ => return Ok(()),
        };
        self.end();
        result
    }

    /// The item under a clicked point.
    fn item_at(&self, p: Point) -> Result<u64, String> {
        let d = self.doc.lock().unwrap();
        self.hit(&d, p).ok_or_else(|| "Nothing there; click on a shape.".into())
    }

    /// +1 for outside a closed shape (or left of an open one, as the model offsets it), -1 otherwise.
    fn offset_side(&self, id: u64, p: Point) -> f64 {
        let d = self.doc.lock().unwrap();
        let Some(item) = d.item(id) else { return 1.0 };
        if let Shape::Path { contours } = &item.shape
            && contours.iter().all(|c| c.closed)
        {
            return if item.shape.contains(p) { -1.0 } else { 1.0 };
        }
        // Open: which side of the nearest segment the point is on. Left on screen (y down) has a negative cross product.
        let outline = item.shape.outline(0.05);
        let nearest = outline
            .iter()
            .flat_map(|l| l.windows(2))
            .min_by(|a, b| segment_distance(p, a[0], a[1]).total_cmp(&segment_distance(p, b[0], b[1])));
        match nearest {
            Some(s) => {
                let cross = (s[1][0] - s[0][0]) * (p[1] - s[0][1]) - (s[1][1] - s[0][1]) * (p[0] - s[0][0]);
                if cross < 0.0 { 1.0 } else { -1.0 }
            }
            None => 1.0,
        }
    }

    /// Run an op on the selection; it becomes the command's undo step only if it succeeds.
    fn apply<T>(&mut self, f: impl FnOnce(&mut Document, &[u64]) -> Result<T, String>) -> Result<T, String> {
        let ids = self.doc.lock().unwrap().selection.clone();
        self.apply_ids(&ids, f)
    }

    /// Same for picked items; the command keeps running for the next pick.
    fn apply_to(&mut self, ids: &[u64], f: impl FnOnce(&mut Document, &[u64]) -> Result<(), String>) -> Result<(), String> {
        self.apply_ids(ids, f)
    }

    fn apply_ids<T>(&mut self, ids: &[u64], f: impl FnOnce(&mut Document, &[u64]) -> Result<T, String>) -> Result<T, String> {
        let mut d = self.doc.lock().unwrap();
        let before = d.s.clone();
        let out = f(&mut d, ids)?;
        if !self.cmdline.checkpointed {
            d.push_undo(before);
            self.cmdline.checkpointed = true;
        } else {
            d.touch();
        }
        Ok(out)
    }
}
