//! Annotate: text, dimensions, measuring, guide lines and review comments.

use super::{App, Icon, button, panel};
use crate::gui::lucide;
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
        panel(ui, "Review", |ui| {
            if button(ui, true, false, self.comments.placing, Icon::Glyph(lucide::MESSAGE_SQUARE), "Comment").on_hover_text("Comment\nPin a comment on a spot or a shape, for an agent (or a person) to act on").clicked() {
                self.toggle_comment_tool();
            }
            ui.vertical(|ui| {
                if self.small(ui, !self.comments.hidden, Icon::Glyph(lucide::EYE), "Show comments", "Show or hide the comment pins") {
                    self.comments.hidden = !self.comments.hidden;
                }
            });
        });
    }
}
