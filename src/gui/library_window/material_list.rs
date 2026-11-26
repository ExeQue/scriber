//! The material list with search, category filter and cut/score/engrave tags, shared by the
//! editor's sidebar and the ribbon's material picker.

use super::widgets::natural_cmp;
use crate::library::{Category, Library, Material, Operation};
use eframe::egui::{self, Align, Button, Color32, ComboBox, Layout, RichText};

/// Shown when the library has no materials at all.
pub(super) const NO_MATERIALS: &str = "No materials yet — + Add one or import";

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

/// One operation's mark: a rounded tag with its letter (C, S, E), filled in a colour that reads on
/// dark and light themes when the material has a setting for it, only outlined when not.
fn pill(ui: &mut egui::Ui, op: Operation, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), egui::Sense::hover());
    let dark = ui.visuals().dark_mode;
    // Brighter than the canvas colours: score's dark blue and engrave's black vanish on a dark theme.
    let (fill, letter) = match op {
        Operation::Cut => (Color32::from_rgb(226, 76, 66), Color32::WHITE),
        Operation::Score => (Color32::from_rgb(66, 140, 245), Color32::WHITE),
        Operation::Engrave if dark => (Color32::from_rgb(228, 228, 228), Color32::BLACK),
        Operation::Engrave => (Color32::from_rgb(45, 45, 45), Color32::WHITE),
    };
    let text = &op.label()[..1];
    let font = egui::FontId::proportional(10.5);
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, 4, fill);
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, font, letter);
    } else {
        let weak = ui.visuals().weak_text_color();
        painter.rect_stroke(rect.shrink(0.5), 4, egui::Stroke::new(1.0, weak), egui::StrokeKind::Inside);
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, font, weak);
    }
    response
}

/// The cut, score and engrave tags of one material for this machine and laser.
pub(super) fn op_pills(ui: &mut egui::Ui, m: &Material, machine: Option<&str>, laser: Option<&str>) {
    let mut ready = Vec::new();
    let mut hover: Option<egui::Response> = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for op in Operation::ALL {
            let on = has(m, machine, laser, op);
            if on {
                ready.push(op.label().to_lowercase());
            }
            let r = pill(ui, op, on);
            hover = Some(match hover.take() {
                Some(h) => r.union(h),
                None => r,
            });
        }
    });
    let target = if machine.is_some() { "this machine" } else { "any machine" };
    let text = if ready.is_empty() { format!("No settings for {target} yet") } else { format!("Settings for {target}: {}", ready.join(", ")) };
    if let Some(h) = hover {
        h.on_hover_text(text);
    }
}

/// What the tags mean.
pub(super) fn legend(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for op in Operation::ALL {
            pill(ui, op, true);
            ui.label(RichText::new(op.label()).small());
            ui.add_space(6.0);
        }
        ui.label(RichText::new("Outlined: no settings yet").small().weak());
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
        let mut list: Vec<(usize, &Material)> = materials.iter().enumerate().filter(|(_, m)| m.category == c && f.shows(m, machine, laser)).collect();
        list.sort_by(|a, b| natural_cmp(&a.1.name, &b.1.name));
        if list.is_empty() {
            continue;
        }
        any = true;
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(c.label()).small().weak()).truncate());
        for (i, m) in list {
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                op_pills(ui, m, machine, laser);
                // Justified: the row is as wide as the list, with its text on the left.
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    if ui.add(Button::selectable(selected == Some(m.id.as_str()), m.name.as_str()).truncate().min_size(egui::vec2(0.0, 26.0))).on_hover_text(&m.name).clicked() {
                        clicked = Some(i);
                    }
                });
            });
        }
    }
    if materials.is_empty() {
        ui.label(RichText::new(NO_MATERIALS).weak());
    } else if !any {
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

/// The ribbon's material picker: search, filter by kind, and the list with its tags for the document's machine.
pub(super) fn picker(ctx: &egui::Context, f: &mut Filter, lib: &Library, machine: Option<&str>, laser: Option<&str>, current: Option<&str>) -> Picked {
    let mut picked = Picked::Nothing;
    let modal = egui::Modal::new(egui::Id::unique("material-picker")).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.heading("Choose a material");
        let machine_name = machine.and_then(|id| lib.machine(id)).map(|m| m.name.as_str());
        ui.label(RichText::new(machine_name.map_or("No machine chosen: the tags show settings for any machine.".into(), |n| format!("The tags show which settings exist for your {n}."))).weak());
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
