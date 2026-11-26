//! Laser: the machine and material, export, and the machines & materials library. The operation
//! and the preview/check panels are used often, so they sit on the Home tab (built here).

use super::{App, Icon, panel};
use crate::gui::icons::pill;
use crate::gui::labels::{OPEN_SCORED, op_color, op_help, op_label};
use crate::gui::library_window::{Editor, pickers};
use crate::model::Op;
use eframe::egui::{RichText, Ui};

impl App {
    pub(super) fn laser(&mut self, ui: &mut Ui) {
        let doc = self.doc.clone();
        panel(ui, "Machine & material", |ui| pickers(ui, &mut doc.lock().unwrap()));
        panel(ui, "Export", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if self.small(ui, false, Icon::Glyph("💾"), "SVG for the laser", "Save the drawing as an SVG file for the laser software") {
                    self.export_svg();
                }
                if self.small(ui, false, Icon::Glyph("🖼"), "PNG picture", "Save a picture of the drawing") {
                    self.export_png();
                }
            });
        });
        panel(ui, "Library", |ui| {
            if self.small(ui, false, Icon::Glyph("⚙"), "Machines & materials…", "Your laser cutters and the power and speed to use for each material") {
                self.library = Some(Editor::new());
            }
        });
    }

    /// Preview and the pre-flight check (Home tab).
    pub(super) fn check_panel(&mut self, ui: &mut Ui) {
        let doc = self.doc.clone();
        let d = doc.lock().unwrap();
        panel(ui, "Laser", |ui| {
            ui.vertical(|ui| self.preview_and_check_buttons(ui, &d));
        });
    }

    /// What new or selected shapes do on the laser (Home tab).
    pub(super) fn operation_panel(&mut self, ui: &mut Ui) {
        let doc = self.doc.clone();
        let d = doc.lock().unwrap();
        let sel = d.selection.clone();
        let items: Vec<(Op, bool)> = d.s.items.iter().filter(|i| sel.contains(&i.id)).map(|i| (i.op, i.shape.has_area())).collect();
        // Show the selection's op when they agree; otherwise the default for new shapes.
        let op = match items.first() {
            None => Some(self.new_op),
            Some(first) => Some(first.0).filter(|o| items.iter().all(|i| i.0 == *o)),
        };
        let mut set_op = None;
        panel(ui, "Operation", |ui| {
            ui.vertical(|ui| {
                let target = if items.is_empty() { "New shapes".to_string() } else { format!("{} selected", items.len()) };
                ui.label(RichText::new(target).weak());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for o in Op::ALL {
                        if pill(ui, op == Some(o), Some(op_color(o)), op_label(o)).on_hover_text(op_help(o)).clicked() {
                            set_op = Some(o);
                        }
                    }
                });
                if items.iter().any(|i| i.0 == Op::Engrave && !i.1) {
                    ui.label(RichText::new(OPEN_SCORED).weak().small());
                }
            });
        });
        drop(d);
        let Some(set_op) = set_op else { return };
        if items.is_empty() {
            self.new_op = set_op;
            return;
        }
        let mut d = doc.lock().unwrap();
        d.checkpoint();
        for item in d.s.items.iter_mut().filter(|i| sel.contains(&i.id)) {
            item.op = set_op;
        }
    }
}
