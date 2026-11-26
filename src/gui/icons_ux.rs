//! Help texts and line-art icons for the AutoCAD UX pack's commands (guides, zoom, editing,
//! selection), drawn in the same style as icons.rs.

use super::command::Cmd;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, pos2};

pub(super) fn help(cmd: Cmd) -> &'static str {
    match cmd {
        Cmd::Xline => "An endless guide line to draw along: through two points, level, plumb, at an angle, halving an angle or parallel. Never cut.",
        Cmd::Ray => "A guide line from a point to the edge of the world. Never cut.",
        Cmd::Centerline => "A guide line midway between two lines. It follows them when they move.",
        Cmd::Centermark => "A cross on the centre of a circle or arc. It follows the circle.",
        Cmd::Zoom => "Drag a box to zoom in on it; or go back to the previous view, or to the selection.",
        Cmd::Stretch => "Drag a box over corners to move just those; the lines to them stretch.",
        Cmd::Join => "Join lines that touch end to end into one shape.",
        Cmd::Explode => "Split a shape into its separate lines and arcs (or a compound path into its parts).",
        Cmd::Align => "Move and turn the selection so two of its points land on two other points.",
        Cmd::MatchProp => "Copy the laser operation (and font) from one shape to others.",
        Cmd::Break => "Cut a gap into a line or shape, or split it at a point.",
        Cmd::Extend => "Make a line or arc longer until it meets the next shape.",
        Cmd::Lengthen => "Make a line or arc longer or shorter by an amount, or to an exact length.",
        Cmd::Group => "Keep the selected shapes together as a group.",
        Cmd::Ungroup => "Undo a group; the shapes stay where they are.",
        Cmd::Isolate => "Show only the selected shapes for now (nothing is deleted).",
        Cmd::Unisolate => "Show every shape again.",
        Cmd::SelectSimilar => "Select every shape like the selected one (same operation, same kind).",
        Cmd::QuickSelect => "Select shapes by laser operation, kind and size.",
        _ => "",
    }
}

/// Paint the icon of a UX-pack command in the unit square `r`.
pub(super) fn paint(p: &egui::Painter, r: Rect, c: Color32, cmd: Cmd) {
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let s = Stroke::new(1.6, c);
    let line = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s));
    };
    let dashed = |a: (f32, f32), b: (f32, f32)| {
        p.add(egui::Shape::dashed_line(&[at(a.0, a.1), at(b.0, b.1)], s, 4.0, 2.0));
    };
    let dot = |x: f32, y: f32| {
        p.circle_filled(at(x, y), 2.0, c);
    };
    let rect = |a: (f32, f32), b: (f32, f32)| {
        p.rect_stroke(Rect::from_min_max(at(a.0, a.1), at(b.0, b.1)), 0.0, s, egui::StrokeKind::Middle);
    };
    match cmd {
        Cmd::Xline => {
            dashed((0.0, 1.0), (1.0, 0.0));
            dot(0.5, 0.5);
        }
        Cmd::Ray => {
            dashed((0.15, 0.85), (1.0, 0.0));
            dot(0.15, 0.85);
        }
        Cmd::Centerline => {
            line(&[(0.1, 0.1), (0.9, 0.1)]);
            line(&[(0.1, 0.9), (0.9, 0.9)]);
            dashed((0.0, 0.5), (1.0, 0.5));
        }
        Cmd::Centermark => {
            p.circle_stroke(at(0.5, 0.5), 0.32 * r.width(), s);
            dashed((0.0, 0.5), (1.0, 0.5));
            dashed((0.5, 0.0), (0.5, 1.0));
        }
        Cmd::Zoom => {
            p.circle_stroke(at(0.4, 0.4), 0.3 * r.width(), s);
            line(&[(0.62, 0.62), (0.95, 0.95)]);
            line(&[(0.28, 0.4), (0.52, 0.4)]);
            line(&[(0.4, 0.28), (0.4, 0.52)]);
        }
        Cmd::Stretch => {
            line(&[(0.0, 0.3), (0.45, 0.3), (0.45, 0.9), (0.0, 0.9)]);
            dashed((0.45, 0.3), (0.95, 0.3));
            dashed((0.45, 0.9), (0.95, 0.9));
            line(&[(0.6, 0.6), (0.95, 0.6)]);
            line(&[(0.82, 0.48), (0.95, 0.6), (0.82, 0.72)]);
        }
        Cmd::Join => {
            line(&[(0.0, 0.8), (0.5, 0.2), (1.0, 0.8)]);
            dot(0.5, 0.2);
        }
        Cmd::Explode => {
            line(&[(0.0, 0.35), (0.35, 0.0)]);
            line(&[(0.65, 0.0), (1.0, 0.35)]);
            line(&[(0.0, 0.65), (0.35, 1.0)]);
            line(&[(0.65, 1.0), (1.0, 0.65)]);
            dot(0.5, 0.5);
        }
        Cmd::Align => {
            rect((0.0, 0.55), (0.35, 0.95));
            p.add(egui::Shape::closed_line(vec![at(0.7, 0.05), at(0.95, 0.3), at(0.7, 0.55), at(0.45, 0.3)], s));
            dashed((0.35, 0.55), (0.45, 0.3));
        }
        Cmd::MatchProp => {
            line(&[(0.1, 0.9), (0.55, 0.45)]);
            p.add(egui::Shape::convex_polygon(vec![at(0.5, 0.3), at(0.85, 0.0), at(1.0, 0.15), at(0.7, 0.5)], c, s));
        }
        Cmd::Break => {
            line(&[(0.0, 0.5), (0.38, 0.5)]);
            line(&[(0.62, 0.5), (1.0, 0.5)]);
            dot(0.38, 0.5);
            dot(0.62, 0.5);
        }
        Cmd::Extend => {
            line(&[(0.9, 0.0), (0.9, 1.0)]);
            line(&[(0.0, 0.5), (0.45, 0.5)]);
            dashed((0.45, 0.5), (0.9, 0.5));
        }
        Cmd::Lengthen => {
            line(&[(0.0, 0.5), (0.6, 0.5)]);
            line(&[(0.7, 0.5), (1.0, 0.5)]);
            line(&[(0.85, 0.35), (1.0, 0.5), (0.85, 0.65)]);
        }
        Cmd::Group | Cmd::Ungroup => {
            rect((0.1, 0.1), (0.5, 0.5));
            p.circle_stroke(at(0.68, 0.68), 0.2 * r.width(), s);
            let corners: [Pos2; 4] = [at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)];
            if cmd == Cmd::Group {
                p.add(egui::Shape::dashed_line(&[corners[0], corners[1], corners[2], corners[3], corners[0]], Stroke::new(1.0, c), 3.0, 2.0));
            } else {
                for k in [0, 2] {
                    p.add(egui::Shape::dashed_line(&[corners[k], corners[k + 1]], Stroke::new(1.0, c), 3.0, 2.0));
                }
            }
        }
        Cmd::Isolate | Cmd::Unisolate => {
            // An eye; crossed out for ISOLATE (the others disappear).
            let lid = |sign: f32| (0..=12).map(|i| {
                let t = i as f32 / 12.0;
                at(t, 0.5 - sign * 0.35 * (t * std::f32::consts::PI).sin())
            }).collect::<Vec<_>>();
            p.add(egui::Shape::line(lid(1.0), s));
            p.add(egui::Shape::line(lid(-1.0), s));
            p.circle_filled(at(0.5, 0.5), 0.12 * r.width(), c);
            if cmd == Cmd::Isolate {
                line(&[(0.1, 0.95), (0.9, 0.05)]);
            }
        }
        Cmd::SelectSimilar => {
            rect((0.0, 0.1), (0.4, 0.5));
            rect((0.6, 0.5), (1.0, 0.9));
            line(&[(0.4, 0.3), (0.6, 0.7)]);
            line(&[(0.15, 0.3), (0.25, 0.4)]);
        }
        Cmd::QuickSelect => {
            p.add(egui::Shape::closed_line(vec![at(0.0, 0.05), at(1.0, 0.05), at(0.62, 0.5), at(0.62, 0.95), at(0.38, 0.8), at(0.38, 0.5)], s));
        }
        _ => {}
    }
}
