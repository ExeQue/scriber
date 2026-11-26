//! Icons for the ways a tool draws (split buttons): the shape with the points the user clicks.
//! A ring marks the first point, dots the others.

use crate::gui::command::Cmd;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, pos2};

pub(in crate::gui) fn paint_variant(p: &Painter, r: Rect, c: Color32, cmd: Cmd, n: usize) {
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let s = Stroke::new(1.4, c);
    let thin = Stroke::new(1.0, c.gamma_multiply(0.7));
    let dot = |x: f32, y: f32| _ = p.circle_filled(at(x, y), 2.2, c);
    let ring = |x: f32, y: f32| _ = p.circle_stroke(at(x, y), 2.6, Stroke::new(1.3, c));
    let line = |a: (f32, f32), b: (f32, f32), s: Stroke| _ = p.line_segment([at(a.0, a.1), at(b.0, b.1)], s);
    let arc = |cx: f32, cy: f32, rad: f32, a0: f32, a1: f32| {
        let pts: Vec<Pos2> = (0..=20)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / 20.0).to_radians();
                at(cx + rad * a.cos(), cy + rad * a.sin())
            })
            .collect();
        p.add(egui::Shape::line(pts, s));
    };
    let on = |cx: f32, cy: f32, rad: f32, deg: f32| {
        let a = deg.to_radians();
        (cx + rad * a.cos(), cy + rad * a.sin())
    };
    match (cmd, n) {
        (Cmd::Circle, _) => {
            arc(0.5, 0.5, 0.42, 0.0, 360.0);
            match n {
                0 => {
                    line((0.5, 0.5), (0.92, 0.5), thin);
                    ring(0.5, 0.5);
                    dot(0.92, 0.5);
                }
                1 => {
                    line((0.08, 0.5), (0.92, 0.5), thin);
                    ring(0.08, 0.5);
                    dot(0.92, 0.5);
                }
                _ => {
                    for (k, deg) in [200.0, 300.0, 60.0].into_iter().enumerate() {
                        let (x, y) = on(0.5, 0.5, 0.42, deg);
                        if k == 0 { ring(x, y) } else { dot(x, y) }
                    }
                }
            }
        }
        (Cmd::Arc, _) => {
            let (cx, cy, rad) = (0.5, 0.72, 0.42);
            arc(cx, cy, rad, 180.0, 360.0);
            let (start, end, top) = ((cx - rad, cy), (cx + rad, cy), (cx, cy - rad));
            match n {
                0 => {
                    ring(start.0, start.1);
                    dot(top.0, top.1);
                    dot(end.0, end.1);
                }
                1 => {
                    line(start, (cx, cy), thin);
                    ring(start.0, start.1);
                    dot(cx, cy);
                    dot(end.0, end.1);
                }
                2 => {
                    line((cx, cy), start, thin);
                    ring(cx, cy);
                    dot(start.0, start.1);
                    dot(end.0, end.1);
                }
                _ => {
                    line((cx, cy), top, thin);
                    ring(start.0, start.1);
                    dot(end.0, end.1);
                }
            }
        }
        (Cmd::Rectang, _) => match n {
            2 => {
                let pts = [(0.12, 0.62), (0.68, 0.3), (0.88, 0.65), (0.32, 0.97)];
                p.add(egui::Shape::closed_line(pts.iter().map(|&(x, y)| at(x, y)).collect(), s));
                ring(0.12, 0.62);
                dot(0.68, 0.3);
                dot(0.88, 0.65);
            }
            _ => {
                p.rect_stroke(Rect::from_min_max(at(0.08, 0.2), at(0.92, 0.8)), 0.0, s, egui::StrokeKind::Middle);
                if n == 0 {
                    ring(0.08, 0.2);
                    dot(0.92, 0.8);
                } else {
                    line((0.5, 0.5), (0.92, 0.8), thin);
                    ring(0.5, 0.5);
                    dot(0.92, 0.8);
                }
            }
        },
        (Cmd::Polygon, _) => {
            let corner = |i: usize| on(0.5, 0.5, 0.45, i as f32 * 60.0 - 90.0);
            p.add(egui::Shape::closed_line((0..6).map(|i| { let (x, y) = corner(i); at(x, y) }).collect(), s));
            let (a, b) = (corner(1), corner(2));
            let to = if n == 0 { a } else { ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0) };
            line((0.5, 0.5), to, thin);
            ring(0.5, 0.5);
            dot(to.0, to.1);
        }
        _ => {}
    }
}
