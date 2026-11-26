//! Arrange: reshaping lines, groups, what is shown and selecting.

use super::{App, panel};
use crate::gui::command::Cmd;
use eframe::egui::Ui;

impl App {
    pub(super) fn arrange(&mut self, ui: &mut Ui) {
        panel(ui, "Reshape", |ui| {
            self.smalls(
                ui,
                &[
                    (Some(Cmd::Stretch), "Stretch"),
                    (Some(Cmd::Join), "Join"),
                    (Some(Cmd::Explode), "Explode"),
                    (Some(Cmd::Align), "Align"),
                    (Some(Cmd::Break), "Break"),
                    (Some(Cmd::Extend), "Extend"),
                    (Some(Cmd::Lengthen), "Lengthen"),
                    (Some(Cmd::MatchProp), "Copy laser settings"),
                ],
            )
        });
        panel(ui, "Group", |ui| self.smalls(ui, &[(Some(Cmd::Group), "Group"), (Some(Cmd::Ungroup), "Ungroup")]));
        panel(ui, "Show", |ui| self.smalls(ui, &[(Some(Cmd::Isolate), "Show only selection"), (Some(Cmd::Unisolate), "Show all")]));
        panel(ui, "Select", |ui| self.smalls(ui, &[(Some(Cmd::SelectSimilar), "Select similar"), (Some(Cmd::QuickSelect), "Quick select")]));
    }
}
