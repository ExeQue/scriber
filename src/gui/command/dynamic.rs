//! Dynamic input, as in AutoCAD. Once the first point is down, small fields next to the cursor
//! (and on the command card) show the measurements of what is being drawn: radius and diameter,
//! length and angle, width and height, an arc's angle. They follow the mouse. Typing a number goes
//! straight into the first field; Tab locks it and moves to the next, so e.g. the length is fixed
//! while the angle still follows the mouse; Enter places the point.

use super::curves::{ArcMode, CircleMode};
use super::parse::toward;
use super::{App, Cmd, Step};
use crate::model::Point;
use crate::model::geom::distance;
use eframe::egui::{self, DragValue, Event, Key, RichText, TextEdit};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// From the last point: radius (circle, polygon) or radius from the end point (arc by start, end, radius).
    Radius { diameter: bool },
    /// Length and angle from the last point (lines, polylines, move/copy distance).
    LengthAngle,
    /// Width and height from the first corner.
    Size,
    /// Included angle of an arc around its centre.
    Sweep,
    /// A polygon: radius from the centre, and the number of sides.
    Polygon,
}

impl Kind {
    /// Field labels and units. A diameter field shows twice the radius (they are one value).
    fn fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Kind::Radius { diameter: true } => &[("Radius", "mm"), ("Diameter", "mm")],
            Kind::Radius { diameter: false } => &[("Radius", "mm")],
            Kind::LengthAngle => &[("Length", "mm"), ("Angle", "°")],
            Kind::Size => &[("Width", "mm"), ("Height", "mm")],
            Kind::Sweep => &[("Angle", "°")],
            Kind::Polygon => &[("Radius", "mm"), ("Sides", "")],
        }
    }

    /// How many independent values there are (radius and diameter share one).
    fn values(self) -> usize {
        match self {
            Kind::LengthAngle | Kind::Size => 2,
            _ => 1,
        }
    }
}

/// Typed and locked values; unlocked ones follow the mouse.
#[derive(Default)]
pub struct DynInput {
    values: [f64; 2],
    locked: [bool; 2],
    /// What is being typed in each field (empty = show the live value as a hint).
    texts: [String; 2],
    /// The field that should take the keyboard next frame.
    focus_next: Option<usize>,
    /// What the values belong to; they reset when this changes.
    key: Option<(Cmd, Step, usize)>,
}

fn field_id(i: usize) -> egui::Id {
    egui::Id::unique("dynamic-input").with(i)
}

impl App {
    pub(super) fn has_dyn_fields(&self) -> bool {
        self.dyn_kind().is_some()
    }

    fn dyn_kind(&self) -> Option<Kind> {
        let (cmd, step) = self.cmdline.active?;
        let n = self.draft.len();
        Some(match (cmd, step, n) {
            (Cmd::Circle, Step::Draw, 1) if self.cmdline.circle_mode == CircleMode::CenterRadius => Kind::Radius { diameter: true },
            (Cmd::Polygon, Step::Draw, 1) => Kind::Polygon,
            (Cmd::Arc, Step::Draw, 2) => match self.cmdline.arc_mode {
                ArcMode::StartEndRadius => Kind::Radius { diameter: false },
                ArcMode::StartCenterEnd | ArcMode::CenterStartEnd => Kind::Sweep,
                ArcMode::ThreePoint => return None,
            },
            (Cmd::Line | Cmd::Pline | Cmd::DimLinear | Cmd::Dist, Step::Draw, 1..) => Kind::LengthAngle,
            (Cmd::Rectang, Step::Draw, 1) => Kind::Size,
            (Cmd::Move | Cmd::Copy, Step::Second, _) => Kind::LengthAngle,
            _ => return None,
        })
    }

    /// The measurements the mouse gives right now.
    fn live(&self, kind: Kind, p: Point) -> [f64; 2] {
        let Some(&last) = self.draft.last() else { return [0.0; 2] };
        match kind {
            Kind::Radius { .. } | Kind::Polygon => [distance(last, p), 0.0],
            Kind::LengthAngle => [distance(last, p), (last[1] - p[1]).atan2(p[0] - last[0]).to_degrees()],
            Kind::Size => [p[0] - self.draft[0][0], p[1] - self.draft[0][1]],
            Kind::Sweep => {
                let (centre, start) = if self.cmdline.arc_mode == ArcMode::StartCenterEnd { (self.draft[1], self.draft[0]) } else { (self.draft[0], self.draft[1]) };
                let ang = |q: Point| (centre[1] - q[1]).atan2(q[0] - centre[0]).to_degrees();
                [(ang(p) - ang(start)).rem_euclid(360.0), 0.0]
            }
        }
    }

    /// Locked values where set, the mouse's values for the rest.
    fn effective(&self, kind: Kind, p: Point) -> [f64; 2] {
        let live = self.live(kind, p);
        let d = &self.cmdline.dynamic;
        [0, 1].map(|i| if d.locked[i] { d.values[i] } else { live[i] })
    }

    /// The point the locked values describe (the preview uses it); None while nothing is locked.
    pub(in crate::gui) fn dyn_point(&self, pointer: Point) -> Option<Point> {
        if !self.cmdline.dynamic.locked.iter().any(|l| *l) {
            return None;
        }
        self.point_from_values(pointer)
    }

    fn point_from_values(&self, pointer: Point) -> Option<Point> {
        let (cmd, _) = self.cmdline.active?;
        let kind = self.dyn_kind()?;
        let last = *self.draft.last()?;
        let [a, b] = self.effective(kind, pointer);
        Some(match kind {
            Kind::Radius { .. } if cmd == Cmd::Arc => self.curve_number(cmd, a, Some(pointer))?,
            Kind::Radius { .. } | Kind::Polygon => toward(last, Some(pointer), a),
            Kind::Sweep => self.curve_number(cmd, a, Some(pointer))?,
            // Angles are counter-clockwise on screen; the model's y points down.
            Kind::LengthAngle => [last[0] + a * b.to_radians().cos(), last[1] - a * b.to_radians().sin()],
            Kind::Size => [self.draft[0][0] + a, self.draft[0][1] + b],
        })
    }

    /// Reset when the command moves on to another point or step.
    fn track(&mut self) {
        let key = self.cmdline.active.map(|(c, s)| (c, s, self.draft.len()));
        if self.cmdline.dynamic.key != key {
            self.cmdline.dynamic = DynInput { key, ..DynInput::default() };
        }
    }

    /// Set field `i` from a typed number (a diameter is stored as its radius).
    fn set_field(&mut self, kind: Kind, i: usize, v: f64) {
        // A polygon's side count is a setting, not a measurement: it changes the shape, not the point.
        if kind == Kind::Polygon && i == 1 {
            self.sides = (v.round() as u32).clamp(3, 64);
            return;
        }
        let (slot, v) = if matches!(kind, Kind::Radius { diameter: true }) && i == 1 { (0, v / 2.0) } else { (i, v) };
        self.cmdline.dynamic.values[slot] = v;
        self.cmdline.dynamic.locked[slot] = true;
    }

    fn shown(&self, kind: Kind, i: usize, values: [f64; 2]) -> f64 {
        if kind == Kind::Polygon && i == 1 {
            return self.sides as f64;
        }
        if matches!(kind, Kind::Radius { diameter: true }) && i == 1 { values[0] * 2.0 } else { values[i.min(kind.values() - 1)] }
    }

    fn place_from_values(&mut self) {
        if let Some(p) = self.point_from_values(self.pointer_mm.unwrap_or_default()) {
            self.place_point(p);
        }
    }

    /// The fields next to the cursor. Typing a number when no text field has the keyboard starts
    /// editing the first one; Tab locks the field and moves on; Enter places the point.
    pub(in crate::gui) fn dyn_cursor(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.dyn_kind() else { return };
        self.track();
        let Some(p) = self.pointer_mm else { return };
        let count = kind.fields().len();
        let focused = (0..count).find(|i| ctx.memory(|m| m.has_focus(field_id(*i))));

        // Start typing anywhere: the first number goes into the first field.
        if focused.is_none() && !ctx.text_edit_focused() {
            let typed: Option<String> = ctx.input_mut(|i| {
                let pos = i.events.iter().position(|e| matches!(e, Event::Text(t) if t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-')))?;
                match i.events.remove(pos) {
                    Event::Text(t) => Some(t),
                    _ => None,
                }
            });
            if let Some(t) = typed {
                self.cmdline.dynamic.texts[0] = t;
                self.cmdline.dynamic.focus_next = Some(0);
            }
        }
        // Tab: lock what is typed and move to the next field.
        if let Some(i) = focused
            && ctx.input_mut(|inp| inp.consume_key(egui::Modifiers::NONE, Key::Tab))
        {
            if let Ok(v) = self.cmdline.dynamic.texts[i].trim().parse::<f64>() {
                self.set_field(kind, i, v);
            }
            let next = (i + 1) % count;
            self.cmdline.dynamic.texts[next].clear();
            self.cmdline.dynamic.focus_next = Some(next);
        }

        let values = self.effective(kind, p);
        let at = self.w2s(p) + egui::vec2(20.0, 20.0);
        let mut enter = false;
        egui::Area::new(egui::Id::unique("dynamic-input-area")).fixed_pos(at).order(egui::Order::Tooltip).interactable(true).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(6.0).show(ui, |ui| {
                for (i, (label, unit)) in kind.fields().iter().enumerate() {
                    ui.horizontal(|ui| {
                        let locked = match kind {
                            Kind::Radius { .. } => self.cmdline.dynamic.locked[0],
                            Kind::Polygon => i == 0 && self.cmdline.dynamic.locked[0],
                            _ => self.cmdline.dynamic.locked[i],
                        };
                        ui.label(RichText::new(*label).small().weak());
                        let shown = self.shown(kind, i, values);
                        let hint = if kind == Kind::Polygon && i == 1 { format!("{shown}") } else { format!("{shown:.2}") };
                        let edit = TextEdit::singleline(&mut self.cmdline.dynamic.texts[i]).id(field_id(i)).hint_text(hint).desired_width(64.0);
                        let r = ui.add(edit);
                        if r.changed()
                            && let Ok(v) = self.cmdline.dynamic.texts[i].trim().parse::<f64>()
                        {
                            self.set_field(kind, i, v);
                        }
                        enter |= r.lost_focus() && ui.input(|inp| inp.key_pressed(Key::Enter));
                        ui.label(RichText::new(if locked { format!("{unit} 🔒") } else { (*unit).to_string() }).small().weak());
                    });
                }
            });
        });
        if let Some(i) = self.cmdline.dynamic.focus_next.take() {
            ctx.memory_mut(|m| m.request_focus(field_id(i)));
        }
        if enter {
            self.place_from_values();
        }
    }

    /// The same measurements on the command card, with a button that places the point.
    pub(super) fn dyn_fields(&mut self, ui: &mut egui::Ui) {
        let Some(kind) = self.dyn_kind() else { return };
        self.track();
        let p = self.pointer_mm.unwrap_or_default();
        let values = self.effective(kind, p);
        let mut set = None;
        egui::Grid::new("dynamic").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            for (i, (label, unit)) in kind.fields().iter().enumerate() {
                ui.label(*label);
                let mut v = self.shown(kind, i, values);
                if ui.add(DragValue::new(&mut v).speed(0.1).max_decimals(3).suffix(format!(" {unit}"))).changed() {
                    set = Some((i, v));
                }
                ui.end_row();
            }
        });
        if let Some((i, v)) = set {
            self.set_field(kind, i, v);
        }
        ui.label(RichText::new("Type a number to set it, Tab for the next one, Enter to place.").small().weak());
        let label = if matches!(self.cmdline.active, Some((Cmd::Move | Cmd::Copy, _))) { "Place here" } else { "Create" };
        if ui.button(label).clicked() {
            self.place_from_values();
        }
    }
}
