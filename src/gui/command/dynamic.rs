//! Dynamic input, as in AutoCAD. Once the first point is down, small fields next to the cursor
//! (and on the command card) show the measurements of what is being drawn: radius and diameter,
//! length and angle, width and height, an arc's angle. They follow the mouse. Typing a number goes
//! straight into the first field; Tab locks it and moves to the next, so e.g. the length is fixed
//! while the angle still follows the mouse; Enter places the point.

use super::curves::{ArcMode, CircleMode};
use super::rects::{RectMode, height_from};
use super::parse::toward;
use super::{App, Cmd, Step};
use crate::gui::grips::Grip;
use crate::gui::select::Drag;

use crate::model::Point;
use crate::model::constraints::expr::fmt_num;
use crate::model::geom::distance;
use eframe::egui::{self, DragValue, Event, Key, RichText};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Kind {
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
    /// Height of a 3-point rectangle, from its first edge.
    Height,
    /// ROTATE's angle around the base point (counter-clockwise).
    Angle,
    /// SCALE's factor (the distance from the base point, as in AutoCAD).
    Factor,
    /// DIM: the value of the dimension being placed.
    Dimension { angle: bool },
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
            Kind::Polygon => &[("Diameter", "mm"), ("Sides", "")],
            Kind::Height => &[("Height", "mm")],
            Kind::Angle => &[("Angle", "°")],
            Kind::Factor => &[("Factor", "×")],
            Kind::Dimension { angle: true } => &[("Angle", "°")],
            Kind::Dimension { angle: false } => &[("Value", "mm")],
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
    pub(super) values: [f64; 2],
    pub(super) locked: [bool; 2],
    /// The diameter field (not the radius) was typed last.
    pub(super) diameter: bool,
    /// What is being typed in each field (empty = show the live value).
    texts: [String; 2],
    /// The field the keyboard is typing into.
    active: Option<usize>,
    /// What the values belong to; they reset when this changes.
    key: Option<(Cmd, Step, usize, [u64; 2])>,
}

/// The character a number key types (used when the platform sends no text event for it).
fn key_char(key: Key) -> Option<char> {
    Some(match key {
        Key::Num0 => '0',
        Key::Num1 => '1',
        Key::Num2 => '2',
        Key::Num3 => '3',
        Key::Num4 => '4',
        Key::Num5 => '5',
        Key::Num6 => '6',
        Key::Num7 => '7',
        Key::Num8 => '8',
        Key::Num9 => '9',
        Key::Minus => '-',
        Key::Period | Key::Comma => '.',
        _ => return None,
    })
}

impl App {
    pub(in crate::gui) fn has_dyn_fields(&self) -> bool {
        self.dyn_kind().is_some()
    }

    pub(super) fn dyn_kind(&self) -> Option<Kind> {
        // A dragged or hot grip: how far and which way it moved, or a circle's radius.
        if let Some(Drag::Grip { grip, .. }) = &self.drag {
            return match grip {
                Grip::Radius { .. } => Some(Kind::Radius { diameter: true }),
                Grip::Scale { .. } | Grip::Rotate | Grip::DimLine { .. } => None,
                _ => Some(Kind::LengthAngle),
            };
        }
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
            (Cmd::Rectang, Step::Draw, 1) if self.rect_in(cmd, RectMode::ThreePoint) => Kind::LengthAngle,
            (Cmd::Rectang, Step::Draw, 1) => Kind::Size,
            (Cmd::Rectang, Step::Draw, 2) => Kind::Height,
            (Cmd::Move | Cmd::Copy, Step::Second, _) => Kind::LengthAngle,
            (Cmd::Rotate, Step::Second, _) => Kind::Angle,
            (Cmd::Scale, Step::Second, _) => Kind::Factor,
            (Cmd::Dim, Step::Location, _) => Kind::Dimension { angle: self.dim_is_angle() },
            // Everything else that asks for a point after a first one: length and angle from it.
            (Cmd::Text, ..) => return None,
            (_, Step::Second, 1..) | (_, Step::Draw | Step::Ux, 1..) if self.tool.points() != 1 => Kind::LengthAngle,
            _ => return None,
        })
    }

    /// Where the measurements start: the last point, or a dragged grip's start (a circle's centre for its radius).
    fn dyn_base(&self) -> Option<Point> {
        match &self.drag {
            Some(Drag::Grip { grip: Grip::Radius { id, contour }, base, .. }) => {
                base.iter().find(|i| i.id == *id).and_then(|i| i.shape.contours().get(*contour).and_then(|c| c.as_circle())).map(|(m, _)| m)
            }
            Some(Drag::Grip { at, .. }) => Some(*at),
            _ => self.draft.last().copied(),
        }
    }

    /// The measurements the mouse gives right now.
    fn live(&self, kind: Kind, p: Point) -> [f64; 2] {
        if let Kind::Dimension { .. } = kind {
            return [self.dim_live(p), 0.0];
        }
        let Some(last) = self.dyn_base() else { return [0.0; 2] };
        match kind {
            Kind::Radius { .. } => [distance(last, p), 0.0],
            Kind::Polygon => [2.0 * distance(last, p), 0.0],
            Kind::LengthAngle => [distance(last, p), (last[1] - p[1]).atan2(p[0] - last[0]).to_degrees()],
            // From the centre the size is twice the offset.
            Kind::Size if self.rect_in(Cmd::Rectang, RectMode::Center) => [2.0 * (p[0] - last[0]).abs(), 2.0 * (p[1] - last[1]).abs()],
            Kind::Size => [p[0] - self.draft[0][0], p[1] - self.draft[0][1]],
            Kind::Height => [height_from(self.draft[0], self.draft[1], p), 0.0],
            Kind::Angle => [(last[1] - p[1]).atan2(p[0] - last[0]).to_degrees(), 0.0],
            Kind::Factor => [distance(last, p), 0.0],
            Kind::Dimension { .. } => [0.0; 2],
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
        let cmd = self.cmdline.active.map(|(c, _)| c);
        let kind = self.dyn_kind()?;
        let last = self.dyn_base()?;
        let [a, b] = self.effective(kind, pointer);
        Some(match kind {
            Kind::Radius { .. } if cmd == Some(Cmd::Arc) => self.curve_number(Cmd::Arc, a, Some(pointer))?,
            Kind::Radius { .. } => toward(last, Some(pointer), a),
            Kind::Polygon => toward(last, Some(pointer), a / 2.0),
            Kind::Sweep => self.curve_number(cmd?, a, Some(pointer))?,
            // Angles are counter-clockwise on screen; the model's y points down.
            Kind::LengthAngle => [last[0] + a * b.to_radians().cos(), last[1] - a * b.to_radians().sin()],
            Kind::Size if cmd.is_some_and(|c| self.rect_in(c, RectMode::Center)) => [last[0] + a / 2.0, last[1] + b / 2.0],
            Kind::Size => [self.draft[0][0] + a, self.draft[0][1] + b],
            Kind::Angle => {
                let len = distance(last, pointer).max(1.0);
                [last[0] + len * a.to_radians().cos(), last[1] - len * a.to_radians().sin()]
            }
            Kind::Factor => toward(last, Some(pointer), a),
            Kind::Dimension { .. } => pointer,
            Kind::Height => {
                let (a0, b0) = (self.draft[0], self.draft[1]);
                let len = distance(a0, b0).max(1e-12);
                let n = [-(b0[1] - a0[1]) / len, (b0[0] - a0[0]) / len];
                [b0[0] + n[0] * a, b0[1] + n[1] * a]
            }
        })
    }

    /// Reset when the command moves on to another point or step. The last point is part of the
    /// key, so LINE starts fresh for every segment even though one point stays placed.
    fn track(&mut self) {
        let last = self.draft.last().map_or([0; 2], |p| [p[0].to_bits(), p[1].to_bits()]);
        let key = self.cmdline.active.map(|(c, s)| (c, s, self.draft.len(), last));
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
        let diameter = matches!(kind, Kind::Radius { diameter: true }) && i == 1;
        self.cmdline.dynamic.diameter = diameter;
        let (slot, v) = if diameter { (0, v / 2.0) } else { (i, v) };
        self.cmdline.dynamic.values[slot] = v;
        self.cmdline.dynamic.locked[slot] = true;
        // DIM keeps the typed value itself: dynamic input resets when the click places it.
        if let Kind::Dimension { .. } = kind {
            self.cmdline.smart_dim.value = fmt_num(v);
        }
    }

    fn shown(&self, kind: Kind, i: usize, values: [f64; 2]) -> f64 {
        if kind == Kind::Polygon && i == 1 {
            return self.sides as f64;
        }
        if matches!(kind, Kind::Radius { diameter: true }) && i == 1 { values[0] * 2.0 } else { values[i.min(kind.values() - 1)] }
    }

    fn place_from_values(&mut self) {
        if let Some(Kind::Dimension { .. }) = self.dyn_kind() {
            if let Err(e) = self.dim_enter() {
                self.cmdline.log(e);
            }
            return;
        }
        let Some(p) = self.point_from_values(self.pointer_mm.unwrap_or_default()) else { return };
        if matches!(self.drag, Some(Drag::Grip { .. })) {
            self.move_grip(p, false, false);
            self.finish_grip(None);
        } else {
            self.place_point(p);
        }
    }

    /// Forget typed values (a new tool, or the point was placed).
    pub(in crate::gui) fn dyn_reset(&mut self) {
        self.cmdline.dynamic = DynInput::default();
    }

    /// Keyboard for dynamic input, run before the other shortcuts so its keys are not reused:
    /// numbers type into the active field (the first one to start with), Backspace edits,
    /// Tab locks the field and moves on, Enter places the point, Esc clears what was typed.
    pub(in crate::gui) fn dyn_keys(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.dyn_kind() else { return };
        self.track();
        if ctx.text_edit_focused() {
            return;
        }
        let count = kind.fields().len();
        let events = ctx.input(|i| i.events.clone());
        let has_text = events.iter().any(|e| matches!(e, Event::Text(_)));
        let mut used = Vec::new();
        let mut place = false;
        for (n, e) in events.iter().enumerate() {
            let typed: Option<String> = match e {
                Event::Text(t) if t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')) => Some(t.replace(',', ".")),
                Event::Key { key, pressed: true, modifiers, .. } if !has_text && !modifiers.command && !modifiers.alt => key_char(*key).map(String::from),
                _ => None,
            };
            let d = &mut self.cmdline.dynamic;
            if let Some(t) = typed {
                let i = *d.active.get_or_insert(0);
                d.texts[i].push_str(&t);
                used.push(n);
                continue;
            }
            let Event::Key { key, pressed: true, .. } = e else { continue };
            match (key, d.active) {
                (Key::Backspace, Some(i)) => {
                    d.texts[i].pop();
                    used.push(n);
                }

                (Key::Enter, Some(_)) => {
                    place = true;
                    used.push(n);
                }
                (Key::Escape, Some(_)) => {
                    *d = DynInput { key: d.key, ..DynInput::default() };
                    used.push(n);
                }
                _ => {}
            }
            // Keep a digit's key press away from the shortcuts too.
            if matches!(e, Event::Key { key, .. } if key_char(*key).is_some() && d.active.is_some()) {
                used.push(n);
            }
        }
        // Tab presses were taken out of egui's input (see `take_tabs`), so egui does not move focus.
        for _ in 0..std::mem::take(&mut self.dyn_tabs) {
            let d = &mut self.cmdline.dynamic;
            d.active = Some(d.active.map_or(0, |i| (i + 1) % count));
        }
        // Typed text becomes the field's value; an emptied field follows the mouse again.
        for i in 0..count {
            let text = self.cmdline.dynamic.texts[i].clone();
            match text.trim().parse::<f64>() {
                Ok(v) => self.set_field(kind, i, v),
                Err(_) if text.is_empty() && self.cmdline.dynamic.active == Some(i) => self.unlock_field(kind, i),
                Err(_) => {}
            }
        }
        if !used.is_empty() {
            ctx.input_mut(|i| {
                let mut n = 0;
                i.events.retain(|_| {
                    n += 1;
                    !used.contains(&(n - 1))
                });
            });
        }
        if place {
            self.place_from_values();
        }
    }

    /// Called from eframe's raw input hook: while dynamic input is open and no text field has the
    /// keyboard, Tab cycles the measurement fields instead of moving egui's focus.
    pub(in crate::gui) fn take_tabs(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.dyn_kind().is_none() || ctx.text_edit_focused() {
            return;
        }
        raw.events.retain(|e| match e {
            Event::Key { key: Key::Tab, pressed, .. } => {
                if *pressed {
                    self.dyn_tabs += 1;
                }
                false
            }
            _ => true,
        });
    }

    fn unlock_field(&mut self, kind: Kind, i: usize) {
        let slot = if matches!(kind, Kind::Radius { .. } | Kind::Polygon) { 0 } else { i };
        self.cmdline.dynamic.locked[slot] = false;
        if let Kind::Dimension { .. } = kind {
            self.cmdline.smart_dim.value.clear();
        }
    }

    /// The fields next to the cursor: the active one is highlighted, locked values show a lock.
    pub(in crate::gui) fn dyn_cursor(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.dyn_kind() else { return };
        self.track();
        let Some(p) = self.pointer_mm else { return };
        let values = self.effective(kind, p);
        let at = self.w2s(p) + egui::vec2(20.0, 20.0);
        let d = &self.cmdline.dynamic;
        egui::Area::new(egui::Id::unique("dynamic-input-area")).fixed_pos(at).order(egui::Order::Tooltip).interactable(false).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(6.0).show(ui, |ui| {
                for (i, (label, unit)) in kind.fields().iter().enumerate() {
                    let slot = if matches!(kind, Kind::Radius { .. }) || (kind == Kind::Polygon && i == 0) { 0 } else { i };
                    let locked = !(kind == Kind::Polygon && i == 1) && d.locked[slot];
                    let shown = self.shown(kind, i, values);
                    let text = if d.active == Some(i) && !d.texts[i].is_empty() {
                        d.texts[i].clone()
                    } else if kind == Kind::Polygon && i == 1 {
                        format!("{shown}")
                    } else {
                        format!("{shown:.2}")
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(*label).small().weak());
                        let field = RichText::new(format!("{text:>8}")).monospace();
                        let field = if d.active == Some(i) { field.background_color(ui.visuals().selection.bg_fill).strong() } else { field };
                        ui.label(field);
                        ui.label(RichText::new(if locked { format!("{unit} 🔒") } else { (*unit).to_string() }).small().weak());
                    });
                }
                ui.label(RichText::new("Type · Tab next · Enter place").small().weak());
            });
        });
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
