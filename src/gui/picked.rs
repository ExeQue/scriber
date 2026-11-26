//! What the running command has picked so far (the first object of FILLET, the lines of a
//! constraint, …): each picked shape glows in an accent colour with a numbered badge at the spot
//! that was clicked, until the command ends.

use super::App;
use crate::model::Point;
use eframe::egui::{self, Align2, Color32, FontId, Stroke};

const PICKED: Color32 = Color32::from_rgb(0, 170, 255);

pub struct Picked {
    pub item: u64,
    pub at: Point,
}

impl App {
    /// Remember a pick of the running command (shown until the command ends).
    pub(in crate::gui) fn mark_picked(&mut self, item: u64, at: Point) {
        self.picked.push(Picked { item, at });
    }

    pub(in crate::gui) fn clear_picked(&mut self) {
        self.picked.clear();
    }

    pub(super) fn paint_picked(&mut self, painter: &egui::Painter, tol: f64) {
        if self.picked.is_empty() {
            return;
        }
        let doc = self.doc.clone();
        let d = doc.lock().unwrap();
        let (offset, zoom) = (self.canvas_rect.min + self.view.offset, self.view.zoom);
        let to_screen = |p: &Point| offset + egui::vec2(p[0] as f32, p[1] as f32) * zoom;
        for (n, pick) in self.picked.iter().enumerate() {
            if let Some(item) = d.item(pick.item) {
                for line in &self.geometry.get(item, false, tol).outline {
                    painter.add(egui::Shape::line(line.iter().map(to_screen).collect(), Stroke::new(5.0, PICKED.gamma_multiply(0.55))));
                }
            }
            let at = to_screen(&pick.at);
            painter.circle(at, 8.0, PICKED, Stroke::new(1.0, Color32::WHITE));
            painter.text(at, Align2::CENTER_CENTER, format!("{}", n + 1), FontId::proportional(11.0), Color32::WHITE);
        }
    }
}
