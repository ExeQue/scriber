//! The pre-flight check in the ribbon's Laser tab: Preview and Check buttons, and a small window listing the
//! warnings; clicking one selects its shapes and zooms to them. A window rather than a modal, so the
//! drawing stays usable while the list is open.

use super::App;
use crate::library::{self, Library};
use crate::model::Document;
use crate::model::preflight::{self, Severity, Warning};
use eframe::egui::{self, Color32, RichText};

#[derive(Default)]
pub(super) struct Check {
    pub open: bool,
    /// The document revision and library the warnings were made for.
    key: Option<(u64, Library)>,
    warnings: Vec<Warning>,
}

impl Check {
    /// Run the check again when the document or the library changed.
    fn refresh(&mut self, d: &Document) {
        let lib = library::get();
        if self.key.as_ref().is_some_and(|(rev, l)| *rev == d.rev && *l == *lib) {
            return;
        }
        self.warnings = preflight::check(&d.s, &lib);
        self.key = Some((d.rev, lib.clone()));
    }
}

fn color(s: Severity) -> Color32 {
    match s {
        Severity::Error => Color32::from_rgb(230, 80, 70),
        Severity::Warning => Color32::from_rgb(230, 170, 40),
    }
}

impl App {
    /// The Preview toggle and the Check button with the number of warnings.
    pub(super) fn preview_and_check_buttons(&mut self, ui: &mut egui::Ui, d: &Document) {
        if super::icons::pill(ui, self.preview.is_some(), None, "Preview").on_hover_text(format!("Show only what the laser makes ({})", super::shortcuts::preview_key())).clicked() {
            self.toggle_preview();
        }
        ui.add_space(4.0);
        self.check.refresh(d);
        let w = &self.check.warnings;
        let dot = w.first().map(|w| color(w.severity)).unwrap_or(Color32::from_rgb(90, 180, 90));
        let label = if w.is_empty() { "Check".to_string() } else { format!("Check ({})", w.len()) };
        if super::icons::pill(ui, self.check.open, Some(dot), &label).on_hover_text("Things that could go wrong on the laser").clicked() {
            self.check.open = !self.check.open;
        }
    }

    pub(super) fn check_window(&mut self, ctx: &egui::Context) {
        if !self.check.open {
            return;
        }
        let mut open = true;
        let mut pick = None;
        egui::Window::new("Check before lasering").id(self.popouts.id("check"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .pivot(egui::Align2::RIGHT_TOP)
            .default_pos(self.canvas_rect.right_top() + egui::vec2(-12.0, 12.0))
            .show(ctx, |ui| {
                if self.check.warnings.is_empty() {
                    ui.label("Nothing found. The drawing looks ready for the laser.");
                }
                for (k, w) in self.check.warnings.iter().enumerate() {
                    let text = RichText::new(format!("●  {}", w.text)).color(color(w.severity));
                    if ui.add(egui::Button::new(text).frame(false).wrap()).on_hover_text("Select and show these shapes").clicked() {
                        pick = Some(k);
                    }
                }
            });
        self.check.open = open;
        let Some(ids) = pick.map(|k| self.check.warnings[k].items.clone()) else { return };
        let b = {
            let mut d = self.doc.lock().unwrap();
            d.selection = ids.into_iter().filter(|id| d.item(*id).is_some()).collect();
            d.bbox(&d.selection)
        };
        self.preview = None;
        if let Some(b) = b {
            self.zoom_to(b);
        }
    }
}
