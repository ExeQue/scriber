//! The material list with search, category filter and cut/score/engrave dots, shared by the
//! editor's sidebar and the ribbon's material picker.

use crate::gui::labels::op_color;
use crate::library::{Category, Library, Material, Operation};
use crate::model::Op;
use eframe::egui::{self, Align, Button, ComboBox, Layout, RichText};

/// What the list shows; kept between frames by its owner.
#[derive(Clone, Default)]
pub(super) struct Filter {
    pub search: String,
    pub category: Option<Category>,
    /// Only materials with at least one setting for the machine.
    pub only_ready: bool,
}

impl Filter {
    fn shows(&self, m: &Material, machine: Option<&str>, laser: Option<&str>) -> bool {
        let text = format!("{} {}", m.name, m.category.label()).to_lowercase();
        self.search.to_lowercase().split_whitespace().all(|w| text.contains(w))
            && self.category.is_none_or(|c| c == m.category)
            && (!self.only_ready || Operation::ALL.iter().any(|op| has(m, machine, laser, *op)))
    }
}

/// Whether the material has a setting for `op` on this machine and laser (on any machine when none is chosen).
fn has(m: &Material, machine: Option<&str>, laser: Option<&str>, op: Operation) -> bool {
    match machine {
        Some(machine) => m.setting(machine, laser, op).is_some(),
        None => m.settings.iter().any(|s| s.operation == op),
    }
}

fn color(op: Operation) -> egui::Color32 {
    op_color(match op {
        Operation::Cut => Op::Cut,
        Operation::Score => Op::Score,
        Operation::Engrave => Op::Engrave,
    })
}

/// Three dots in the op colours: filled when the material has settings for that operation, hollow when not.
pub(super) fn op_dots(ui: &mut egui::Ui, m: &Material, machine: Option<&str>, laser: Option<&str>) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(34.0, 12.0), egui::Sense::hover());
    let mut ready = Vec::new();
    for (k, op) in Operation::ALL.into_iter().enumerate() {
        let centre = rect.left_center() + egui::vec2(6.0 + k as f32 * 11.0, 0.0);
        if has(m, machine, laser, op) {
            ui.painter().circle_filled(centre, 4.0, color(op));
            ready.push(op.label().to_lowercase());
        } else {
            ui.painter().circle_stroke(centre, 3.5, egui::Stroke::new(1.0, color(op).gamma_multiply(0.6)));
        }
    }
    let target = if machine.is_some() { "this machine" } else { "any machine" };
    let text = if ready.is_empty() { format!("No settings for {target} yet") } else { format!("Settings for {target}: {}", ready.join(", ")) };
    response.on_hover_text(text);
}

/// What the dots mean, in their colours.
pub(super) fn legend(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for op in Operation::ALL {
            ui.label(RichText::new(format!("● {}", op.label())).small().color(color(op)));
        }
        ui.label(RichText::new("(hollow: no settings yet)").small().weak());
    });
}

pub(super) fn search_box(ui: &mut egui::Ui, f: &mut Filter) {
    ui.add(egui::TextEdit::singleline(&mut f.search).hint_text("🔍 Search").desired_width(f32::INFINITY));
}

/// The category filter as a drop-down (for the narrow sidebar).
pub(super) fn category_combo(ui: &mut egui::Ui, f: &mut Filter) {
    ComboBox::from_id_salt("category_filter").selected_text(f.category.map_or("All kinds", Category::label)).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
        ui.selectable_value(&mut f.category, None, "All kinds");
        for c in Category::ALL {
            ui.selectable_value(&mut f.category, Some(c), c.label());
        }
    });
}

/// The category filter as chips, only for kinds the library has.
fn category_chips(ui: &mut egui::Ui, f: &mut Filter, lib: &Library) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut f.category, None, "All");
        for c in Category::ALL.into_iter().filter(|c| lib.materials.iter().any(|m| m.category == *c)) {
            ui.selectable_value(&mut f.category, Some(c), c.label());
        }
    });
}

/// The materials that pass the filter, under a heading per category. Returns the index of a clicked one.
pub(super) fn rows(ui: &mut egui::Ui, materials: &[Material], f: &Filter, machine: Option<&str>, laser: Option<&str>, selected: Option<&str>) -> Option<usize> {
    let mut clicked = None;
    let mut any = false;
    for c in Category::ALL {
        let list: Vec<(usize, &Material)> = materials.iter().enumerate().filter(|(_, m)| m.category == c && f.shows(m, machine, laser)).collect();
        if list.is_empty() {
            continue;
        }
        any = true;
        ui.add_space(4.0);
        ui.label(RichText::new(c.label()).small().weak());
        for (i, m) in list {
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                op_dots(ui, m, machine, laser);
                // Justified: the row is as wide as the list, with its text on the left.
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    if ui.add(Button::selectable(selected == Some(m.id.as_str()), m.name.as_str()).min_size(egui::vec2(0.0, 26.0))).clicked() {
                        clicked = Some(i);
                    }
                });
            });
        }
    }
    if !any {
        ui.label(RichText::new("No material matches.").weak());
    }
    clicked
}

/// What the ribbon's picker did this frame.
pub(super) enum Picked {
    Nothing,
    Closed,
    Chose(Option<String>),
}

/// The ribbon's material picker: search, filter by kind, and the list with its dots for the document's machine.
pub(super) fn picker(ctx: &egui::Context, f: &mut Filter, lib: &Library, machine: Option<&str>, laser: Option<&str>, current: Option<&str>) -> Picked {
    let mut picked = Picked::Nothing;
    let modal = egui::Modal::new(egui::Id::unique("material-picker")).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.heading("Choose a material");
        let machine_name = machine.and_then(|id| lib.machine(id)).map(|m| m.name.as_str());
        ui.label(RichText::new(machine_name.map_or("No machine chosen: the dots show settings for any machine.".into(), |n| format!("The dots show which settings exist for your {n}."))).weak());
        ui.add_space(6.0);
        search_box(ui, f);
        category_chips(ui, f, lib);
        if machine.is_some() {
            ui.checkbox(&mut f.only_ready, "Only materials with settings for this machine");
        }
        ui.separator();
        egui::ScrollArea::vertical().max_height(340.0).auto_shrink([false, false]).show(ui, |ui| {
            ui.vertical(|ui| {
                if let Some(i) = rows(ui, &lib.materials, f, machine, laser, current) {
                    picked = Picked::Chose(Some(lib.materials[i].id.clone()));
                }
            });
        });
        ui.separator();
        legend(ui);
        ui.horizontal(|ui| {
            if ui.button("No material").on_hover_text("Clear the document's material").clicked() {
                picked = Picked::Chose(None);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    picked = Picked::Closed;
                }
            });
        });
    });
    match picked {
        Picked::Nothing if modal.should_close() => Picked::Closed,
        p => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Setting;

    #[test]
    fn filter_by_words_kind_and_settings() {
        let cut = Setting { machine: "m2".into(), laser: "d10".into(), operation: Operation::Cut, power: 100.0, speed: 8.0, passes: 1, line_interval: None, bidirectional: None };
        let m = Material { id: "ply".into(), name: "Basswood plywood 3 mm".into(), category: Category::Plywood, thickness: 3.0, notes: String::new(), source: String::new(), settings: vec![cut] };
        let f = |search: &str, category, only_ready| Filter { search: search.into(), category, only_ready };
        assert!(f("ply 3", None, false).shows(&m, None, None));
        assert!(f("mdf", None, false).shows(&m, None, None), "the category label counts too");
        assert!(!f("acrylic", None, false).shows(&m, None, None));
        assert!(!f("", Some(Category::Wood), false).shows(&m, None, None));
        assert!(f("", None, true).shows(&m, Some("m2"), Some("d10")));
        assert!(!f("", None, true).shows(&m, Some("m2"), Some("ir")));
        assert!(!f("", None, true).shows(&m, Some("k40"), None));
    }
}
