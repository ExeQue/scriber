//! The Theme menu (View tab and the View menu): every theme with a small preview swatch.

use super::{Choice, State, Theme, sources};
use eframe::egui::{self, RichText, Sense, Ui, vec2};

/// Background, material and accent side by side.
fn swatch(ui: &mut Ui, t: Option<&Theme>) {
    let (rect, _) = ui.allocate_exact_size(vec2(30.0, 14.0), Sense::hover());
    let Some(t) = t else { return };
    let third = rect.width() / 3.0;
    for (i, c) in [t.panel, t.material, t.accent].into_iter().enumerate() {
        let r = egui::Rect::from_min_size(rect.min + vec2(third * i as f32, 0.0), vec2(third, rect.height()));
        ui.painter().rect_filled(r, 0.0, c);
    }
    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
}

impl State {
    fn entry(&mut self, ui: &mut Ui, choice: Choice, label: &str, theme: Option<&Theme>, tip: &str) {
        ui.horizontal(|ui| {
            swatch(ui, theme);
            if ui.selectable_label(self.choice == choice, label).on_hover_text(tip).clicked() {
                self.choice = choice;
                self.note = None;
                ui.close();
            }
        });
    }

    pub fn menu(&mut self, ui: &mut Ui) {
        let omarchy = self.omarchy.clone();
        let system = match &omarchy {
            Some(Ok((name, _))) => format!("Follows Omarchy ({name})"),
            _ => "Follows your system's light or dark mode".into(),
        };
        let system_theme = match &omarchy {
            Some(Ok((_, t))) => Some(*t),
            _ => None,
        };
        self.entry(ui, Choice::System, "System", system_theme.as_ref(), &system);
        match &omarchy {
            Some(Ok((name, t))) => self.entry(ui, Choice::Omarchy, &format!("Omarchy: {name}"), Some(t), "Always Omarchy's current theme; changes with it"),
            Some(Err(e)) => _ = ui.add_enabled(false, egui::Button::new("Omarchy")).on_disabled_hover_text(format!("Omarchy's theme cannot be read: {e}")),
            None => _ = ui.add_enabled(false, egui::Button::new("Omarchy")).on_disabled_hover_text("Only on Omarchy (omarchy.org)"),
        }
        ui.separator();
        for (id, src) in sources::PRESETS {
            let p = super::palette::parse(src).ok();
            let name = p.as_ref().and_then(|p| p.name.clone()).unwrap_or(id.to_string());
            self.entry(ui, Choice::Preset(id.to_string()), &name, p.map(|p| p.theme()).as_ref(), "Built in");
        }
        let files: Vec<_> = self.entries.iter().map(|e| (e.stem.clone(), e.theme.clone())).collect();
        if !files.is_empty() {
            ui.separator();
        }
        for (stem, theme) in files {
            let tip = format!("{}", sources::user_dir().join(format!("{stem}.toml")).display());
            match theme {
                Ok((name, t)) => self.entry(ui, Choice::File(stem), &name, Some(&t), &tip),
                Err(e) => _ = ui.label(RichText::new(format!("{stem}: {e}")).color(super::get().error)).on_hover_text(tip),
            }
        }
        ui.separator();
        if ui.button("Save current as theme…").on_hover_text("Write the theme you see now to a new file in the themes folder, to make your own from").clicked() {
            match sources::save(&self.name, &super::get()) {
                Ok((stem, path)) => {
                    self.choice = Choice::File(stem);
                    self.note = Some(format!("Saved as {}", path.display()));
                }
                Err(e) => self.note = Some(format!("Could not save: {e}")),
            }
        }
        if ui.button("Open themes folder").on_hover_text(sources::user_dir().display().to_string()).clicked()
            && let Err(e) = sources::open_folder()
        {
            self.note = Some(format!("Could not open {}: {e}", sources::user_dir().display()));
        }
        if ui.button("Reload").on_hover_text("Read the theme files again").clicked() {
            self.reload();
        }
        if let Some(e) = &self.error {
            ui.label(RichText::new(format!("{e}; using {}", self.name)).color(super::get().error));
        }
        if let Some(n) = &self.note {
            ui.label(RichText::new(n).small());
        }
    }
}
