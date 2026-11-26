//! View: zoom, the laser preview and what the canvas shows.

use super::{App, Icon, panel};
use crate::gui::command::Cmd;
use crate::gui::shortcuts::{self, Action};
use eframe::egui::Ui;

impl App {
    pub(super) fn view_tab(&mut self, ui: &mut Ui, editing: bool) {
        panel(ui, "Zoom", |ui| {
            ui.add_enabled_ui(editing, |ui| self.large(ui, Some(Cmd::Zoom), "Window"));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if self.small(ui, false, Icon::Glyph("⛶"), "Whole material", &format!("Show the whole material\nShortcut: {}", shortcuts::of(Action::ZoomAll).unwrap_or("-"))) {
                    self.view.zoom_extents();
                }
                if self.small(ui, false, Icon::Glyph("⟲"), "Previous view", "Go back to the view before") {
                    self.zoom_previous();
                }
                if self.small(ui, false, Icon::Glyph("🔍"), "Selection", "Zoom to the selected shapes") {
                    self.zoom_selection();
                }
            });
        });
        panel(ui, "Laser", |ui| {
            let key = shortcuts::preview_key();
            if self.small(ui, self.preview.is_some(), Icon::Glyph("👁"), "Preview", &format!("Show only what the laser makes\nShortcut: {key}")) {
                self.toggle_preview();
            }
        });
        panel(ui, "Show", |ui| {
            let toggles: [(&mut bool, &str, &str, &str); 3] = [
                (&mut self.aids.grid, "#", "Grid", "Show the grid (F7)"),
                (&mut self.show_guides, "👁", "Guide lines", "Show the guide lines (they are never cut)"),
                (&mut self.measure_hover, "📏", "Measure on hover", "Show lengths and sizes under the pointer (or hold Ctrl)"),
            ];
            let compact = self.ribbon.compact;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (on, glyph, label, tip) in toggles {
                    if super::button(ui, false, compact, *on, Icon::Glyph(glyph), label).on_hover_text(format!("{label}\n{tip}")).clicked() {
                        *on = !*on;
                    }
                }
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (on, glyph, label, tip) in [
                    (&mut self.snap, "⊞", "Snap to grid", "Round points to the grid"),
                    (&mut self.show_cmdline, ">_", "Command line", "Type AutoCAD commands (for CAD users)"),
                ] {
                    if super::button(ui, false, compact, *on, Icon::Glyph(glyph), label).on_hover_text(format!("{label}\n{tip}")).clicked() {
                        *on = !*on;
                    }
                }
                if super::button(ui, false, compact, !self.comments.hidden, Icon::Glyph(crate::gui::lucide::MESSAGE_SQUARE), "Comments").on_hover_text("Comments\nShow the review comment pins").clicked() {
                    self.comments.hidden = !self.comments.hidden;
                }
            });
        });
        panel(ui, "Theme", |ui| {
            ui.menu_button("Theme ▾", |ui| self.theme.menu(ui)).response.on_hover_text("Colours: follow Omarchy or the system, light, dark and more");
        });
    }
}
