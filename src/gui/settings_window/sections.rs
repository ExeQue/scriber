//! The Settings dialog's sections. They change the editor's state directly; settings.rs saves it.

use super::super::library_window::{Editor, section};
use super::super::snap::Aid;
use super::App;
use eframe::egui::{self, DragValue, RichText};

impl App {
    pub(super) fn appearance_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "Theme", "The colours of the window and the canvas. Theme files live in the themes folder.", |ui| self.theme.picker(ui));
    }

    pub(super) fn aids_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "Snaps and tracking", "How drawing starts; the status bar turns them on and off as you draw.", |ui| {
            for a in Aid::ALL {
                let mut on = self.aid_on(a);
                if ui.checkbox(&mut on, RichText::new(a.label()).monospace()).on_hover_text(a.help()).changed() {
                    self.toggle(a);
                }
                ui.label(RichText::new(a.help()).weak().small());
            }
        });
        section(ui, "Polar angles", "POLAR tracks every multiple of the step, and the extra angles both ways.", |ui| {
            ui.horizontal(|ui| {
                ui.label("Track every");
                ui.add(DragValue::new(&mut self.aids.polar_step).range(1.0..=90.0).speed(0.5).suffix("°"));
            });
            ui.horizontal(|ui| {
                ui.label("Also track");
                self.polar_extra_field(ui);
            });
        });
        section(ui, "On the canvas", "Also in the ribbon's View tab.", |ui| {
            ui.checkbox(&mut self.show_guides, "Show the guide lines (they are never cut)");
            ui.checkbox(&mut self.measure_hover, "Measure on hover: lengths and sizes under the pointer (else hold Ctrl)");
        });
    }

    pub(super) fn files_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "At start", "What Scriber opens when it starts without a file.", |ui| {
            ui.checkbox(&mut self.reopen_files, "Reopen the files that were open when Scriber closed");
        });
        let recent = self.ws.recent();
        section(ui, "Recent files", "File → Open recent, and what AI agents see as recent projects.", |ui| {
            if recent.is_empty() {
                ui.label(RichText::new("No recent files.").weak());
            }
            for f in &recent {
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let text = if f.is_file() { RichText::new(name) } else { RichText::new(format!("{name} (missing)")).weak() };
                ui.add(egui::Label::new(text).truncate()).on_hover_text(f.display().to_string());
            }
            ui.add_space(4.0);
            if ui.add_enabled(!recent.is_empty(), egui::Button::new("Clear the list")).clicked() {
                self.ws.set_recent(Vec::new());
            }
        });
    }

    /// Returns true when it opened the library window (the dialog closes for it).
    pub(super) fn library_section(&mut self, ui: &mut egui::Ui) -> bool {
        section(ui, "Library sources", "Libraries of machines and materials that other people publish.", |ui| {
            ui.checkbox(&mut self.auto_sync, "Sync the library sources when Scriber starts");
            ui.label(RichText::new("Each source is checked at most once a day; Sources in Machines & materials syncs by hand.").weak().small());
        });
        let mut open = false;
        section(ui, "Machines & materials", "Your laser cutters, materials and their settings.", |ui| {
            open = ui.button("Open Machines & materials…").clicked();
        });
        if open {
            self.library = Some(Editor::new(&self.doc.lock().unwrap().s));
            self.settings = None;
        }
        open
    }

    /// Returns true when it opened the agents window (the dialog closes for it).
    pub(super) fn agents_section(&mut self, ui: &mut egui::Ui) -> bool {
        let mut open = false;
        section(ui, "MCP server", "AI agents draw in Scriber through its MCP server, which starts with the window.", |ui| {
            let t = crate::gui::theme::get();
            match &self.mcp {
                Ok(port) => ui.label(RichText::new(format!("● Running on port {port}")).color(t.ok)),
                Err(e) => ui.label(RichText::new(format!("● Not running (is Scriber already running?): {e}")).color(t.error)),
            };
            ui.add_space(4.0);
            open = ui.button("Connect AI agents…").clicked();
        });
        section(ui, "Command line", "Type AutoCAD commands, for CAD users. Also in the ribbon's View tab.", |ui| {
            ui.checkbox(&mut self.show_cmdline, "Show the command line");
        });
        if open {
            self.settings = None;
            self.open_agents();
        }
        open
    }
}
