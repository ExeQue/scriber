//! File > Import DXF…: pick what the laser does with each DXF layer, then import (one undo step).
//! The choices are remembered per layer name for the next DXF.

use super::App;
use super::labels::{op_label, rgb};
use super::tools::Tool;
use crate::export::dxf::import::{self, Dxf};
use crate::model::{Op, Point};
use eframe::egui;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Largest distance in mm between an ellipse or spline and the arcs that replace it.
const TOLERANCE: f64 = 0.01;

/// The open Import DXF dialog.
pub(super) struct DxfDialog {
    path: PathBuf,
    dxf: Dxf,
    /// Per group: the op, or None to leave it out.
    ops: Vec<Option<Op>>,
    /// Put the drawing's top-left corner at (x, y) instead of where the file has it.
    place: bool,
    x: f64,
    y: f64,
}

impl App {
    pub(super) fn import_dxf_dialog(&mut self, ctx: &egui::Context) {
        self.dialog.pick(ctx, super::Want::ImportDxf, rfd::FileDialog::new().add_filter("DXF", &["dxf"]));
    }

    /// The DXF file was chosen (or dropped at `at`): read it and ask what to do with its layers.
    pub(super) fn dxf_file_chosen(&mut self, path: PathBuf, at: Option<Point>) {
        let height = self.doc.lock().s.height;
        let read = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| import::read(&b, height, TOLERANCE));
        match read {
            Ok(dxf) => {
                let ops = dxf.groups.iter().map(|g| self.dxf_ops.get(&g.name).copied().unwrap_or(g.guess)).collect();
                let [x, y] = at.unwrap_or([10.0, 10.0]);
                self.dxf_dialog = Some(DxfDialog { path, dxf, ops, place: at.is_some(), x, y });
            }
            Err(e) => self.message = format!("Could not import the DXF: {e}"),
        }
    }

    pub(super) fn dxf_import_window(&mut self, ctx: &egui::Context) {
        let Some(s) = &mut self.dxf_dialog else { return };
        let (mut open, mut import) = (true, false);
        let file = s.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        egui::Window::new("Import DXF").id(self.popouts.id("dxf")).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(file);
            ui.label(format!("Units: {}", s.dxf.units)).on_hover_text("From the file's $INSUNITS. A file without units is read as millimetres.");
            ui.add_space(4.0);
            egui::Grid::new("dxf-layers").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                for (k, g) in s.dxf.groups.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, rgb(g.rgb));
                        ui.painter().rect_stroke(rect, 2.0, (1.0, egui::Color32::GRAY), egui::StrokeKind::Inside);
                        ui.label(&g.name);
                    });
                    ui.label(format!("{} items", g.shapes.len()));
                    let text = s.ops[k].map_or("Skip", op_label);
                    egui::ComboBox::from_id_salt(("dxf-op", k)).width(90.0).selected_text(text).show_ui(ui, |ui| {
                        for op in Op::ALL {
                            ui.selectable_value(&mut s.ops[k], Some(op), op_label(op));
                        }
                        ui.selectable_value(&mut s.ops[k], None, "Skip").on_hover_text("Leave this layer out");
                    });
                    ui.end_row();
                }
            });
            if !s.dxf.skipped.is_empty() {
                let left: Vec<String> = s.dxf.skipped.iter().map(|(what, n)| format!("{what} ×{n}")).collect();
                ui.label(format!("Left out: {}", left.join(", ")));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.place, "Place at").on_hover_text("Off: keep the position from the file (its origin at the material's bottom-left corner)");
                ui.add_enabled_ui(s.place, |ui| {
                    ui.add(egui::DragValue::new(&mut s.x).speed(0.5).max_decimals(3).prefix("x ").suffix(" mm"));
                    ui.add(egui::DragValue::new(&mut s.y).speed(0.5).max_decimals(3).prefix("y ").suffix(" mm"));
                });
            });
            ui.add_space(6.0);
            import = ui.button("Import").clicked();
        });
        if !import {
            if !open {
                self.dxf_dialog = None;
            }
            return;
        }
        let Some(s) = self.dxf_dialog.take() else { return };
        let ops: BTreeMap<String, Option<Op>> = s.dxf.groups.iter().zip(&s.ops).map(|(g, op)| (g.name.clone(), *op)).collect();
        let at = s.place.then_some([s.x, s.y]);
        let result = {
            let mut d = self.doc.lock();
            let before = d.s.clone();
            d.import_dxf(&s.dxf, &ops, at).inspect(|_| d.push_undo(before))
        };
        self.message = match result {
            Ok(ids) => {
                self.set_tool(Tool::Select);
                format!("Imported {} items from {}.", ids.len(), s.path.display())
            }
            Err(e) => format!("Could not import the DXF: {e}"),
        };
        self.dxf_ops.extend(ops);
    }
}
