//! The ribbon's icons and help texts: line-art icons painted for every command (so they stay sharp
//! at any size), the plain-language help shown in tooltips, and the rounded toggle (`pill`).

use super::command::Cmd;
use super::glyphs;
use crate::model::Kind;
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, pos2, vec2};

pub(super) const ACCENT: Color32 = Color32::from_rgb(0, 110, 160);

/// The help text of a command (`None` is Select), shown in its tooltip.
pub(super) fn help(cmd: Option<Cmd>) -> &'static str {
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
        Cmd::Undo => "Take back the last change.",
        Cmd::Redo => "Do again what Undo took back.",
        Cmd::Constrain(k) => constraint_help(k),
        Cmd::DcLinear => "Lock the horizontal or vertical distance between two points; drag it to change.",
        Cmd::AutoConstrain => "Add the constraints the selection already nearly meets (joined ends, level lines, right angles, …).",
        Cmd::DelConstraint => "Remove every constraint from the selected shapes. They stay where they are.",
        Cmd::ConstraintBar => "Show or hide the constraint icons (for the selection, or all).",
        Cmd::Parameters => "List the locked sizes (d1, d2, …) to change their values or link them with formulas.",
        c => super::icons_ux::help(c),
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

/// A rounded toggle with an optional colour dot.
pub(super) fn pill(ui: &mut egui::Ui, on: bool, dot: Option<Color32>, text: &str) -> egui::Response {
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
pub(super) fn paint_icon(p: &egui::Painter, r: Rect, c: Color32, cmd: Option<Cmd>) {
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
        Cmd::Undo | Cmd::Redo => {
            // A curved arrow, pointing left for Undo and right for Redo.
            let flip = |x: f32| if cmd == Cmd::Undo { x } else { 1.0 - x };
            let pts: Vec<Pos2> = (0..=16).map(|i| {
                let a = (180.0 + 180.0 * i as f32 / 16.0).to_radians();
                at(flip(0.55 + 0.35 * a.cos()), 0.65 + 0.4 * a.sin())
            }).collect();
            p.add(egui::Shape::line(pts, s));
            line(&[(flip(0.05), 0.45), (flip(0.2), 0.68), (flip(0.38), 0.5)]);
        }
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
        cmd => super::icons_ux::paint(p, r, c, cmd),
    }
}

/// Background-ish colour to punch the subtracted circle out of the filled one.
fn ui_bg_hint(fg: Color32) -> Color32 {
    if fg == Color32::WHITE { ACCENT } else { Color32::from_gray(27) }
}
