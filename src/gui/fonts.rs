//! A font picker: the installed font families with a search field, as a combo box, or as a
//! submenu inside a menu (a combo box's popup would replace the menu, which then closes).

use crate::model::text;
use eframe::egui;
use egui::containers::menu::{MenuConfig, SubMenuButton, is_in_menu};

/// Generic families listed first; each is the system's default font of that kind.
const GENERIC: [&str; 3] = ["sans-serif", "serif", "monospace"];

/// Pick a font family into `font`. Returns true when it changed.
pub fn font_picker(ui: &mut egui::Ui, id_salt: &str, font: &mut String) -> bool {
    let search_id = ui.make_persistent_id((id_salt, "font-search"));
    let current = font.clone();
    let mut changed = false;
    let list = |ui: &mut egui::Ui| {
        ui.set_min_width(180.0);
        let mut search: String = ui.data_mut(|d| d.get_temp(search_id)).unwrap_or_default();
        let field = ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search fonts"));
        if !field.has_focus() && search.is_empty() {
            field.request_focus();
        }
        let needle = search.to_lowercase();
        let names = GENERIC.into_iter().chain(text::families().iter().map(String::as_str));
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for name in names.filter(|n| n.to_lowercase().contains(&needle)) {
                if ui.selectable_label(*font == name, name).clicked() {
                    *font = name.to_owned();
                    changed = true;
                    ui.close();
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(search_id, search));
    };
    if is_in_menu(ui) {
        // Clicking the search field must not close the menu; picking a font does (ui.close).
        let config = MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        SubMenuButton::new(current).config(config).ui(ui, list);
    } else {
        egui::ComboBox::from_id_salt(id_salt).selected_text(current).width(180.0).height(340.0).show_ui(ui, list);
    }
    changed
}
