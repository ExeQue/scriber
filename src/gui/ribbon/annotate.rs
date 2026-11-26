//! Annotate: text, dimensions, measuring and guide lines.

use super::{App, Icon, panel};
use crate::gui::command::Cmd;
use eframe::egui::Ui;

impl App {
    pub(super) fn annotate(&mut self, ui: &mut Ui) {
        panel(ui, "Text and sizes", |ui| {
            self.large(ui, Some(Cmd::Text), "Text");
            self.large(ui, Some(Cmd::DimLinear), "Note dimension");
            self.large(ui, Some(Cmd::Dist), "Measure");
        });
        panel(ui, "Guides", |ui| {
            self.smalls(ui, &[(Some(Cmd::Xline), "Guide line"), (Some(Cmd::Ray), "Guide ray"), (Some(Cmd::Centerline), "Centre line"), (Some(Cmd::Centermark), "Centre mark")]);
            ui.vertical(|ui| {
                if self.small(ui, self.show_guides, Icon::Glyph("👁"), "Show guide lines", "Show or hide the guide lines (they are never cut)") {
                    self.show_guides = !self.show_guides;
                }
            });
        });
    }
}
