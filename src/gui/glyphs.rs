//! Line-art glyphs for the constraint kinds, drawn in a unit square: the ribbon's Parametric tab
//! and the constraint bars on the canvas use the same pictures.

use crate::model::Kind;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, pos2};

/// Draw the glyph for `kind` into `r` with colour `c`.
pub fn paint(p: &Painter, r: Rect, c: Color32, kind: Kind) {
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let s = Stroke::new((r.width() / 12.0).clamp(1.0, 1.6), c);
    let line = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s));
    };
    let circle = |x: f32, y: f32, rad: f32| {
        p.circle_stroke(at(x, y), rad * r.width(), s);
    };
    let dot = |x: f32, y: f32| {
        p.circle_filled(at(x, y), (r.width() / 9.0).max(1.5), c);
    };
    let arc = |cx: f32, cy: f32, rad: f32, a0: f32, a1: f32| {
        let pts: Vec<Pos2> = (0..=16)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / 16.0).to_radians();
                at(cx + rad * a.cos(), cy + rad * a.sin())
            })
            .collect();
        p.add(egui::Shape::line(pts, s));
    };
    let arrows = |a: (f32, f32), b: (f32, f32)| {
        line(&[a, b]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        let (ux, uy) = (dx / len * 0.18, dy / len * 0.18);
        for (tip, sign) in [(a, 1.0), (b, -1.0)] {
            line(&[(tip.0 + sign * ux - uy * 0.6, tip.1 + sign * uy + ux * 0.6), tip, (tip.0 + sign * ux + uy * 0.6, tip.1 + sign * uy - ux * 0.6)]);
        }
    };
    match kind {
        Kind::Coincident => {
            line(&[(0.05, 0.85), (0.5, 0.5), (0.95, 0.75)]);
            dot(0.5, 0.5);
        }
        Kind::Collinear => {
            line(&[(0.0, 0.85), (0.35, 0.6)]);
            line(&[(0.6, 0.42), (1.0, 0.15)]);
            p.add(egui::Shape::dashed_line(&[at(0.35, 0.6), at(0.6, 0.42)], Stroke::new(1.0, c), 2.0, 2.0));
        }
        Kind::Concentric => {
            circle(0.5, 0.5, 0.45);
            circle(0.5, 0.5, 0.22);
            dot(0.5, 0.5);
        }
        Kind::Fix => {
            arc(0.5, 0.42, 0.24, 180.0, 360.0);
            line(&[(0.26, 0.42), (0.26, 0.5)]);
            line(&[(0.74, 0.42), (0.74, 0.5)]);
            p.rect_stroke(Rect::from_min_max(at(0.15, 0.5), at(0.85, 0.95)), 1.0, s, egui::StrokeKind::Middle);
        }
        Kind::Parallel => {
            line(&[(0.1, 0.9), (0.55, 0.1)]);
            line(&[(0.45, 0.9), (0.9, 0.1)]);
        }
        Kind::Perpendicular => {
            line(&[(0.05, 0.9), (0.95, 0.9)]);
            line(&[(0.5, 0.9), (0.5, 0.05)]);
        }
        Kind::Horizontal => {
            line(&[(0.05, 0.5), (0.95, 0.5)]);
            line(&[(0.05, 0.3), (0.05, 0.7)]);
            line(&[(0.95, 0.3), (0.95, 0.7)]);
        }
        Kind::Vertical => {
            line(&[(0.5, 0.05), (0.5, 0.95)]);
            line(&[(0.3, 0.05), (0.7, 0.05)]);
            line(&[(0.3, 0.95), (0.7, 0.95)]);
        }
        Kind::Tangent => {
            circle(0.5, 0.62, 0.33);
            line(&[(0.0, 0.29), (1.0, 0.29)]);
        }
        Kind::Smooth => {
            arc(0.3, 0.5, 0.25, 180.0, 360.0);
            arc(0.8, 0.5, 0.25, 180.0, 0.0);
        }
        Kind::Symmetric => {
            p.add(egui::Shape::dashed_line(&[at(0.5, 0.0), at(0.5, 1.0)], Stroke::new(1.0, c), 2.5, 2.0));
            line(&[(0.38, 0.15), (0.05, 0.5), (0.38, 0.85)]);
            line(&[(0.62, 0.15), (0.95, 0.5), (0.62, 0.85)]);
        }
        Kind::Equal => {
            line(&[(0.1, 0.35), (0.9, 0.35)]);
            line(&[(0.1, 0.65), (0.9, 0.65)]);
        }
        Kind::DimHorizontal => {
            line(&[(0.05, 0.15), (0.05, 0.85)]);
            line(&[(0.95, 0.15), (0.95, 0.85)]);
            arrows((0.05, 0.5), (0.95, 0.5));
        }
        Kind::DimVertical => {
            line(&[(0.15, 0.05), (0.85, 0.05)]);
            line(&[(0.15, 0.95), (0.85, 0.95)]);
            arrows((0.5, 0.05), (0.5, 0.95));
        }
        Kind::DimAligned => {
            line(&[(0.0, 0.7), (0.3, 1.0)]);
            line(&[(0.7, 0.0), (1.0, 0.3)]);
            arrows((0.15, 0.85), (0.85, 0.15));
        }
        Kind::DimAngular => {
            line(&[(0.05, 0.95), (0.95, 0.95)]);
            line(&[(0.05, 0.95), (0.7, 0.1)]);
            arc(0.05, 0.95, 0.6, -53.0, 0.0);
        }
        Kind::DimRadial | Kind::DimDiameter => {
            circle(0.5, 0.5, 0.45);
            if kind == Kind::DimRadial {
                line(&[(0.5, 0.5), (0.82, 0.18)]);
                dot(0.5, 0.5);
            } else {
                line(&[(0.18, 0.82), (0.82, 0.18)]);
            }
        }
    }
}
