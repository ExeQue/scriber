//! The ways a drawing tool can draw, for the ribbon's split buttons: their names, which one is
//! used now, and starting the tool in one of them.

use super::curves::{ArcMode, CircleMode};
use super::rects::RectMode;
use super::{App, Cmd};
use eframe::egui;

impl App {
    /// The ways `cmd` can draw and the one used now; empty for tools with only one way.
    pub(in crate::gui) fn variants(&self, cmd: Cmd) -> (Vec<&'static str>, usize) {
        let c = &self.cmdline;
        let find = |names: Vec<&'static str>, now: &'static str| {
            let n = names.iter().position(|x| *x == now).unwrap_or(0);
            (names, n)
        };
        match cmd {
            Cmd::Circle => find(CircleMode::ALL.map(CircleMode::label).to_vec(), c.circle_mode.label()),
            Cmd::Arc => find(ArcMode::ALL.map(ArcMode::label).to_vec(), c.arc_mode.label()),
            Cmd::Rectang => find(RectMode::ALL.map(RectMode::label).to_vec(), c.rect_mode.label()),
            Cmd::Polygon => (vec!["Size to a corner", "Size to a side"], c.polygon_flat as usize),
            _ => (Vec::new(), 0),
        }
    }

    /// The ways `cmd` draws as a list with icons (command card, right-click menu); picking one starts over.
    pub(in crate::gui) fn variant_list(&mut self, ui: &mut egui::Ui, cmd: Cmd) {
        let (names, chosen) = self.variants(cmd);
        for (n, name) in names.iter().enumerate() {
            let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(200.0), 24.0), egui::Sense::click());
            let v = ui.visuals();
            let (bg, fg) = match (n == chosen, r.hovered()) {
                (true, _) => (v.selection.bg_fill, v.strong_text_color()),
                (false, true) => (v.widgets.hovered.bg_fill, v.strong_text_color()),
                _ => (egui::Color32::TRANSPARENT, v.text_color()),
            };
            ui.painter().rect_filled(rect, 3.0, bg);
            let icon = egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 3.0), egui::vec2(18.0, 18.0));
            crate::gui::variant_icons::paint_variant(ui.painter(), icon, fg, cmd, n);
            ui.painter().text(egui::pos2(icon.right() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, *name, egui::TextStyle::Button.resolve(ui.style()), fg);
            if r.clicked() && n != chosen {
                self.set_variant(cmd, n);
            }
        }
    }

    /// Switch `cmd` to its `n`th way; the points placed so far are dropped.
    fn set_variant(&mut self, cmd: Cmd, n: usize) {
        match cmd {
            Cmd::Circle => self.set_circle_mode(CircleMode::ALL[n.min(2)]),
            Cmd::Arc => self.set_arc_mode(ArcMode::ALL[n.min(3)]),
            Cmd::Rectang => self.set_rect_mode(RectMode::ALL[n.min(2)]),
            Cmd::Polygon => self.cmdline.polygon_flat = n == 1,
            _ => {}
        }
    }

    /// Start `cmd` drawing in its `n`th way (and remember that way).
    pub(in crate::gui) fn start_variant(&mut self, cmd: Cmd, n: usize) {
        let c = &mut self.cmdline;
        match cmd {
            Cmd::Circle => c.circle_mode = CircleMode::ALL[n.min(2)],
            Cmd::Arc => c.arc_mode = ArcMode::ALL[n.min(3)],
            Cmd::Rectang => c.rect_mode = RectMode::ALL[n.min(2)],
            Cmd::Polygon => c.polygon_flat = n == 1,
            _ => {}
        }
        self.start(cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_starts_in_that_way() {
        let doc = std::sync::Arc::new(parking_lot::Mutex::new(crate::model::Document::new(100.0, 100.0)));
        let mut app = App::new(crate::model::Workspace::from_shared(doc), Err(String::new()));
        app.start_variant(Cmd::Arc, 2);
        assert_eq!(app.cmdline.arc_mode, ArcMode::CenterStartEnd);
        assert_eq!(app.variants(Cmd::Arc).1, 2);
        assert_eq!(app.active_cmd(), Some(Cmd::Arc));
        app.start_variant(Cmd::Polygon, 1);
        assert!(app.cmdline.polygon_flat);
    }
}
