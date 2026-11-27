//! Dimensional constraints on the canvas: drawn as dimensions with their `name=value` label
//! (`d1=40`, `d2=d1/2 (20)`). Double-clicking a label edits its value; dragging it moves it.

use super::App;
use super::canvas::arrow_head;
use crate::model::constraints::expr::fmt_num;
use crate::model::constraints::parameter_values;
use super::select::Drag;
use crate::model::constraints::dim_label::{arc_angles, dim_line, label_point, label_through};
use crate::model::constraints::pick::{DimPlace, dim_place};
use crate::model::geom::distance;
use crate::model::{Constraint, Document, Kind, Point, State};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, vec2};


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

    /// Grabbing dimension `id`'s label at `at`: the drag that moves it.
    pub(super) fn grab_label(&self, d: &Document, id: u64, at: Point) -> Option<Drag> {
        let c = d.constraint(id)?;
        let anchor = label_point(dim_place(&d.s, c.kind, &c.refs)?, c.kind, c.offset, c.shift);
        Some(Drag::Label { id, grab: [at[0] - anchor[0], at[1] - anchor[1]] })
    }

    /// Move dimension `id`'s label to `at`: along its line and away from the geometry, or around
    /// the arc, or along and around the leader.
    pub(super) fn drag_label(&self, id: u64, at: Point) {
        let mut d = self.doc.lock();
        let Some(place) = d.constraint(id).and_then(|c| dim_place(&d.s, c.kind, &c.refs)) else { return };
        let Some(c) = d.s.constraints.iter_mut().find(|c| c.id == id) else { return };
        (c.offset, c.shift) = label_through(place, c.kind, at);
        d.touch();
    }

    pub(super) fn paint_dimensions(&mut self, painter: &egui::Painter, s: &State) {
        let values = parameter_values(s).unwrap_or_default();
        let labels = s.constraints.iter().filter(|c| c.kind.is_dimension()).filter_map(|c| Some((c.id, self.paint_lock_dimension(painter, s, c, values.get(&c.name).copied(), crate::gui::theme::get().dimension)?))).collect();
        self.dim_labels = labels;
        self.paint_dim_preview(painter, s);
    }

    /// One dimensional constraint, in `color`; returns its label's rectangle.
    pub(in crate::gui) fn paint_lock_dimension(&self, painter: &egui::Painter, s: &State, c: &Constraint, value: Option<f64>, color: Color32) -> Option<Rect> {
        let stroke = Stroke::new(1.0, color);
        let line = |a: Point, b: Point| painter.line_segment([self.w2s(a), self.w2s(b)], stroke);
        let place = dim_place(s, c.kind, &c.refs)?;
        let mid = label_point(place, c.kind, c.offset, c.shift);
        // The dimension line's ends (or the arc's); a label moved past them gets the line drawn out to it.
        let (a, b) = match place {
            DimPlace::Linear(p, q) => {
                let (a2, b2) = dim_line(p, q, c.kind, c.offset);
                line(p, a2);
                line(q, b2);
                line(a2, b2);
                let (from, to) = if distance(mid, a2) < distance(mid, b2) { (a2, b2) } else { (b2, a2) };
                if distance(mid, to) > distance(from, to) {
                    line(from, mid);
                }
                (a2, b2)
            }
            DimPlace::Angle(i, u, v) => {
                let r = c.offset.abs().max(2.0);
                let (a0, sweep) = arc_angles(u, v);
                let at = |t: f64| add(i, [t.cos(), t.sin()], r);
                let pts: Vec<Pos2> = (0..=24).map(|k| self.w2s(at(a0 + sweep * k as f64 / 24.0))).collect();
                painter.add(egui::Shape::line(pts, stroke));
                (at(a0), at(a0 + sweep))
            }
            DimPlace::Circle(m, r) => {
                let d = [c.offset.to_radians().cos(), c.offset.to_radians().sin()];
                let tip = add(m, d, r);
                let from = if c.kind == Kind::DimDiameter { add(m, d, -r) } else { m };
                line(from, if distance(m, mid) > r { mid } else { tip });
                (from, tip)
            }
        };
        let (sa, sb) = (self.w2s(a), self.w2s(b));
        if matches!(place, DimPlace::Linear(..)) || c.kind == Kind::DimDiameter {
            arrow_head(painter, sa, sb, color);
        }
        arrow_head(painter, sb, sa, color);
        let galley = painter.layout_no_wrap(label(c, value), FontId::proportional(13.0), color);
        let rect = Rect::from_center_size(self.w2s(mid) - vec2(0.0, galley.size().y / 2.0 + 2.0), galley.size()).expand(2.0);
        painter.rect_filled(rect, 2.0, Color32::from_white_alpha(220));
        painter.galley(rect.min + vec2(2.0, 2.0), galley, color);
        Some(rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Ref;

    #[test]
    fn labels() {
        let mut c = Constraint { id: 1, kind: Kind::DimAligned, refs: vec![Ref::Item { item: 1 }], name: "d1".into(), expr: "40".into(), offset: 0.0, shift: 0.0 };
        assert_eq!(label(&c, Some(40.0)), "d1=40");
        c.expr = "d2 / 2".into();
        assert_eq!(label(&c, Some(12.5)), "d1=d2 / 2 (12.5)");
        c.kind = Kind::DimAngular;
        c.expr = "90".into();
        assert_eq!(label(&c, Some(90.0)), "d1=90°");
    }
}
