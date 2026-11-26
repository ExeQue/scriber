//! Temporary overrides, as in AutoCAD: holding Shift while drawing flips ORTHO, and Shift+right-click
//! opens a one-shot object snap menu whose choice applies to the next point only. Besides the
//! snap kinds it offers "Mid between 2 points" and "From" (a base point, then an offset from it).

use super::App;
use super::canvas::tooltip;
use super::snap::{self, Kind};
use crate::model::Point;
use eframe::egui::{self, Color32, DragValue, Stroke};

/// The object snap override waiting for the next point.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Once {
    /// Only this kind of snap counts.
    Only(Kind),
    /// The point halfway between two picks (the first once picked).
    MidBetween(Option<Point>),
    /// A base point (once picked), then the point as an offset from it.
    From { base: Option<Point>, dx: f64, dy: f64 },
}

const KINDS: [Kind; 8] = [Kind::Endpoint, Kind::Midpoint, Kind::Center, Kind::Quadrant, Kind::Intersection, Kind::Perpendicular, Kind::Tangent, Kind::Nearest];

impl App {
    /// ORTHO as it applies now: Shift held while drawing flips it.
    pub(super) fn ortho_now(&self) -> bool {
        self.aids.ortho ^ (self.shift && self.active_cmd().is_some())
    }

    pub(super) fn snap_once(&self) -> Option<Kind> {
        match self.once {
            Some(Once::Only(k)) => Some(k),
            _ => None,
        }
    }

    /// The point a pick at `p` gives: halfway from the first pick for "Mid between 2 points".
    pub(super) fn override_point(&self, p: Point) -> Point {
        match self.once {
            Some(Once::MidBetween(Some(a))) => [(a[0] + p[0]) / 2.0, (a[1] + p[1]) / 2.0],
            _ => p,
        }
    }

    /// A point given while an override waits. True when it was only the override's own first pick
    /// (the first of two points, or the base point), so the command does not get it; otherwise the
    /// override is used up.
    pub(super) fn override_pick(&mut self, p: Point) -> bool {
        match &mut self.once {
            Some(Once::MidBetween(first @ None)) => {
                *first = Some(p);
                self.message = "Mid between 2 points: pick the second point.".into();
                true
            }
            Some(Once::From { base: base @ None, .. }) => {
                *base = Some(p);
                self.message = "From: click the point, or type its offset from the base point.".into();
                true
            }
            _ => {
                self.once = None;
                false
            }
        }
    }

    /// The Shift+right-click menu.
    pub(super) fn osnap_menu(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Snap the next point to").small().weak());
        let mut pick = None;
        for k in KINDS {
            if ui.button(k.label()).clicked() {
                pick = Some((Once::Only(k), format!("Next point: {} only.", k.label())));
            }
        }
        ui.separator();
        if ui.button("Mid between 2 points").clicked() {
            pick = Some((Once::MidBetween(None), "Mid between 2 points: pick the first point.".into()));
        }
        if ui.button("From (base point + offset)").clicked() {
            pick = Some((Once::From { base: None, dx: 0.0, dy: 0.0 }, "From: pick the base point.".into()));
        }
        if self.once.is_some() && ui.button("No override").clicked() {
            self.once = None;
            ui.close();
        }
        if let Some((once, msg)) = pick {
            self.once = Some(once);
            self.message = msg;
            ui.close();
        }
    }

    /// "From" with its base point picked: type the offset and place it.
    pub(super) fn override_window(&mut self, ctx: &egui::Context) {
        let Some(Once::From { base: Some(b), mut dx, mut dy }) = self.once else { return };
        let mut place = false;
        let at = self.w2s(b) + egui::vec2(16.0, 24.0);
        egui::Area::new(egui::Id::unique("from-offset")).fixed_pos(at).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new("Offset from the base point").small().weak());
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(&mut dx).speed(0.1).prefix("x ").suffix(" mm"));
                    ui.add(DragValue::new(&mut dy).speed(0.1).prefix("y ").suffix(" mm"));
                });
                place = ui.button("Place").clicked();
            });
        });
        self.once = Some(Once::From { base: Some(b), dx, dy });
        if place {
            self.place_point([b[0] + dx, b[1] + dy]);
        }
    }

    /// What a two-pick override has so far: the first point or base point, joined to the cursor.
    pub(super) fn paint_override(&self, painter: &egui::Painter, pointer: Point) {
        let (from, label) = match self.once {
            Some(Once::MidBetween(Some(a))) => (a, "Mid between 2 points".to_string()),
            Some(Once::From { base: Some(b), .. }) => (b, format!("From: {:.2}, {:.2}", pointer[0] - b[0], pointer[1] - b[1])),
            _ => return,
        };
        let stroke = Stroke::new(1.0, Color32::from_rgb(255, 170, 0));
        painter.extend(egui::Shape::dashed_line(&[self.w2s(from), self.w2s(pointer)], stroke, 4.0, 4.0));
        snap::paint_marker(painter, self.w2s(from), Kind::Endpoint);
        tooltip(painter, self.w2s(pointer) + egui::vec2(0.0, 20.0), &label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Document;
    use std::sync::{Arc, Mutex};

    #[test]
    fn mid_between_takes_two_picks() {
        let mut app = App::new(Arc::new(Mutex::new(Document::new(100.0, 100.0))), Err(String::new()));
        app.once = Some(Once::MidBetween(None));
        assert!(app.override_pick([0.0, 0.0]));
        assert_eq!(app.override_point([10.0, 4.0]), [5.0, 2.0]);
        assert!(!app.override_pick([5.0, 2.0]));
        assert_eq!(app.once, None);
        assert_eq!(app.override_point([10.0, 4.0]), [10.0, 4.0]);
    }
}
