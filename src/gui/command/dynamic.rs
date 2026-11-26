//! Dynamic input, as in AutoCAD: once the first point is down, the command card shows the
//! measurements of what is being drawn (radius and diameter, length and angle, width and height…).
//! They follow the mouse until the user types a value; then the preview locks to the typed values
//! and "Create" (or Enter) uses them.

use super::curves::{ArcMode, CircleMode};
use super::parse::toward;
use super::{App, Cmd, Step};
use crate::model::Point;
use crate::model::geom::distance;
use eframe::egui::{self, DragValue};

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
}

/// The values on the card and whether the user has typed them.
#[derive(Default)]
pub struct DynInput {
    a: f64,
    b: f64,
    edited: bool,
    /// What the values belong to; they reset when this changes.
    key: Option<(Cmd, Step, usize)>,
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
            (Cmd::Polygon, Step::Draw, 1) => Kind::Radius { diameter: false },
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

    /// The point the typed values describe, when the user has typed some (the preview uses it).
    pub(in crate::gui) fn dyn_point(&self, pointer: Point) -> Option<Point> {
        if !self.cmdline.dynamic.edited {
            return None;
        }
        self.point_from_values(pointer)
    }

    fn point_from_values(&self, pointer: Point) -> Option<Point> {
        let (cmd, _) = self.cmdline.active?;
        let last = *self.draft.last()?;
        let DynInput { a, b, .. } = self.cmdline.dynamic;
        Some(match self.dyn_kind()? {
            Kind::Radius { .. } if cmd == Cmd::Arc => self.curve_number(cmd, a, Some(pointer))?,
            Kind::Radius { .. } => toward(last, Some(pointer), a),
            Kind::Sweep => self.curve_number(cmd, a, Some(pointer))?,
            // Angles are counter-clockwise on screen; the model's y points down.
            Kind::LengthAngle => [last[0] + a * b.to_radians().cos(), last[1] - a * b.to_radians().sin()],
            Kind::Size => [self.draft[0][0] + a, self.draft[0][1] + b],
        })
    }

    /// Follow the mouse until the user types; reset when the command moves on.
    fn track(&mut self, kind: Kind, pointer: Option<Point>) {
        let key = self.cmdline.active.map(|(c, s)| (c, s, self.draft.len()));
        if self.cmdline.dynamic.key != key {
            self.cmdline.dynamic = DynInput { key, ..DynInput::default() };
        }
        let (Some(p), false) = (pointer, self.cmdline.dynamic.edited) else { return };
        let Some(&last) = self.draft.last() else { return };
        let (a, b) = match kind {
            Kind::Radius { .. } => (distance(last, p), 0.0),
            Kind::LengthAngle => (distance(last, p), (last[1] - p[1]).atan2(p[0] - last[0]).to_degrees()),
            Kind::Size => (p[0] - self.draft[0][0], p[1] - self.draft[0][1]),
            Kind::Sweep => {
                let (centre, start) = if self.cmdline.arc_mode == ArcMode::StartCenterEnd { (self.draft[1], self.draft[0]) } else { (self.draft[0], self.draft[1]) };
                let ang = |q: Point| (centre[1] - q[1]).atan2(q[0] - centre[0]).to_degrees();
                ((ang(p) - ang(start)).rem_euclid(360.0), 0.0)
            }
        };
        self.cmdline.dynamic.a = a;
        self.cmdline.dynamic.b = b;
    }

    /// The measurement fields on the command card, with a button that places the point.
    pub(super) fn dyn_fields(&mut self, ui: &mut egui::Ui) {
        let Some(kind) = self.dyn_kind() else { return };
        self.track(kind, self.pointer_mm);
        let d = &mut self.cmdline.dynamic;
        let mut changed = false;
        let mut enter = false;
        let mut field = |ui: &mut egui::Ui, label: &str, v: &mut f64, suffix: &str| {
            ui.label(label);
            let r = ui.add(DragValue::new(v).speed(0.1).max_decimals(3).suffix(suffix));
            changed |= r.changed();
            enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.end_row();
        };
        egui::Grid::new("dynamic").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| match kind {
            Kind::Radius { diameter } => {
                let before = d.a;
                field(ui, "Radius", &mut d.a, " mm");
                if diameter {
                    let mut dia = d.a * 2.0;
                    field(ui, "Diameter", &mut dia, " mm");
                    if d.a == before {
                        d.a = dia / 2.0;
                    }
                }
            }
            Kind::LengthAngle => {
                field(ui, "Length", &mut d.a, " mm");
                field(ui, "Angle", &mut d.b, "°");
            }
            Kind::Size => {
                field(ui, "Width", &mut d.a, " mm");
                field(ui, "Height", &mut d.b, " mm");
            }
            Kind::Sweep => field(ui, "Angle", &mut d.a, "°"),
        });
        if changed {
            d.edited = true;
        }
        let create = ui.button(if matches!(self.cmdline.active, Some((Cmd::Move | Cmd::Copy, _))) { "Place here" } else { "Create" }).clicked();
        if (create || enter)
            && let Some(p) = self.point_from_values(self.pointer_mm.unwrap_or_default())
        {
            self.place_point(p);
        }
    }
}
