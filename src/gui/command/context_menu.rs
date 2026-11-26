//! The right-click menu on the canvas, like AutoCAD's shortcut menu: while a command runs it
//! offers Enter, Cancel and the command's options; otherwise it offers the last command and
//! what can be done with the selection.

use super::card::{next_label, title};
use super::parse::Entry;
use super::{App, Cmd, Step};
use eframe::egui;

impl App {
    pub(in crate::gui) fn canvas_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(190.0);
        match self.cmdline.active {
            Some((cmd, step)) => self.command_menu(ui, cmd, step),
            None => self.idle_menu(ui),
        }
    }

    fn command_menu(&mut self, ui: &mut egui::Ui, cmd: Cmd, step: Step) {
        if ui.button(next_label(cmd, step)).clicked() {
            self.answer(Entry::Empty, None);
        }
        if ui.button("Cancel").clicked() {
            self.escape();
        }
        let placed = self.draft.len();
        let options: &[(&str, &str)] = match (cmd, step) {
            (Cmd::Pline, Step::Draw) if placed >= 3 => &[("Close shape", "CLOSE"), ("Undo last point", "UNDO")],
            (Cmd::Pline, Step::Draw) if placed >= 1 => &[("Undo last point", "UNDO")],
            (Cmd::Circle, Step::Draw) if placed == 1 => &[("Diameter", "DIAMETER")],
            (Cmd::Rectang, Step::Draw) if placed == 0 => &[("Rounded corners…", "FILLET")],
            (Cmd::Fillet | Cmd::Chamfer, Step::Pick) => &[("All corners of the selection", "ALL")],
            (Cmd::Mirror, Step::EraseSource) => &[("Keep the original", "NO"), ("Delete the original", "YES")],
            _ => &[],
        };
        if !options.is_empty() {
            ui.separator();
        }
        for (label, word) in options {
            if ui.button(*label).clicked() {
                self.answer(Entry::Word((*word).into()), None);
            }
        }
    }

    fn idle_menu(&mut self, ui: &mut egui::Ui) {
        if let Some(last) = self.cmdline.last
            && ui.button(format!("Repeat {}", title(last))).clicked()
        {
            self.start(last);
        }
        let selected = self.doc.lock().unwrap().selection.len();
        if selected > 0 {
            ui.separator();
            let mut edits = vec![Cmd::Move, Cmd::Copy, Cmd::Rotate, Cmd::Scale, Cmd::Mirror, Cmd::Offset, Cmd::Array, Cmd::Erase];
            if selected >= 2 {
                edits.extend([Cmd::Union, Cmd::Subtract, Cmd::Intersect]);
            }
            for cmd in edits {
                if ui.button(title(cmd)).clicked() {
                    self.start(cmd);
                }
            }
            if ui.button("Deselect all").clicked() {
                self.doc.lock().unwrap().selection.clear();
            }
        }
        ui.separator();
        if ui.button("Undo").clicked() {
            self.doc.lock().unwrap().undo();
        }
        if ui.button("Redo").clicked() {
            self.doc.lock().unwrap().redo();
        }
        if ui.button("Select all").clicked() {
            self.select_all();
        }
        if ui.button("Show whole material").clicked() {
            self.view.zoom_extents();
        }
    }
}
