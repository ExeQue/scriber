//! The left toolbar (icon buttons for every command, grouped like AutoCAD's Draw and Modify
//! panels) and the laser bar under the menu (what new or selected shapes do on the laser).

use super::App;
use super::command::Cmd;
use super::labels::{fill_help, op_color, op_help, op_label};
use super::glyphs;
use crate::model::{Kind, Op};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};

const BUTTON: f32 = 34.0;
const ACCENT: Color32 = Color32::from_rgb(0, 110, 160);

/// The toolbar groups; `None` is the Select button.
const GROUPS: [(&str, &[Option<Cmd>]); 5] = [
    ("Draw", &[None, Some(Cmd::Line), Some(Cmd::Pline), Some(Cmd::Rectang), Some(Cmd::Circle), Some(Cmd::Arc), Some(Cmd::Polygon), Some(Cmd::Text), Some(Cmd::DimLinear), Some(Cmd::Dist)]),
    ("Modify", &[Some(Cmd::Move), Some(Cmd::Copy), Some(Cmd::Rotate), Some(Cmd::Scale), Some(Cmd::Mirror), Some(Cmd::Erase), Some(Cmd::Fillet), Some(Cmd::Chamfer), Some(Cmd::Offset), Some(Cmd::Trim), Some(Cmd::Array)]),
    ("Combine", &[Some(Cmd::Union), Some(Cmd::Subtract), Some(Cmd::Intersect)]),
    (
        "Constrain",
        &[
            Some(Cmd::Constrain(Kind::Coincident)),
            Some(Cmd::Constrain(Kind::Collinear)),
            Some(Cmd::Constrain(Kind::Concentric)),
            Some(Cmd::Constrain(Kind::Fix)),
            Some(Cmd::Constrain(Kind::Parallel)),
            Some(Cmd::Constrain(Kind::Perpendicular)),
            Some(Cmd::Constrain(Kind::Horizontal)),
            Some(Cmd::Constrain(Kind::Vertical)),
            Some(Cmd::Constrain(Kind::Tangent)),
            Some(Cmd::Constrain(Kind::Smooth)),
            Some(Cmd::Constrain(Kind::Symmetric)),
            Some(Cmd::Constrain(Kind::Equal)),
            Some(Cmd::AutoConstrain),
            Some(Cmd::DelConstraint),
            Some(Cmd::ConstraintBar),
        ],
    ),
    (
        "Lock sizes",
        &[
            Some(Cmd::DcLinear),
            Some(Cmd::Constrain(Kind::DimHorizontal)),
            Some(Cmd::Constrain(Kind::DimVertical)),
            Some(Cmd::Constrain(Kind::DimAligned)),
            Some(Cmd::Constrain(Kind::DimAngular)),
            Some(Cmd::Constrain(Kind::DimRadial)),
            Some(Cmd::Constrain(Kind::DimDiameter)),
            Some(Cmd::Parameters),
        ],
    ),
];

fn help(cmd: Option<Cmd>) -> &'static str {
    let Some(cmd) = cmd else { return "Select shapes. Drag on empty space for a window (→) or crossing (←)." };
    match cmd {
        Cmd::Line => "Straight lines, point after point.",
        Cmd::Pline => "Connected lines as one shape; [Close] closes it.",
        Cmd::Rectang => "A rectangle from two corners; [Fillet] rounds the corners.",
        Cmd::Circle => "A circle from its centre and radius.",
        Cmd::Arc => "Part of a circle: centre, start, end.",
        Cmd::Polygon => "A shape with equal sides, e.g. a hexagon.",
        Cmd::Text => "Text to engrave or cut.",
        Cmd::DimLinear => "A measurement shown on the drawing.",
        Cmd::Dist => "Measure a distance without drawing anything.",
        Cmd::Move => "Move the selection from a base point to a second point.",
        Cmd::Copy => "Place copies of the selection.",
        Cmd::Rotate => "Turn the selection around a base point.",
        Cmd::Scale => "Make the selection bigger or smaller.",
        Cmd::Mirror => "Mirror the selection over a line.",
        Cmd::Erase => "Delete the selection.",
        Cmd::Fillet => "Round a corner with a radius, or join two lines.",
        Cmd::Chamfer => "Cut a corner off at a distance, or join two lines.",
        Cmd::Offset => "A copy at a distance, inside or outside (kerf compensation).",
        Cmd::Trim => "Cut away the part of a line between two crossings.",
        Cmd::Array => "Copies in rows and columns, or around a centre.",
        Cmd::Union => "Merge the selected shapes into one.",
        Cmd::Subtract => "Cut the other selected shapes out of the first one.",
        Cmd::Intersect => "Keep only where the selected shapes overlap.",
        Cmd::Undo | Cmd::Redo => "",
        Cmd::Constrain(k) => constraint_help(k),
        Cmd::DcLinear => "Lock the horizontal or vertical distance between two points; drag it to change.",
        Cmd::AutoConstrain => "Add the constraints the selection already nearly meets (joined ends, level lines, right angles, …).",
        Cmd::DelConstraint => "Remove every constraint from the selected shapes. They stay where they are.",
        Cmd::ConstraintBar => "Show or hide the constraint icons (for the selection, or all).",
        Cmd::Parameters => "List the locked sizes (d1, d2, …) to change their values or link them with formulas.",
    }
}

fn constraint_help(k: Kind) -> &'static str {
    match k {
        Kind::Coincident => "Join a point to another point, or put it on a line or circle.",
        Kind::Collinear => "Put two lines on one straight line.",
        Kind::Concentric => "Give two circles or arcs the same centre.",
        Kind::Fix => "Lock a point or a line where it is.",
        Kind::Parallel => "Make the second line parallel to the first.",
        Kind::Perpendicular => "Make the second line square (90°) to the first.",
        Kind::Horizontal => "Make a line (or two points) level.",
        Kind::Vertical => "Make a line (or two points) plumb.",
        Kind::Tangent => "Let a line or circle touch a circle or arc without crossing it.",
        Kind::Smooth => "Join two curves end to end without a kink.",
        Kind::Symmetric => "Mirror two points or shapes over a line.",
        Kind::Equal => "Give two lines the same length, or two circles the same size.",
        Kind::DimHorizontal => "Lock the horizontal distance between two points.",
        Kind::DimVertical => "Lock the vertical distance between two points.",
        Kind::DimAligned => "Lock a length or the distance between two points.",
        Kind::DimAngular => "Lock the angle between two lines.",
        Kind::DimRadial => "Lock the radius of a circle or arc.",
        Kind::DimDiameter => "Lock the diameter of a circle or arc.",
    }
}

impl App {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let active = self.active_cmd();
        for (title, cmds) in GROUPS {
            ui.add_space(6.0);
            ui.label(RichText::new(title).small().weak());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for &cmd in cmds {
                    let on = match cmd {
                        None => active.is_none(),
                        Some(c) => active == Some(c),
                    };
                    let name = match cmd {
                        None => "Select (Esc)".to_string(),
                        Some(c) if c.is_constraint() => format!("{} ({})", c.title(), c.name()),
                        Some(c) => format!("{} ({})", title_case(c.name()), c.alias()),
                    };
                    let response = icon_button(ui, on, |p, r, c| paint_icon(p, r, c, cmd)).on_hover_text(format!("{name}\n{}", help(cmd)));
                    if response.clicked() {
                        match cmd {
                            Some(c) => self.start(c),
                            None => self.cancel(),
                        }
                    }
                }
            });
        }
    }

    /// The laser bar: what the selection (or, with nothing selected, new shapes) does on the laser.
    pub(super) fn laser_bar(&mut self, ui: &mut egui::Ui) {
        let doc = self.doc.clone();
        let mut d = doc.lock().unwrap();
        let sel = d.selection.clone();
        let items: Vec<(Op, bool)> = d.s.items.iter().filter(|i| sel.contains(&i.id)).map(|i| (i.op, i.fill)).collect();
        // Show the selection's values when they agree; otherwise the defaults for new shapes.
        let (op, fill) = if items.is_empty() {
            (Some(self.new_op), Some(self.new_fill))
        } else {
            let same = |f: &dyn Fn(&(Op, bool)) -> bool| items.iter().all(f);
            let op = items.first().map(|i| i.0).filter(|o| same(&|i| i.0 == *o));
            let fill = items.first().map(|i| i.1).filter(|f| same(&|i| i.1 == *f));
            (op, fill)
        };
        let mut set_op = None;
        let mut set_fill = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let target = if items.is_empty() { "New shapes".to_string() } else { format!("{} selected", items.len()) };
            ui.label(RichText::new(target).weak());
            ui.add_space(8.0);
            for o in [Op::Cut, Op::Score, Op::Engrave, Op::NoLine, Op::Annotate] {
                if pill(ui, op == Some(o), Some(op_color(o)), op_label(o)).on_hover_text(op_help(o)).clicked() {
                    set_op = Some(o);
                }
            }
            ui.add_space(12.0);
            if pill(ui, fill == Some(true), None, "▦  Fill").on_hover_text(fill_help()).clicked() {
                set_fill = Some(fill != Some(true));
            }
        });
        super::library_window::pickers(ui, &mut d);
        if set_op.is_none() && set_fill.is_none() {
            return;
        }
        if items.is_empty() {
            self.new_op = set_op.unwrap_or(self.new_op);
            self.new_fill = set_fill.unwrap_or(self.new_fill);
            return;
        }
        d.checkpoint();
        for item in d.s.items.iter_mut().filter(|i| sel.contains(&i.id)) {
            item.op = set_op.unwrap_or(item.op);
            item.fill = set_fill.unwrap_or(item.fill);
        }
    }
}

fn title_case(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut c = lower.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// A square button with a painted icon.
fn icon_button(ui: &mut egui::Ui, on: bool, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(BUTTON), Sense::click());
    let visuals = ui.visuals();
    let (bg, fg) = match (on, response.hovered()) {
        (true, _) => (ACCENT, Color32::WHITE),
        (false, true) => (visuals.widgets.hovered.bg_fill, visuals.strong_text_color()),
        (false, false) => (Color32::TRANSPARENT, visuals.text_color()),
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(5), bg);
    paint(painter, rect.shrink(8.0), fg);
    response
}

/// A rounded toggle with an optional colour dot.
fn pill(ui: &mut egui::Ui, on: bool, dot: Option<Color32>, text: &str) -> egui::Response {
    let visuals = ui.visuals().clone();
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(13.5), visuals.text_color());
    let pad = vec2(10.0, 5.0);
    let dot_w = if dot.is_some() { 14.0 } else { 0.0 };
    let size = vec2(galley.size().x + dot_w, galley.size().y) + pad * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let (bg, stroke, fg) = match (on, response.hovered()) {
        (true, _) => (ACCENT, Stroke::NONE, Color32::WHITE),
        (false, true) => (visuals.widgets.hovered.bg_fill, Stroke::new(1.0, visuals.widgets.hovered.bg_stroke.color), visuals.strong_text_color()),
        (false, false) => (visuals.widgets.inactive.weak_bg_fill, Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color), visuals.text_color()),
    };
    let painter = ui.painter();
    painter.rect(rect, CornerRadius::same(255), bg, stroke, egui::StrokeKind::Inside);
    let mut x = rect.left() + pad.x;
    if let Some(c) = dot {
        painter.circle(pos2(x + 4.0, rect.center().y), 4.5, c, Stroke::new(1.0, Color32::from_white_alpha(if on { 200 } else { 60 })));
        x += dot_w;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    response
}

/// Line-art icons drawn in a unit square, so they stay sharp at any scale.
fn paint_icon(p: &egui::Painter, r: Rect, c: Color32, cmd: Option<Cmd>) {
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let s = Stroke::new(1.6, c);
    let line = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s));
    };
    let circle = |x: f32, y: f32, rad: f32| {
        p.circle_stroke(at(x, y), rad * r.width(), s);
    };
    let dot = |x: f32, y: f32| {
        p.circle_filled(at(x, y), 2.0, c);
    };
    let arc = |cx: f32, cy: f32, rad: f32, a0: f32, a1: f32| {
        let pts: Vec<Pos2> = (0..=16).map(|i| {
            let a = (a0 + (a1 - a0) * i as f32 / 16.0).to_radians();
            at(cx + rad * a.cos(), cy + rad * a.sin())
        }).collect();
        p.add(egui::Shape::line(pts, s));
    };
    let Some(cmd) = cmd else {
        // An arrow cursor.
        let pts = [(0.2, 0.05), (0.2, 0.85), (0.42, 0.65), (0.58, 0.98), (0.7, 0.92), (0.54, 0.6), (0.85, 0.6)];
        p.add(egui::Shape::closed_line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s));
        return;
    };
    match cmd {
        Cmd::Line => {
            line(&[(0.1, 0.9), (0.9, 0.1)]);
            dot(0.1, 0.9);
            dot(0.9, 0.1);
        }
        Cmd::Pline => line(&[(0.05, 0.8), (0.35, 0.2), (0.6, 0.7), (0.95, 0.3)]),
        Cmd::Rectang => _ = p.rect_stroke(Rect::from_min_max(at(0.05, 0.2), at(0.95, 0.8)), 0.0, s, egui::StrokeKind::Middle),
        Cmd::Circle => {
            circle(0.5, 0.5, 0.45);
            dot(0.5, 0.5);
        }
        Cmd::Arc => {
            arc(0.5, 0.75, 0.45, 180.0, 360.0);
            dot(0.5, 0.75);
        }
        Cmd::Polygon => {
            let pts: Vec<Pos2> = (0..6).map(|i| {
                let a = (i as f32 * 60.0 - 90.0).to_radians();
                at(0.5 + 0.47 * a.cos(), 0.5 + 0.47 * a.sin())
            }).collect();
            p.add(egui::Shape::closed_line(pts, s));
        }
        Cmd::Text => {
            line(&[(0.1, 0.12), (0.9, 0.12)]);
            line(&[(0.5, 0.12), (0.5, 0.92)]);
        }
        Cmd::DimLinear => {
            line(&[(0.05, 0.35), (0.95, 0.35)]);
            line(&[(0.05, 0.1), (0.05, 0.9)]);
            line(&[(0.95, 0.1), (0.95, 0.9)]);
            line(&[(0.2, 0.2), (0.05, 0.35), (0.2, 0.5)]);
            line(&[(0.8, 0.2), (0.95, 0.35), (0.8, 0.5)]);
        }
        Cmd::Dist => {
            p.rect_stroke(Rect::from_min_max(at(0.0, 0.35), at(1.0, 0.65)), 0.0, s, egui::StrokeKind::Middle);
            for i in 1..5 {
                let x = i as f32 * 0.2;
                line(&[(x, 0.35), (x, if i % 2 == 0 { 0.52 } else { 0.46 })]);
            }
        }
        Cmd::Move => {
            line(&[(0.5, 0.0), (0.5, 1.0)]);
            line(&[(0.0, 0.5), (1.0, 0.5)]);
            for (a, b, c2) in [((0.35, 0.15), (0.5, 0.0), (0.65, 0.15)), ((0.35, 0.85), (0.5, 1.0), (0.65, 0.85)), ((0.15, 0.35), (0.0, 0.5), (0.15, 0.65)), ((0.85, 0.35), (1.0, 0.5), (0.85, 0.65))] {
                line(&[a, b, c2]);
            }
        }
        Cmd::Copy => {
            p.rect_stroke(Rect::from_min_max(at(0.0, 0.0), at(0.62, 0.62)), 2.0, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(at(0.38, 0.38), at(1.0, 1.0)), 2.0, s, egui::StrokeKind::Middle);
        }
        Cmd::Rotate => {
            arc(0.5, 0.5, 0.42, 200.0, 470.0);
            line(&[(0.62, 0.0), (0.82, 0.12), (0.66, 0.3)]);
        }
        Cmd::Scale => {
            p.rect_stroke(Rect::from_min_max(at(0.0, 0.45), at(0.55, 1.0)), 0.0, s, egui::StrokeKind::Middle);
            line(&[(0.45, 0.55), (1.0, 0.0)]);
            line(&[(0.62, 0.0), (1.0, 0.0), (1.0, 0.38)]);
        }
        Cmd::Mirror => {
            p.add(egui::Shape::dashed_line(&[at(0.5, 0.0), at(0.5, 1.0)], s, 3.0, 2.5));
            line(&[(0.38, 0.15), (0.05, 0.85), (0.38, 0.85), (0.38, 0.15)]);
            line(&[(0.62, 0.15), (0.95, 0.85), (0.62, 0.85), (0.62, 0.15)]);
        }
        Cmd::Erase => {
            line(&[(0.15, 0.15), (0.85, 0.85)]);
            line(&[(0.85, 0.15), (0.15, 0.85)]);
        }
        Cmd::Fillet => {
            line(&[(0.1, 0.05), (0.1, 0.55)]);
            arc(0.55, 0.55, 0.45, 90.0, 180.0);
            line(&[(0.55, 1.0), (0.95, 1.0)]);
        }
        Cmd::Chamfer => line(&[(0.1, 0.05), (0.1, 0.6), (0.5, 1.0), (0.95, 1.0)]),
        Cmd::Offset => {
            line(&[(0.05, 0.95), (0.05, 0.35), (0.65, 0.35)]);
            line(&[(0.35, 0.95), (0.35, 0.65), (0.95, 0.65)]);
            p.add(egui::Shape::dashed_line(&[at(0.05, 0.05), at(0.95, 0.05)], Stroke::new(1.0, c), 2.0, 2.0));
        }
        Cmd::Trim => {
            line(&[(0.0, 0.5), (0.45, 0.5)]);
            p.add(egui::Shape::dashed_line(&[at(0.55, 0.5), at(1.0, 0.5)], s, 2.5, 2.5));
            line(&[(0.5, 0.05), (0.5, 0.95)]);
        }
        Cmd::Array => {
            for y in [0.0, 0.55] {
                for x in [0.0, 0.55] {
                    p.rect_stroke(Rect::from_min_size(at(x, y), vec2(0.45, 0.45) * r.width()), 1.0, s, egui::StrokeKind::Middle);
                }
            }
        }
        Cmd::Union | Cmd::Subtract | Cmd::Intersect => {
            let fill = c.gamma_multiply(0.45);
            let (a, b) = (at(0.36, 0.5), at(0.64, 0.5));
            let rad = 0.34 * r.width();
            match cmd {
                Cmd::Union => {
                    p.circle_filled(a, rad, fill);
                    p.circle_filled(b, rad, fill);
                }
                Cmd::Subtract => {
                    p.circle_filled(a, rad, fill);
                }
                _ => {
                    // The lens where the two circles overlap: circle a's right arc, then circle b's left arc.
                    let h = (rad * rad - (0.14 * r.width()).powi(2)).sqrt();
                    let side = |centre: Pos2, dir: f32, from: f32, to: f32| {
                        (0..=12).map(move |i| {
                            let y = from + (to - from) * i as f32 / 12.0;
                            pos2(centre.x + dir * (rad * rad - y * y).max(0.0).sqrt(), centre.y + y)
                        })
                    };
                    let lens: Vec<Pos2> = side(a, 1.0, -h, h).chain(side(b, -1.0, h, -h)).collect();
                    p.add(egui::Shape::convex_polygon(lens, fill, Stroke::NONE));
                }
            }
            p.circle_stroke(a, rad, s);
            p.circle_stroke(b, rad, s);
            if cmd == Cmd::Subtract {
                p.circle_filled(b, rad - 0.8, ui_bg_hint(c));
                p.circle_stroke(b, rad, s);
            }
        }
        Cmd::Undo | Cmd::Redo => {}
        Cmd::Constrain(k) => glyphs::paint(p, r, c, k),
        Cmd::DcLinear => {
            glyphs::paint(p, r, c, Kind::DimHorizontal);
            p.text(at(0.5, 0.15), egui::Align2::CENTER_CENTER, "d", egui::FontId::proportional(r.height() * 0.45), c);
        }
        Cmd::AutoConstrain => {
            glyphs::paint(p, Rect::from_min_max(at(0.0, 0.3), at(0.7, 1.0)), c, Kind::Perpendicular);
            line(&[(0.82, 0.0), (0.82, 0.36)]);
            line(&[(0.64, 0.18), (1.0, 0.18)]);
        }
        Cmd::DelConstraint => {
            glyphs::paint(p, Rect::from_min_max(at(0.0, 0.0), at(0.7, 0.7)), c, Kind::Parallel);
            line(&[(0.6, 0.6), (1.0, 1.0)]);
            line(&[(1.0, 0.6), (0.6, 1.0)]);
        }
        Cmd::ConstraintBar => {
            for x in [0.0, 0.36, 0.72] {
                p.rect_stroke(Rect::from_min_size(at(x, 0.32), vec2(0.28, 0.36) * r.width()), 1.0, s, egui::StrokeKind::Middle);
            }
        }
        Cmd::Parameters => {
            p.rect_stroke(Rect::from_min_max(at(0.0, 0.1), at(1.0, 0.9)), 1.0, s, egui::StrokeKind::Middle);
            for y in [0.37, 0.63] {
                line(&[(0.0, y), (1.0, y)]);
            }
            line(&[(0.4, 0.1), (0.4, 0.9)]);
        }
    }
}

/// Background-ish colour to punch the subtracted circle out of the filled one.
fn ui_bg_hint(fg: Color32) -> Color32 {
    if fg == Color32::WHITE { ACCENT } else { Color32::from_gray(27) }
}
