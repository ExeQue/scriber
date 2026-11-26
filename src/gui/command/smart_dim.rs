//! DIM, AutoCAD's smart dimension, as one tool for every driving dimension (a lock on a size):
//! click a line for its length, a circle for its diameter, an arc for its radius, two points for
//! their distance, two lines for their angle (see `rules`). The dimension follows the pointer until
//! clicked; dynamic input shows its value, and a typed number (or an expression on the card) and
//! Enter places it with that value, which then drives the geometry. It repeats until Esc or Enter.

mod rules;

use super::parse::Entry;
use super::{App, Cmd, Step};
use crate::model::constraints::expr::fmt_num;
use crate::gui::canvas::APERTURE;
use crate::model::constraints::pick::{nearest_segment, outline};
use crate::model::{Constraint, Document, Kind, Point, Ref, State};
use eframe::egui::{self, Color32, RichText, Stroke};
use rules::{Layout, Pick, classify, layout, plan};

/// What DIM has picked so far, and the value typed for it.
#[derive(Default)]
pub struct SmartDim {
    picks: Vec<Ref>,
    /// Radius instead of diameter (diameter for an arc); Shift swaps it too.
    flip: bool,
    /// A number or expression typed (dynamic input or the card); empty = the size as it is.
    pub value: String,
    /// A name typed on the card; empty = the next free d1, rad1, ….
    pub name: String,
    /// Where the pointer last was on the drawing, for placing from the keyboard or the card.
    at: Option<Point>,
}

const PREVIEW: Color32 = Color32::from_rgb(120, 165, 235);
const HOVER: Color32 = Color32::from_rgb(90, 170, 255);

/// "length", "angle", … for messages.
fn what(kind: Kind) -> &'static str {
    match kind {
        Kind::DimAngular => "angle",
        Kind::DimRadial => "radius",
        Kind::DimDiameter => "diameter",
        _ => "distance",
    }
}

impl App {
    pub(super) fn start_dim(&mut self) {
        self.begin(Cmd::Dim, Step::Constrain);
        let flip = self.cmdline.smart_dim.flip;
        self.cmdline.smart_dim = SmartDim { flip, ..SmartDim::default() };
    }

    pub(super) fn dim_prompt(&self, step: Step) -> String {
        let picks = &self.cmdline.smart_dim.picks;
        match (step, picks.as_slice()) {
            (Step::Location, [Ref::Segment { .. }]) => "Specify dimension location or select a second line for an angle:",
            (Step::Location, _) => "Specify dimension location:",
            (_, []) => "Select a line, circle or arc, or specify first point:",
            _ => "Specify second point or select a line:",
        }
        .into()
    }

    pub(super) fn dim_step(&mut self, step: Step, entry: Entry) -> Result<(), String> {
        match (step, entry) {
            (_, Entry::Empty) => {
                self.end();
                Ok(())
            }
            (Step::Constrain, Entry::Point(p)) => {
                let r = self.pick_ref(p, true).map_err(|_| "Nothing there: click a line, circle or arc, or near a corner or centre.")?;
                let sd = &mut self.cmdline.smart_dim;
                sd.picks.push(r);
                let ready = plan(&self.doc.lock().unwrap().s, &sd.picks, false).is_some();
                if ready {
                    self.set_step(Step::Location);
                } else if sd.picks.len() > 1 {
                    sd.picks.pop();
                    return Err("Pick a second point, or a straight line.".into());
                }
                Ok(())
            }
            (Step::Location, Entry::Point(p)) => {
                if let Some(r) = self.second_line(p) {
                    self.cmdline.smart_dim.picks.push(r);
                    self.cmdline.smart_dim.value.clear();
                    self.dyn_reset();
                    return Ok(());
                }
                self.dim_place(p)
            }
            (Step::Location, Entry::Number(v)) => {
                self.cmdline.smart_dim.value = fmt_num(v);
                self.dim_enter()
            }
            _ => Err("Click on the drawing, or press Enter to finish.".into()),
        }
    }

    /// Another straight line under `p` while one line is picked: the angle between them.
    fn second_line(&self, p: Point) -> Option<Ref> {
        let [first @ Ref::Segment { .. }] = self.cmdline.smart_dim.picks.as_slice() else { return None };
        // Only right on a line: placing inside a shape must not pick its sides.
        let d = self.doc.lock().unwrap();
        let (r, dist) = d.s.items.iter().filter_map(|i| nearest_segment(&d.s, i.id, p)).min_by(|a, b| a.1.total_cmp(&b.1))?;
        (dist <= (APERTURE / self.view.zoom) as f64 && r != *first && classify(&d.s, &r) == Some(Pick::Line)).then_some(r)
    }

    /// Radius or diameter, with Shift held.
    fn dim_flip(&self) -> bool {
        self.cmdline.smart_dim.flip ^ self.shift
    }

    /// What a click at `at` would add, including a second line under the pointer.
    fn dim_layout(&self, s: &State, at: Point) -> Option<Layout> {
        let mut picks = self.cmdline.smart_dim.picks.clone();
        picks.extend(self.second_line(at));
        layout(s, &picks, self.dim_flip(), at)
    }

    /// The value dynamic input shows: the size as it is with the pointer at `p`.
    pub(super) fn dim_live(&self, p: Point) -> f64 {
        let s = self.doc.lock().unwrap().s.clone();
        self.dim_layout(&s, p).map_or(0.0, |l| l.value)
    }

    /// The picks are two lines: the value is an angle.
    pub(super) fn dim_is_angle(&self) -> bool {
        matches!(self.cmdline.smart_dim.picks.as_slice(), [Ref::Segment { .. }, Ref::Segment { .. }])
    }

    /// Enter in dynamic input or on the card: place it where the pointer last was.
    pub(super) fn dim_enter(&mut self) -> Result<(), String> {
        let at = self.cmdline.smart_dim.at.ok_or("Point at where the dimension goes, then press Enter.")?;
        self.dim_place(at)
    }

    /// Add the dimension through `at`, with the typed value or the size as it is, as its own undo
    /// step; then pick the next one. A refused value keeps the picks, so another can be typed.
    fn dim_place(&mut self, at: Point) -> Result<(), String> {
        let s = self.doc.lock().unwrap().s.clone();
        let lay = self.dim_layout(&s, at).ok_or("That cannot be measured.")?;
        let value = std::mem::take(&mut self.cmdline.smart_dim.value).trim().to_string();
        let name = self.cmdline.smart_dim.name.trim().to_string();
        self.dyn_reset();
        let msg = if already_locked(&s, &lay) {
            self.dim_next();
            format!("That {} is already fixed by other constraints; change or remove those to set it here.", what(lay.kind))
        } else {
            self.cmdline.checkpointed = false;
            let result = self.change(|d| {
                let id = d.add_dimension(lay.kind, lay.refs.clone(), (!name.is_empty()).then_some(name.as_str()), (!value.is_empty()).then_some(value.as_str()), lay.offset)?;
                Ok(d.constraint(id).map(|c| format!("{} = {} now locks this {}; double-click its label to change it.", c.name, c.expr, what(c.kind))).unwrap_or_default())
            });
            self.cmdline.checkpointed = false;
            match result {
                Ok(msg) => {
                    self.cmdline.smart_dim.name.clear();
                    self.dim_next();
                    msg
                }
                Err(e) if value.is_empty() => e,
                Err(e) => format!("Cannot make it {value}: {e} Type another value, or press Esc."),
            }
        };
        self.cmdline.log(msg.clone());
        self.message = msg;
        Ok(())
    }

    fn dim_next(&mut self) {
        self.cmdline.smart_dim.picks.clear();
        self.set_step(Step::Constrain);
    }

    /// Hover glow on what a click would pick, the picks so far, and the dimension that follows the
    /// pointer.
    pub(in crate::gui) fn paint_dim_preview(&mut self, painter: &egui::Painter, s: &State) {
        let Some((Cmd::Dim, step)) = self.cmdline.active else { return };
        let Some(at) = self.pointer_mm else { return };
        self.cmdline.smart_dim.at = Some(at);
        let mut glow: Vec<Ref> = self.cmdline.smart_dim.picks.clone();
        match step {
            Step::Constrain => glow.extend(self.pick_ref(at, true).ok()),
            _ => glow.extend(self.second_line(at)),
        }
        let stroke = Stroke::new(4.0, HOVER.gamma_multiply(0.6));
        for r in &glow {
            match outline(s, r).as_slice() {
                [p] => _ = painter.circle_stroke(self.w2s(*p), 6.0, stroke),
                pts => _ = painter.add(egui::Shape::line(pts.iter().map(|p| self.w2s(*p)).collect(), stroke)),
            }
        }
        if step != Step::Location {
            return;
        }
        let Some(lay) = self.dim_layout(s, at) else { return };
        let typed_name = self.cmdline.smart_dim.name.trim();
        let name = if typed_name.is_empty() { (1..).map(|n| format!("{}{n}", lay.kind.prefix())).find(|n| !s.constraints.iter().any(|c| &c.name == n)).unwrap_or_default() } else { typed_name.to_string() };
        let typed = self.cmdline.smart_dim.value.trim();
        let expr = if typed.is_empty() { fmt_num(lay.value) } else { typed.to_string() };
        let c = Constraint { id: 0, kind: lay.kind, refs: lay.refs, name, expr, offset: lay.offset, shift: 0.0 };
        self.paint_lock_dimension(painter, s, &c, typed.is_empty().then_some(lay.value), PREVIEW);
    }

    /// The command card: what to click, radius or diameter, and a value or expression.
    pub(super) fn dim_fields(&mut self, ui: &mut egui::Ui, step: Step) {
        if step != Step::Location {
            ui.label(RichText::new("Click a line for its length, a circle or arc for its size, two corners for their distance, or two lines for their angle.").small().weak());
            return;
        }
        let round = self.cmdline.smart_dim.picks.len() == 1 && {
            let d = self.doc.lock().unwrap();
            matches!(classify(&d.s, &self.cmdline.smart_dim.picks[0]), Some(Pick::Circle | Pick::Arc))
        };
        if round {
            let sd = &mut self.cmdline.smart_dim;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut sd.flip, false, "Standard");
                ui.selectable_value(&mut sd.flip, true, "Swap radius / diameter");
            })
            .response
            .on_hover_text("A circle gets its diameter and an arc its radius; hold Shift to swap while placing.");
        }
        ui.horizontal(|ui| {
            ui.label("Name");
            let field = ui.add(egui::TextEdit::singleline(&mut self.cmdline.smart_dim.name).hint_text("d1").desired_width(80.0)).on_hover_text("Optional: a name to use in other expressions (width, depth, …)");
            if field.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Err(e) = self.dim_enter()
            {
                self.cmdline.log(e.clone());
                self.message = e;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Value");
            let field = ui.add(egui::TextEdit::singleline(&mut self.cmdline.smart_dim.value).hint_text("as it is, or 40, or d1 / 2").desired_width(150.0));
            if field.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Err(e) = self.dim_enter()
            {
                self.cmdline.log(e.clone());
                self.message = e;
            }
        });
        ui.label(RichText::new("Click on the drawing to place it, or type a value and press Enter. The value then drives the shape.").small().weak());
    }
}

/// The measure cannot change: the other constraints already fix it (a nudged value is refused).
fn already_locked(s: &State, lay: &Layout) -> bool {
    let mut t = Document::default();
    t.s = s.clone();
    let nudged = if lay.kind == Kind::DimAngular && lay.value >= 90.0 { lay.value - 1.0 } else { lay.value + 1.0 };
    t.add_dimension(lay.kind, lay.refs.clone(), None, Some(&fmt_num(nudged)), 0.0).is_err() || t.settle(false).is_err()
}

#[cfg(test)]
mod tests;
