//! A font picker: a combo box of the installed font families with a search field.

use crate::model::text;
use eframe::egui;

/// Generic families listed first; each is the system's default font of that kind.
const GENERIC: [&str; 3] = ["sans-serif", "serif", "monospace"];

/// Pick a font family into `font`. Returns true when it changed.
pub fn font_picker(ui: &mut egui::Ui, id_salt: &str, font: &mut String) -> bool {
    let search_id = ui.make_persistent_id((id_salt, "font-search"));
    let mut changed = false;
    egui::ComboBox::from_id_salt(id_salt).selected_text(font.as_str()).width(180.0).height(320.0).show_ui(ui, |ui| {
        let mut search: String = ui.data_mut(|d| d.get_temp(search_id)).unwrap_or_default();
        let field = ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search fonts"));
        if !field.has_focus() && search.is_empty() {
            field.request_focus();
        }
        let needle = search.to_lowercase();
        let names = GENERIC.into_iter().chain(text::families().iter().map(String::as_str));
        for name in names.filter(|n| n.to_lowercase().contains(&needle)) {
            if ui.selectable_label(font == name, name).clicked() {
                *font = name.to_owned();
                changed = true;
                ui.close();
            }
        }
        ui.data_mut(|d| d.insert_temp(search_id, search));
    });
    changed
}
