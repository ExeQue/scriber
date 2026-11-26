//! Laser: the machine and material, export, and the machines & materials library. The layer
//! (operation) and the preview/check panels are used often, so they sit on the Home tab (built here).

use super::{App, Icon, panel};
use crate::gui::labels::{OPEN_SCORED, op_help, op_label, rgb};
use crate::gui::library_window::{Editor, pickers};
use crate::model::Op;
use eframe::egui::Ui;

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

    /// The layer of new or selected shapes, which sets what the laser does (Home tab): picks the
    /// current layer, or moves the selection to a layer.
    pub(super) fn operation_panel(&mut self, ui: &mut Ui) {
        let doc = self.doc.clone();
        let d = doc.lock().unwrap();
        let sel = d.selection.clone();
        let items: Vec<(u64, bool)> = d.s.items.iter().filter(|i| sel.contains(&i.id)).map(|i| (i.layer, i.shape.has_area())).collect();
        // Show the selection's layer when they agree; otherwise the current layer for new shapes.
        let layer = match items.first() {
            None => Some(d.current_layer()),
            Some(first) => Some(first.0).filter(|l| items.iter().all(|i| i.0 == *l)),
        };
        let mut set_layer = None;
        let title = if items.is_empty() { "Layer for new shapes".to_string() } else { format!("Layer of {} selected", items.len()) };
        let open_shapes = items.iter().any(|i| !i.1);
        panel(ui, &title, |ui| {
            for l in &d.s.layers {
                let mut tip = format!("{}\n{}: {}", l.name, op_label(l.op), op_help(l.op));
                if l.op == Op::Engrave && open_shapes {
                    tip = format!("{tip}\n{OPEN_SCORED}");
                }
                if super::button(ui, true, false, layer == Some(l.id), Icon::Swatch(rgb(l.rgb())), &l.name).on_hover_text(tip).clicked() {
                    set_layer = Some(l.id);
                }
            }
        });
        drop(d);
        let Some(set_layer) = set_layer else { return };
        let mut d = doc.lock().unwrap();
        if items.is_empty() {
            d.set_current_layer(set_layer);
            return;
        }
        d.checkpoint();
        d.move_to_layer(&sel, set_layer).ok();
    }
}
