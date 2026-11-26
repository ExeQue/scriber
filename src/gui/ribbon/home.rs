//! Home: drawing, changing and combining shapes, undo.

use super::{App, panel};
use crate::gui::command::Cmd;
use eframe::egui::Ui;

impl App {
    pub(super) fn home(&mut self, ui: &mut Ui) {
        panel(ui, "Draw", |ui| {
            self.large(ui, Some(Cmd::Line), "Line");
            self.large(ui, Some(Cmd::Pline), "Polyline");
            for (cmd, label) in [(Cmd::Rectang, "Rectangle"), (Cmd::Circle, "Circle"), (Cmd::Arc, "Arc")] {
                self.split(ui, cmd, label);
            }
            self.large(ui, Some(Cmd::Polygon), "Polygon");
            self.smalls(ui, &[(Some(Cmd::Text), "Text"), (None, "Select")]);
        });
        panel(ui, "Modify", |ui| {
            for (cmd, label) in [(Cmd::Move, "Move"), (Cmd::Copy, "Copy"), (Cmd::Rotate, "Rotate")] {
                self.large(ui, Some(cmd), label);
            }
            self.smalls(
                ui,
                &[
                    (Some(Cmd::Scale), "Scale"),
                    (Some(Cmd::Mirror), "Mirror"),
                    (Some(Cmd::Fillet), "Fillet"),
                    (Some(Cmd::Chamfer), "Chamfer"),
                    (Some(Cmd::Offset), "Offset"),
                    (Some(Cmd::Trim), "Trim"),
                    (Some(Cmd::Array), "Array"),
                    (Some(Cmd::Erase), "Delete"),
                ],
            );
        });
        panel(ui, "Combine", |ui| self.smalls(ui, &[(Some(Cmd::Union), "Merge"), (Some(Cmd::Subtract), "Cut out"), (Some(Cmd::Intersect), "Keep overlap")]));
        self.operation_panel(ui);
        self.check_panel(ui);
        panel(ui, "Undo", |ui| self.smalls(ui, &[(Some(Cmd::Undo), "Undo"), (Some(Cmd::Redo), "Redo")]));
    }
}
