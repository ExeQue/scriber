//! Home: drawing, changing and combining shapes, undo.

use super::{App, panel};
use crate::gui::command::Cmd;
use eframe::egui::Ui;

impl App {
    pub(super) fn home(&mut self, ui: &mut Ui) {
        panel(ui, "Draw", |ui| {
            for (cmd, label) in [(Cmd::Line, "Line"), (Cmd::Pline, "Polyline"), (Cmd::Rectang, "Rectangle"), (Cmd::Circle, "Circle"), (Cmd::Arc, "Arc")] {
                self.large(ui, Some(cmd), label);
            }
            self.smalls(ui, &[(Some(Cmd::Polygon), "Polygon"), (Some(Cmd::Text), "Text"), (None, "Select")]);
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
        panel(ui, "Undo", |ui| self.smalls(ui, &[(Some(Cmd::Undo), "Undo"), (Some(Cmd::Redo), "Redo")]));
    }
}
