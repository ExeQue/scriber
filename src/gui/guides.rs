//! Painting guide lines (XLINE, RAY, CENTERLINE, CENTERMARK): dash-dot in a guide colour, clipped to
//! the part of the drawing in view. Also which items are shown at all (guides on/off, ISOLATE).

use super::App;
use crate::model::{BBox, Guide, Item, Shape};
use eframe::egui::{self, Color32, Stroke};

/// Guide lines are drawn in this colour, which no laser operation uses.
const GUIDE: Color32 = Color32::from_rgb(0, 170, 200);
const SELECTED: Color32 = Color32::from_rgb(255, 150, 0);

impl App {
    /// Whether the item is drawn and pickable: guides can be hidden, and ISOLATE hides everything else.
    pub(super) fn visible(&self, item: &Item) -> bool {
        (self.show_guides || !matches!(item.shape, Shape::Guide(_))) && self.isolated.as_ref().is_none_or(|ids| ids.contains(&item.id))
    }

    /// The part of the material (mm) the canvas shows.
    pub(super) fn view_box(&self) -> BBox {
        let (a, b) = (self.s2w(self.canvas_rect.min), self.s2w(self.canvas_rect.max));
        [a[0], a[1], b[0] - a[0], b[1] - a[1]]
    }

    pub(super) fn paint_guide(&self, painter: &egui::Painter, g: &Guide, selected: bool) {
        let stroke = Stroke::new(1.0, GUIDE);
        for [a, b] in g.segments(self.view_box()) {
            let (a, b) = (self.w2s(a), self.w2s(b));
            if selected {
                painter.line_segment([a, b], Stroke::new(6.0, SELECTED.gamma_multiply(0.45)));
            }
            // Dash-dot, like a centre line on a drawing: long dash, gap, dot, gap.
            let len = (b - a).length();
            let dir = (b - a) / len.max(1e-3);
            let mut t = 0.0;
            while t < len {
                painter.line_segment([a + dir * t, a + dir * (t + 12.0).min(len)], stroke);
                let dot = t + 15.0;
                if dot < len {
                    painter.line_segment([a + dir * dot, a + dir * (dot + 1.5).min(len)], stroke);
                }
                t += 19.5;
            }
        }
    }
}
