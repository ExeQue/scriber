//! What the pointer is over, before any click: the shape a click would pick glows, and after a
//! short rest a tooltip tells its name, laser operation, size and constraints. Where shapes
//! overlap a badge offers Shift+Space to cycle. With no command running, holding Ctrl (or the View tab's
//! Measure on hover) measures what is under the cursor: a segment's length, an arc's radius, or the
//! distance between the last two snap points pointed at.

use super::App;
use super::canvas::{APERTURE, tooltip};
use super::labels::op_label;
use super::snap::{self, Seg};
use super::tools::Tool;
use super::tracking::angle;
use crate::model::geom::distance;
use crate::model::{Item, Point, Shape, State};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, vec2};
use std::time::{Duration, Instant};

/// How long the pointer rests on a shape before its tooltip shows.
const TOOLTIP_DELAY: Duration = Duration::from_millis(600);
const GLOW: Color32 = Color32::from_rgb(90, 170, 255);

/// A word for what kind of shape an item is.
fn shape_word(item: &Item) -> &'static str {
    match &item.shape {
        Shape::Path { contours } if contours.len() == 1 && contours[0].as_circle().is_some() => "Circle",
        Shape::Path { contours } if contours.iter().all(|c| c.closed) => "Closed shape",
        Shape::Path { .. } => "Line",
        Shape::Text { .. } => "Text",
        Shape::Image { .. } => "Image",
        Shape::Dimension { .. } => "Dimension",
        Shape::Guide(_) => "Guide line",
    }
}

/// The rollover tooltip's lines for `item`.
fn describe(item: &Item, s: &State) -> Vec<String> {
    let m = item.shape.metrics();
    let name = if item.name.is_empty() { shape_word(item).to_string() } else { format!("{} ({})", item.name, shape_word(item)) };
    let mut lines = vec![name, format!("Laser: {}", op_label(item.effective_op())), format!("Size: {:.2} × {:.2} mm", m.bbox[2], m.bbox[3])];
    let mut measures = vec![format!("length {:.2} mm", m.length)];
    if let Some(a) = m.area {
        measures.push(format!("area {a:.2} mm²"));
    }
    lines.push(measures.join(", "));
    let n = s.constraints.iter().filter(|c| c.refs.iter().any(|r| r.item() == item.id)).count();
    if n > 0 {
        lines.push(format!("Constraints: {n}"));
    }
    lines
}

/// A box of text lines at `pos`.
fn label_box(painter: &egui::Painter, pos: Pos2, lines: &[String]) {
    let galley = painter.layout_no_wrap(lines.join("\n"), FontId::proportional(12.0), Color32::WHITE);
    painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(4.0), 4.0, Color32::from_black_alpha(210));
    painter.galley(pos, galley, Color32::WHITE);
}

impl App {
    /// The shape a click would pick now changed (or not): restart the tooltip delay.
    pub(super) fn update_hover(&mut self, id: Option<u64>) {
        if self.hover.map(|h| h.0) != id {
            self.hover = id.map(|i| (i, Instant::now()));
        }
    }

    /// The glow on the hovered shape, its tooltip and the cycling badge.
    pub(super) fn paint_hover(&self, painter: &egui::Painter, s: &State, pointer: Option<Point>) {
        let (Some((id, since)), Some(p)) = (self.hover, pointer) else { return };
        if self.tool != Tool::Select || self.drag.is_some() {
            return;
        }
        let Some(item) = s.items.iter().find(|i| i.id == id) else { return };
        let tol = (0.5 / self.view.zoom as f64).clamp(0.002, 1.0);
        for line in item.shape.outline(tol) {
            let pts: Vec<Pos2> = line.iter().map(|q| self.w2s(*q)).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(5.0, GLOW.gamma_multiply(0.35))));
        }
        let at = self.w2s(p);
        if self.cycle.count() >= 2 {
            let n = self.cycle.count();
            tooltip(painter, at + vec2(0.0, -40.0), &format!("{n} shapes here — Shift+Space to cycle"));
        }
        let waited = since.elapsed();
        if waited >= TOOLTIP_DELAY {
            label_box(painter, at + vec2(18.0, 40.0), &describe(item, s));
        } else {
            painter.ctx().request_repaint_after(TOOLTIP_DELAY - waited);
        }
    }

    /// Measure on hover: with no command running and Ctrl held (or the View tab toggle on).
    pub(super) fn paint_hover_measure(&mut self, painter: &egui::Painter, items: &[Item], pointer: Option<Point>) {
        let ctrl = painter.ctx().input(|i| i.modifiers.command);
        let Some(p) = pointer.filter(|_| (ctrl || self.measure_hover) && self.active_cmd().is_none() && self.drag.is_none()) else {
            self.measure_pts = (None, None);
            return;
        };
        let tol = (APERTURE / self.view.zoom) as f64;
        let guides = self.guides_in_view(items);
        let at = self.w2s(p);
        let colour = Color32::from_rgb(230, 0, 160);
        if let Some(s) = snap::find(items, &guides, p, tol, None).filter(|s| s.kind != snap::Kind::Nearest) {
            if self.measure_pts.1 != Some(s.p) {
                self.measure_pts = (self.measure_pts.1, Some(s.p));
            }
            snap::paint_marker(painter, self.w2s(s.p), s.kind);
            if let (Some(a), Some(b)) = self.measure_pts {
                painter.line_segment([self.w2s(a), self.w2s(b)], Stroke::new(1.5, colour));
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                label_box(painter, at + vec2(18.0, 18.0), &[format!("Distance {:.2} mm", distance(a, b)), format!("Δx {dx:.2}  Δy {dy:.2}  {:.1}°", angle(a, b))]);
            } else {
                label_box(painter, at + vec2(18.0, 18.0), &[format!("{}: point at another to measure the distance", s.kind.label())]);
            }
            return;
        }
        let Some((seg, _)) = snap::pieces_near(items, &guides, p, tol).into_iter().next() else { return };
        let lines = match seg {
            Seg::Line(a, b) => {
                painter.line_segment([self.w2s(a), self.w2s(b)], Stroke::new(3.0, colour));
                vec![format!("Length {:.2} mm", distance(a, b)), format!("Angle {:.1}°", angle(a, b))]
            }
            Seg::Arc { c, r, sweep, .. } => {
                painter.circle_stroke(self.w2s(c), 3.0, Stroke::new(1.5, colour));
                vec![format!("Radius {r:.2} mm  (Ø {:.2})", 2.0 * r), format!("Arc length {:.2} mm, {:.1}°", r * sweep.abs(), sweep.abs().to_degrees())]
            }
        };
        label_box(painter, at + vec2(18.0, 18.0), &lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Contour, Document, Op};

    #[test]
    fn tooltip_lines() {
        let mut d = Document::new(100.0, 100.0);
        let id = d.add("Lid", Op::Cut, Shape::path(Contour::rect(0.0, 0.0, 20.0, 10.0, 0.0)));
        let lines = describe(d.item(id).unwrap(), &d.s);
        assert_eq!(lines[0], "Lid (Closed shape)");
        assert_eq!(lines[2], "Size: 20.00 × 10.00 mm");
        assert_eq!(lines[3], "length 60.00 mm, area 200.00 mm²");
    }
}
