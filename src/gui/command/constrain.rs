//! The constraint commands, AutoCAD style: GC* (geometric) and DC* (dimensional) ask for the
//! objects, a dimension also for where it goes and its value (`d1=40`, `d1/2`). AUTOCONSTRAIN and
//! DELCONSTRAINT work on the selection, CONSTRAINTBAR shows or hides the constraint icons,
//! PARAMETERS opens the parameters window. Each command is one undo step.

use super::parse::Entry;
use super::{App, Cmd, Step};
use crate::gui::canvas::APERTURE;
use crate::gui::tools::Tool;
use crate::model::constraints::pick::{dim_place, nearest_point, nearest_segment, offset_through, DimPlace};
use crate::model::{Document, Kind, Point, Ref};
use eframe::egui::{self, RichText};

/// What a constraint command has picked so far.
#[derive(Default)]
pub struct ConstrainState {
    refs: Vec<Ref>,
    /// The dimensional constraint whose value is being entered.
    pub editing: Option<u64>,
}

impl Cmd {
    pub(super) fn constrains(self) -> bool {
        matches!(self, Cmd::Constrain(_) | Cmd::DcLinear | Cmd::AutoConstrain | Cmd::DelConstraint | Cmd::ConstraintBar | Cmd::Parameters)
    }

    /// The constraint kind; DCLINEAR picks horizontal or vertical when the dimension is placed.
    fn kind(self) -> Kind {
        match self {
            Cmd::Constrain(k) => k,
            _ => Kind::DimAligned,
        }
    }
}

/// Plain-language names for the toolbar, card and menus (the AutoCAD name goes in the tooltip).
pub(super) fn constrain_title(cmd: Cmd) -> &'static str {
    match cmd {
        Cmd::Constrain(k) => match k {
            Kind::Coincident => "Coincident",
            Kind::Collinear => "Collinear",
            Kind::Concentric => "Concentric",
            Kind::Fix => "Fix",
            Kind::Parallel => "Parallel",
            Kind::Perpendicular => "Perpendicular",
            Kind::Horizontal => "Horizontal",
            Kind::Vertical => "Vertical",
            Kind::Tangent => "Tangent",
            Kind::Smooth => "Smooth",
            Kind::Symmetric => "Symmetric",
            Kind::Equal => "Equal",
            Kind::DimHorizontal => "Horizontal distance",
            Kind::DimVertical => "Vertical distance",
            Kind::DimAligned => "Length",
            Kind::DimAngular => "Angle",
            Kind::DimRadial => "Radius",
            Kind::DimDiameter => "Diameter",
        },
        Cmd::DcLinear => "Distance",
        Cmd::AutoConstrain => "Auto constrain",
        Cmd::DelConstraint => "Remove",
        Cmd::ConstraintBar => "Show icons",
        _ => "Parameters",
    }
}

/// Whether the `n`th pick of this command may be a point (else it is a line, arc or circle).
fn takes_point(cmd: Cmd, n: usize) -> bool {
    match cmd.kind() {
        Kind::Coincident | Kind::Fix | Kind::Horizontal | Kind::Vertical | Kind::DimHorizontal | Kind::DimVertical | Kind::DimAligned => true,
        Kind::Symmetric => n < 2,
        _ => false,
    }
}

/// Whether the picks are complete.
fn complete(cmd: Cmd, refs: &[Ref]) -> bool {
    let one_line = matches!(refs, [Ref::Segment { .. }]);
    match cmd.kind() {
        Kind::Fix | Kind::DimRadial | Kind::DimDiameter => refs.len() == 1,
        Kind::Horizontal | Kind::Vertical | Kind::DimHorizontal | Kind::DimVertical | Kind::DimAligned => refs.len() == 2 || one_line,
        Kind::Symmetric => refs.len() == 3,
        _ => refs.len() == 2,
    }
}

impl App {
    pub(super) fn start_constrain(&mut self, cmd: Cmd) {
        self.cmdline.constrain = ConstrainState::default();
        match cmd {
            Cmd::Parameters => self.show_parameters = true,
            Cmd::ConstraintBar => {
                let sel = self.doc.lock().unwrap().selection.clone();
                if sel.is_empty() {
                    self.bars = !self.bars;
                    self.hidden_bars.clear();
                } else if sel.iter().all(|id| self.hidden_bars.contains(id)) {
                    self.hidden_bars.retain(|id| !sel.contains(id));
                } else {
                    self.hidden_bars.extend(sel);
                }
            }
            Cmd::AutoConstrain | Cmd::DelConstraint => {
                self.begin(cmd, Step::Constrain);
                self.tool = Tool::Select;
                if !self.doc.lock().unwrap().selection.is_empty() {
                    self.answer(Entry::Empty, None);
                }
            }
            _ => {
                self.begin(cmd, Step::Constrain);
                self.constrain_selection(cmd);
            }
        }
    }

    pub(super) fn constrain_prompt(&self, cmd: Cmd, step: Step) -> String {
        let n = self.cmdline.constrain.refs.len();
        let dim = cmd == Cmd::DcLinear || cmd.kind().is_dimension();
        let text = match (step, cmd.kind()) {
            (Step::Expr, _) => return format!("Enter value or expression <{}>:", self.editing_label()),
            (Step::Location, _) => "Specify dimension line location:",
            _ if matches!(cmd, Cmd::AutoConstrain | Cmd::DelConstraint) => "Select objects (Enter when done):",
            (_, Kind::Fix) => "Select point or object to lock:",
            (_, Kind::DimRadial | Kind::DimDiameter) => "Select arc or circle:",
            (_, Kind::DimAngular) if n == 0 => "Select first line:",
            (_, Kind::DimAngular) => "Select second line:",
            (_, Kind::Symmetric) if n == 2 => "Select symmetry line:",
            _ if dim && n == 0 => "Specify first constraint point or select a line:",
            _ if dim => "Specify second constraint point:",
            (_, Kind::Coincident) if n == 0 => "Select first point:",
            (_, Kind::Coincident) => "Select second point or object:",
            (_, Kind::Horizontal | Kind::Vertical) if n == 0 => "Select an object or first point:",
            (_, Kind::Horizontal | Kind::Vertical) => "Select second point:",
            _ if n == 0 => "Select first object:",
            _ => "Select second object:",
        };
        text.into()
    }

    /// `d1=40` for the dimension being edited.
    fn editing_label(&self) -> String {
        let d = self.doc.lock().unwrap();
        self.cmdline.constrain.editing.and_then(|id| d.constraint(id)).map_or(String::new(), |c| format!("{}={}", c.name, c.expr))
    }

    pub(super) fn constrain_step(&mut self, cmd: Cmd, step: Step, entry: Entry) -> Result<(), String> {
        match (step, entry) {
            (Step::Expr, Entry::Empty) => {
                let text = std::mem::take(&mut self.cmdline.input);
                self.enter_value(&text);
                Ok(())
            }
            (Step::Constrain, Entry::Empty) if matches!(cmd, Cmd::AutoConstrain | Cmd::DelConstraint) => {
                let result = self.change(|d| {
                    let ids = d.selection.clone();
                    if ids.is_empty() {
                        return Err("Nothing selected.".into());
                    }
                    match cmd {
                        Cmd::AutoConstrain => {
                            let n = d.auto_constrain(&ids, 0.05, 1.0).len();
                            Ok(format!("{n} constraint(s) added."))
                        }
                        _ => {
                            let on = d.constraints_on(&ids);
                            d.delete_constraints(&on).map(|()| format!("{} constraint(s) removed.", on.len()))
                        }
                    }
                });
                self.end();
                result.map(|msg| self.cmdline.log(msg))
            }
            (_, Entry::Empty) => {
                self.end();
                Ok(())
            }
            (Step::Constrain, Entry::Point(p)) => {
                let n = self.cmdline.constrain.refs.len();
                let r = self.pick_ref(p, takes_point(cmd, n))?;
                self.mark_picked(r.item(), p);
                self.cmdline.constrain.refs.push(r);
                if !complete(cmd, &self.cmdline.constrain.refs) {
                    return Ok(());
                }
                if cmd == Cmd::DcLinear || cmd.kind().is_dimension() {
                    self.set_step(Step::Location);
                    return Ok(());
                }
                let refs = std::mem::take(&mut self.cmdline.constrain.refs);
                let result = self.change(|d| d.add_constraint(cmd.kind(), refs).map(|_| String::new()));
                self.end();
                result.map(|_| ())
            }
            (Step::Location, Entry::Point(p)) => self.place_dimension(cmd, p),
            _ => Err("Click on the drawing, or press Enter to finish.".into()),
        }
    }

    /// The dimension goes through `at`; then its value is asked for.
    fn place_dimension(&mut self, cmd: Cmd, at: Point) -> Result<(), String> {
        let refs = std::mem::take(&mut self.cmdline.constrain.refs);
        let mut id = 0;
        self.change(|d| {
            let mut kind = cmd.kind();
            let place = dim_place(&d.s, kind, &refs).ok_or("cannot measure that")?;
            if let (Cmd::DcLinear, DimPlace::Linear(p, q)) = (cmd, place) {
                // Placed beside the points: vertical; above or below them: horizontal (as AutoCAD).
                let inside = |i: usize| (p[i].min(q[i])..=p[i].max(q[i])).contains(&at[i]);
                let wide = (q[0] - p[0]).abs() >= (q[1] - p[1]).abs();
                kind = if inside(0) || (!inside(1) && wide) { Kind::DimHorizontal } else { Kind::DimVertical };
            }
            id = d.add_dimension(kind, refs, None, None, offset_through(place, kind, at))?;
            Ok(String::new())
        })?;
        self.edit_dimension(id);
        Ok(())
    }

    /// Ask for a dimension's value (also used when its label is double-clicked).
    pub(in crate::gui) fn edit_dimension(&mut self, id: u64) {
        let (kind, expr) = match self.doc.lock().unwrap().constraint(id) {
            Some(c) => (c.kind, c.expr.clone()),
            None => return,
        };
        if self.cmdline.active.is_none() {
            self.cmdline.checkpointed = false;
        }
        self.cmdline.active = Some((Cmd::Constrain(kind), Step::Expr));
        self.tool = Tool::Select;
        self.cmdline.constrain.editing = Some(id);
        self.cmdline.input = expr;
    }

    /// The value typed for a dimension: `40`, `d1/2`, `width=40` (renames it too) or empty (keep).
    pub(super) fn enter_value(&mut self, text: &str) {
        let Some(id) = self.cmdline.constrain.editing else { return self.end() };
        let text = text.trim().to_string();
        let result = self.change(|d| {
            let name = d.constraint(id).map(|c| c.name.clone()).ok_or("the dimension is gone")?;
            let (new_name, value) = match text.split_once('=') {
                Some((n, v)) => (n.trim().to_string(), v.trim().to_string()),
                None => (name.clone(), text.clone()),
            };
            if !value.is_empty() {
                d.set_parameter(&name, &value)?;
            }
            d.rename_parameter(&name, &new_name)?;
            Ok(String::new())
        });
        match result {
            Ok(_) => self.end(),
            Err(e) => {
                self.cmdline.log(e);
                self.cmdline.input = text;
            }
        }
    }

    /// DIMLINEAR from a point of the drawing to another: a driving dimension (d1 = its length)
    /// bound to those points. False when the ends are not both on points (then it is a plain note).
    pub(super) fn associative_dimension(&mut self, a: Point, b: Point) -> bool {
        let at = |p: Point| {
            let d = self.doc.lock().unwrap();
            d.s.items.iter().filter_map(|i| nearest_point(&d.s, i.id, p)).find(|(_, dist)| *dist < 1e-6).map(|r| r.0)
        };
        let (Some(ra), Some(rb)) = (at(a), at(b)) else { return false };
        if ra == rb {
            return false;
        }
        match self.change(|d| d.add_dimension(Kind::DimAligned, vec![ra, rb], None, None, 5.0).map(|id| d.constraint(id).map(|c| format!("{} = {} now locks this distance; double-click its label to change it.", c.name, c.expr)).unwrap_or_default())) {
            Ok(msg) => self.cmdline.log(msg),
            Err(e) => self.cmdline.log(e),
        }
        true
    }

    /// The point (when allowed and near) or the line/arc/circle under `p`.
    fn pick_ref(&self, p: Point, points: bool) -> Result<Ref, String> {
        let d = self.doc.lock().unwrap();
        let tol = (APERTURE / self.view.zoom) as f64;
        if points {
            let near = d.s.items.iter().filter_map(|i| nearest_point(&d.s, i.id, p)).filter(|(_, dist)| *dist <= tol).min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((r, _)) = near {
                return Ok(r);
            }
        }
        let id = self.hit(&d, p).ok_or("Nothing there; click on a line, arc or circle.")?;
        nearest_segment(&d.s, id, p).map(|r| r.0).ok_or_else(|| "Only lines, arcs and circles can be constrained.".into())
    }

    /// Change the document as part of the command's undo step, and solve at once so a conflict is
    /// reported here (and the change undone).
    fn change(&mut self, f: impl FnOnce(&mut Document) -> Result<String, String>) -> Result<String, String> {
        let mut d = self.doc.lock().unwrap();
        let before = d.s.clone();
        let out = match f(&mut d) {
            Ok(out) => out,
            Err(e) => {
                d.s = before;
                return Err(e);
            }
        };
        if !self.cmdline.checkpointed {
            d.push_undo(before);
            self.cmdline.checkpointed = true;
        } else {
            d.touch();
        }
        d.settle(true).map_err(|e| format!("Refused: {e}."))?;
        Ok(out)
    }

    /// The command card's fields for the constraint commands.
    pub(super) fn constrain_fields(&mut self, ui: &mut egui::Ui, cmd: Cmd, step: Step) {
        match step {
            Step::Expr => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.editing_label().split('=').next().unwrap_or_default().to_string()).strong());
                    ui.label("=");
                    ui.text_edit_singleline(&mut self.cmdline.input).request_focus();
                });
                ui.label(RichText::new("A number (mm or degrees), or an expression of other dimensions such as d1 / 2. Type name=value to rename.").small().weak());
            }
            Step::Location => _ = ui.label(RichText::new("Click where the dimension should go.").small().weak()),
            _ if takes_point(cmd, self.cmdline.constrain.refs.len()) => _ = ui.label(RichText::new("Click near a corner or end for a point, or on a line.").small().weak()),
            _ => {}
        }
    }
}

mod selected;
#[cfg(test)]
mod tests;
