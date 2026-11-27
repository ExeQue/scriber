//! File > Import > SVG…: a small dialog for placement and size, and the import itself (one undo step).

use super::App;
use super::tools::Tool;
use crate::export::svg_import::ImportOptions;
use crate::model::Point;
use eframe::egui;
use std::path::{Path, PathBuf};

/// The settings in the Import SVG dialog.
pub(super) struct SvgDialog {
    path: PathBuf,
    /// Put the drawing at (x, y) instead of where the file puts it.
    place: bool,
    x: f64,
    y: f64,
    /// Scale to `width` instead of by `scale`.
    fit: bool,
    width: f64,
    scale: f64,
    tolerance: f64,
}

fn mm(v: &mut f64) -> egui::DragValue<'_> {
    egui::DragValue::new(v).speed(0.5).max_decimals(3).suffix(" mm")
}

impl App {
    pub(super) fn import_svg_dialog(&mut self, ctx: &egui::Context) {
        self.dialog.pick(ctx, super::Want::ImportSvg, rfd::FileDialog::new().add_filter("SVG", &["svg", "svgz"]));
    }

    /// The SVG file was chosen: ask how to place it.
    pub(super) fn svg_file_chosen(&mut self, path: std::path::PathBuf) {
        let d = ImportOptions::default();
        self.svg_dialog = Some(SvgDialog { path, place: false, x: 10.0, y: 10.0, fit: false, width: 100.0, scale: d.scale, tolerance: d.tolerance });
    }

    pub(super) fn svg_import_window(&mut self, ctx: &egui::Context) {
        let Some(s) = &mut self.svg_dialog else { return };
        let (mut open, mut import) = (true, false);
        let file = s.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        egui::Window::new("Import SVG").id(self.popouts.id("svg")).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(file);
            ui.add_space(4.0);
            egui::Grid::new("svg-import").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.checkbox(&mut s.place, "Place at").on_hover_text("Off: keep the position from the file");
                ui.add_enabled_ui(s.place, |ui| {
                    ui.add(mm(&mut s.x).prefix("x "));
                    ui.add(mm(&mut s.y).prefix("y "));
                });
                ui.end_row();
                ui.checkbox(&mut s.fit, "Fit width").on_hover_text("Off: use the size from the file, times Scale");
                ui.add_enabled(s.fit, mm(&mut s.width).range(0.1..=10_000.0));
                ui.end_row();
                ui.label("Scale").on_hover_text("1.333 for 72 DPI files (older Illustrator)");
                ui.add_enabled(!s.fit, egui::DragValue::new(&mut s.scale).speed(0.01).range(0.001..=1000.0));
                ui.end_row();
                ui.label("Curve tolerance").on_hover_text("How far the arcs that replace curves may stray from them");
                ui.add(egui::DragValue::new(&mut s.tolerance).speed(0.001).range(0.001..=1.0).suffix(" mm"));
                ui.end_row();
            });
            ui.add_space(6.0);
            import = ui.button("Import").clicked();
        });
        if import {
            let opts = ImportOptions {
                x: s.place.then_some(s.x),
                y: s.place.then_some(s.y),
                width: s.fit.then_some(s.width),
                scale: s.scale,
                tolerance: s.tolerance,
                dir: None,
            };
            let path = s.path.clone();
            self.import_svg(&path, opts);
        }
        if import || !open {
            self.svg_dialog = None;
        }
    }

    /// Import with default size, its top-left corner at `at` (a dropped file).
    pub(super) fn import_svg_at(&mut self, path: &Path, at: Point) {
        self.import_svg(path, ImportOptions { x: Some(at[0]), y: Some(at[1]), ..ImportOptions::default() });
    }

    fn import_svg(&mut self, path: &Path, mut opts: ImportOptions) {
        opts.dir = path.parent().map(Path::to_path_buf);
        let result = std::fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| {
            let mut d = self.doc.lock().unwrap();
            let before = d.s.clone();
            let ids = d.import_svg(&bytes, &opts)?;
            d.push_undo(before);
            Ok(ids.len())
        });
        match result {
            Ok(n) => {
                self.set_tool(Tool::Select);
                self.message = format!("Imported {n} items from {}.", path.display());
            }
            Err(e) => self.message = format!("Could not import the SVG: {e}"),
        }
    }
}
