//! Dimensional constraints on the canvas: drawn as dimensions with their `name=value` label
//! (`d1=40`, `d2=d1/2 (20)`). Double-clicking a label edits its value.

use super::App;
use super::canvas::arrow_head;
use crate::model::constraints::expr::fmt_num;
use crate::model::constraints::parameter_values;
use crate::model::constraints::pick::{DimPlace, dim_place};
use crate::model::{Constraint, Kind, Point, State};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, vec2};

const DIM: Color32 = Color32::from_rgb(40, 110, 220);

/// `d1=40`, or `d2=d1/2 (20)` when the value comes from an expression.
fn label(c: &Constraint, value: Option<f64>) -> String {
    let unit = if c.kind == Kind::DimAngular { "°" } else { "" };
    let prefix = match c.kind {
        Kind::DimRadial => "R ",
        Kind::DimDiameter => "Ø ",
        _ => "",
    };
    match value {
        Some(v) if c.expr.trim() == fmt_num(v) => format!("{prefix}{}={}{unit}", c.name, c.expr.trim()),
        Some(v) => format!("{prefix}{}={} ({}{unit})", c.name, c.expr, fmt_num(v)),
        None => format!("{prefix}{}={}", c.name, c.expr),
    }
}

fn add(p: Point, v: [f64; 2], k: f64) -> Point {
    [p[0] + v[0] * k, p[1] + v[1] * k]
}

impl App {
    /// A double-click on a dimension's label asks for its value.
    pub(super) fn dimension_input(&mut self, response: &egui::Response) {
        if !response.double_clicked() {
            return;
        }
        let Some(at) = response.interact_pointer_pos() else { return };
        if let Some((id, _)) = self.dim_labels.iter().find(|(_, r)| r.contains(at)).copied() {
            self.edit_dimension(id);
        }
    }

    pub(super) fn paint_dimensions(&mut self, painter: &egui::Painter, s: &State) {
        let mut labels = Vec::new();
        let values = parameter_values(s).unwrap_or_default();
        let stroke = Stroke::new(1.0, DIM);
        let line = |a: Point, b: Point| painter.line_segment([self.w2s(a), self.w2s(b)], stroke);
        for c in s.constraints.iter().filter(|c| c.kind.is_dimension()) {
            let Some(place) = dim_place(s, c.kind, &c.refs) else { continue };
            // The dimension line's ends (or the arc's), and where the label goes.
            let (a, b, mid) = match place {
                DimPlace::Linear(p, q) => {
                    let (a2, b2) = match c.kind {
                        Kind::DimHorizontal => ([p[0], p[1] + c.offset], [q[0], p[1] + c.offset]),
                        Kind::DimVertical => ([p[0] + c.offset, p[1]], [p[0] + c.offset, q[1]]),
                        _ => {
                            let len = (q[0] - p[0]).hypot(q[1] - p[1]).max(1e-9);
                            let n = [(q[1] - p[1]) / len, -(q[0] - p[0]) / len];
                            (add(p, n, c.offset), add(q, n, c.offset))
                        }
                    };
                    line(p, a2);
                    line(q, b2);
                    line(a2, b2);
                    (a2, b2, [(a2[0] + b2[0]) / 2.0, (a2[1] + b2[1]) / 2.0])
                }
                DimPlace::Angle(i, u, v) => {
                    let r = c.offset.abs().max(2.0);
                    let (a0, mut a1) = (u[1].atan2(u[0]), v[1].atan2(v[0]));
                    let mut sweep = a1 - a0;
                    if sweep > std::f64::consts::PI {
                        sweep -= std::f64::consts::TAU;
                    } else if sweep < -std::f64::consts::PI {
                        sweep += std::f64::consts::TAU;
                    }
                    a1 = a0 + sweep;
                    let pts: Vec<Pos2> = (0..=24).map(|k| self.w2s(add(i, [(a0 + sweep * k as f64 / 24.0).cos(), (a0 + sweep * k as f64 / 24.0).sin()], r))).collect();
                    painter.add(egui::Shape::line(pts, stroke));
                    let at = |t: f64| add(i, [t.cos(), t.sin()], r);
                    (at(a0), at(a1), at(a0 + sweep / 2.0))
                }
                DimPlace::Circle(m, r) => {
                    let d = [c.offset.to_radians().cos(), c.offset.to_radians().sin()];
                    let tip = add(m, d, r);
                    let from = if c.kind == Kind::DimDiameter { add(m, d, -r) } else { m };
                    line(from, tip);
                    (from, tip, add(m, d, r * 1.25))
                }
            };
            let (sa, sb) = (self.w2s(a), self.w2s(b));
            if matches!(place, DimPlace::Linear(..)) || c.kind == Kind::DimDiameter {
                arrow_head(painter, sa, sb, DIM);
            }
            arrow_head(painter, sb, sa, DIM);
            let galley = painter.layout_no_wrap(label(c, values.get(&c.name).copied()), FontId::proportional(13.0), DIM);
            let rect = Rect::from_center_size(self.w2s(mid) - vec2(0.0, galley.size().y / 2.0 + 2.0), galley.size()).expand(2.0);
            painter.rect_filled(rect, 2.0, Color32::from_white_alpha(220));
            painter.galley(rect.min + vec2(2.0, 2.0), galley, DIM);
            labels.push((c.id, rect));
        }
        self.dim_labels = labels;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Ref;

    #[test]
    fn labels() {
        let mut c = Constraint { id: 1, kind: Kind::DimAligned, refs: vec![Ref::Item { item: 1 }], name: "d1".into(), expr: "40".into(), offset: 0.0 };
        assert_eq!(label(&c, Some(40.0)), "d1=40");
        c.expr = "d2 / 2".into();
        assert_eq!(label(&c, Some(12.5)), "d1=d2 / 2 (12.5)");
        c.kind = Kind::DimAngular;
        c.expr = "90".into();
        assert_eq!(label(&c, Some(90.0)), "d1=90°");
    }
}
