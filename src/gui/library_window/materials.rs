//! The Materials page: the list of materials and the form for the selected one,
//! with its laser settings for one machine and laser at a time, as named profiles per operation.

use super::Editor;
use super::material_list::{category_combo, rows, search_box};
use super::widgets::{Confirm, ListAction, column_title, field_label, form_column, natural_cmp, notes_section, remove_button, section, sidebar};
use crate::gui::lucide;
use crate::library::{Category, Library, Machine, Material, Operation, Setting};
use eframe::egui::{self, Align, Checkbox, ComboBox, DragValue, Layout, RichText};

const LINES_HELP: &str = "How many engraved lines fit in 1 cm. More lines look darker and smoother but take longer. 100 lines per cm means the lines are 0.1 mm apart.";

pub(super) fn page(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.horizontal_top(|ui| {
        let (materials, filter, selected) = (&ed.lib.materials, &mut ed.filter, &mut ed.material);
        let (machine, laser) = (ed.doc_machine.as_deref(), ed.doc_laser.as_deref());
        let selected_id = materials.get(*selected).map(|m| m.id.clone());
        let mut header_filter = filter.clone();
        let action = sidebar(
            ui,
            "material",
            !materials.is_empty(),
            |ui| {
                search_box(ui, &mut header_filter);
                category_combo(ui, &mut header_filter);
            },
            |ui| {
                if let Some(i) = rows(ui, materials, filter, machine, laser, selected_id.as_deref()) {
                    *selected = i;
                }
            },
        );
        *filter = header_filter;
        let lib = &mut ed.lib;
        match action {
            Some(ListAction::Add) => {
                let category = ed.filter.category.unwrap_or_default();
                let m = Material { id: lib.new_id("New material"), name: "New material".into(), category, thickness: 3.0, notes: String::new(), source: String::new(), settings: Vec::new() };
                lib.materials.push(m);
                ed.material = lib.materials.len() - 1;
            }
            Some(ListAction::Duplicate) => {
                let mut m = lib.materials[ed.material].clone();
                m.name += " (copy)";
                m.id = lib.new_id(&m.name);
                lib.materials.push(m);
                ed.material = lib.materials.len() - 1;
            }
            Some(ListAction::Delete) => {
                let m = &lib.materials[ed.material];
                let id = m.id.clone();
                ed.confirm = Confirm::ask(format!("Delete the material \"{}\"?", m.name), move |lib| lib.remove_material(&id));
            }
            None => {}
        }
        ed.material = ed.material.min(lib.materials.len().saturating_sub(1));
        let Library { machines, materials, .. } = lib;
        let (index, confirm, view) = (ed.material, &mut ed.confirm, &mut ed.settings_view);
        form_column(ui, "material", |ui| match materials.get_mut(index) {
            Some(m) => form(ui, m, machines, index, confirm, view),
            None => _ = ui.label(RichText::new(super::material_list::NO_MATERIALS).weak()),
        });
    });
}

fn form(ui: &mut egui::Ui, m: &mut Material, machines: &[Machine], index: usize, confirm: &mut Option<Confirm>, view: &mut SettingsView) {
    section(ui, "General", "What the material is called, what kind it is and how thick it is.", |ui| {
        egui::Grid::new("material_general").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
            field_label(ui, "Name");
            ui.add(egui::TextEdit::singleline(&mut m.name).desired_width(260.0));
            ui.end_row();
            field_label(ui, "Kind").on_hover_text("Used to sort and filter the material list");
            ComboBox::from_id_salt("category").selected_text(m.category.label()).width(180.0).show_ui(ui, |ui| {
                for c in Category::ALL {
                    ui.selectable_value(&mut m.category, c, c.label());
                }
            });
            ui.end_row();
            field_label(ui, "Thickness");
            ui.add(DragValue::new(&mut m.thickness).range(0.0..=100.0).speed(0.1).suffix(" mm"));
            ui.end_row();
        });
    });
    section(ui, "Laser settings", "Power, speed and passes that work for this material, per machine and laser; several named profiles per operation. Try them on a scrap piece first.", |ui| {
        settings(ui, &mut m.settings, machines, index, confirm, view);
    });
    notes_section(ui, &mut m.notes, &mut m.source);
}

/// A machine and one of its laser modules: (machine id, laser id).
type Key = (String, String);

/// Which machine and laser the material page shows settings for, and the search in its picker.
#[derive(Default)]
pub(super) struct SettingsView {
    pub key: Option<Key>,
    pub search: String,
}

/// "xTool M2 · 10 W diode", or only the machine's name when it has one laser.
fn key_name(machines: &[Machine], key: &Key) -> String {
    match machines.iter().find(|m| m.id == key.0) {
        Some(m) if m.lasers.len() == 1 => m.name.clone(),
        Some(m) => format!("{} · {}", m.name, m.laser(&key.1).map_or(key.1.as_str(), |l| &l.name)),
        None => format!("{} · {}", key.0, key.1),
    }
}

/// The settings for one machine and laser at a time (there can be dozens of machines): a picker
/// that lists the machines with settings first, which others have settings, and per operation its
/// profiles as rows.
fn settings(ui: &mut egui::Ui, settings: &mut Vec<Setting>, machines: &[Machine], index: usize, confirm: &mut Option<Confirm>, view: &mut SettingsView) {
    let mut with: Vec<Key> = Vec::new();
    for s in settings.iter() {
        let key = (s.machine.clone(), s.laser.clone());
        if !with.contains(&key) {
            with.push(key);
        }
    }
    with.sort_by(|a, b| natural_cmp(&key_name(machines, a), &key_name(machines, b)));
    let all: Vec<Key> = machines.iter().flat_map(|m| m.lasers.iter().map(|l| (m.id.clone(), l.id.clone()))).collect();
    let known = |k: &Key| all.contains(k) || with.contains(k);
    let Some(key) = view.key.clone().filter(known).or_else(|| with.first().cloned()).or_else(|| all.first().cloned()) else {
        ui.label(RichText::new("Add a machine on the Machines page first.").weak());
        return;
    };
    view.key = Some(key.clone());

    ui.horizontal(|ui| {
        ui.label(RichText::new("Machine").weak());
        machine_picker(ui, view, machines, &with, &all);
        let others: Vec<&Key> = with.iter().filter(|k| **k != key).collect();
        if !others.is_empty() {
            ui.menu_button(format!("{} Copy from…", lucide::COPY), |ui| {
                ui.label(RichText::new("Copy all profiles from:").weak());
                for k in &others {
                    if ui.button(key_name(machines, k)).clicked() {
                        for mut s in settings.iter().filter(|s| (&s.machine, &s.laser) == (&k.0, &k.1)).cloned().collect::<Vec<_>>() {
                            (s.machine, s.laser) = key.clone();
                            settings.retain(|t| !(t.is_for(&s.machine, &s.laser, s.operation) && t.name == s.name));
                            settings.push(s);
                        }
                    }
                }
            })
            .response
            .on_hover_text("Start from another machine's or laser's numbers");
        }
        if with.contains(&key) {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if remove_button(ui, "all settings for this machine and laser") {
                    let (key, name) = (key.clone(), key_name(machines, &key));
                    *confirm = Confirm::ask(format!("Delete all settings for \"{name}\"?"), move |lib| {
                        lib.materials[index].settings.retain(|s| (&s.machine, &s.laser) != (&key.0, &key.1));
                    });
                }
            });
        }
    });
    // Which other machines have settings; a click shows them.
    let others: Vec<&Key> = with.iter().filter(|k| **k != key).collect();
    if !others.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Also set up for:").weak());
            for k in others.iter().take(4) {
                if ui.small_button(key_name(machines, k)).clicked() {
                    view.key = Some((*k).clone());
                }
            }
            if others.len() > 4 {
                ui.label(RichText::new(format!("(+{} more in the list)", others.len() - 4)).weak());
            }
        });
    }
    ui.add_space(6.0);

    let laser = machines.iter().find(|m| m.id == key.0).and_then(|m| m.laser(&key.1));
    let mut remove = None;
    for op in Operation::ALL {
        ui.push_id(op.label(), |ui| {
            egui::Frame::group(ui.style()).inner_margin(8).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(op.label()).strong());
                    if laser.is_some_and(|l| !l.ops.contains(&op)) {
                        ui.label(RichText::new(format!("(this laser is not set up to {})", op.label().to_lowercase())).weak().small());
                    }
                });
                let rows: Vec<usize> = (0..settings.len()).filter(|i| settings[*i].is_for(&key.0, &key.1, op)).collect();
                if !rows.is_empty() {
                    egui::Grid::new("profiles").num_columns(7).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
                        let titles: &[&str] = if op == Operation::Engrave { &["Profile", "Power", "Speed", "Passes", "Lines per cm", "Both ways"] } else { &["Profile", "Power", "Speed", "Passes"] };
                        for t in titles {
                            column_title(ui, t);
                        }
                        ui.label(""); // the remove column
                        ui.end_row();
                        for (n, &i) in rows.iter().enumerate() {
                            setting_row(ui, i, &mut settings[i], n == 0);
                            if remove_button(ui, "this profile") {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                }
                let add = if rows.is_empty() { format!("{} Add {} setting", lucide::PLUS, op.label().to_lowercase()) } else { format!("{} Add profile", lucide::PLUS) };
                if ui.button(add).on_hover_text("Profiles are named settings, e.g. Light, Dark or Photo; layers choose one").clicked() {
                    // The first is the default one and needs no name.
                    let name = if rows.is_empty() { String::new() } else { (rows.len() + 1..).map(|n| format!("Profile {n}")).find(|n| !rows.iter().any(|i| settings[*i].name == *n)).unwrap() };
                    let copy = rows.last().map(|i| settings[*i].clone());
                    settings.push(Setting { name, ..copy.unwrap_or_else(|| Setting::new(&key.0, &key.1, op)) });
                }
            });
        });
        ui.add_space(6.0);
    }
    if let Some(i) = remove {
        let s = &settings[i];
        let what = if s.name.is_empty() { format!("the default {} setting", s.operation.label().to_lowercase()) } else { format!("the profile \"{}\"", s.name) };
        *confirm = Confirm::ask(format!("Delete {what} for \"{}\"?", key_name(machines, &key)), move |lib| {
            lib.materials[index].settings.remove(i);
        });
    }
}

/// The machine and laser to show, with a search box: those with settings first, then the rest.
fn machine_picker(ui: &mut egui::Ui, view: &mut SettingsView, machines: &[Machine], with: &[Key], all: &[Key]) {
    let key = view.key.clone().unwrap_or_default();
    let mut rest: Vec<&Key> = all.iter().filter(|k| !with.contains(k)).collect();
    rest.sort_by(|a, b| natural_cmp(&key_name(machines, a), &key_name(machines, b)));
    ComboBox::from_id_salt("settings_machine").selected_text(key_name(machines, &key)).width(260.0).height(420.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show_ui(ui, |ui| {
        ui.add(egui::TextEdit::singleline(&mut view.search).hint_text("Search machines").desired_width(f32::INFINITY));
        let words = view.search.to_lowercase();
        let shows = |k: &Key| {
            let name = key_name(machines, k).to_lowercase();
            words.split_whitespace().all(|w| name.contains(w))
        };
        let mut pick = None;
        ui.label(RichText::new("With settings for this material").small().weak());
        let mut any = false;
        for k in with.iter().filter(|k| shows(k)) {
            any = true;
            if ui.selectable_label(*k == key, key_name(machines, k)).clicked() {
                pick = Some(k.clone());
            }
        }
        if !any {
            ui.label(RichText::new("None").weak());
        }
        ui.separator();
        ui.label(RichText::new("Add for another machine…").small().weak());
        for k in rest.into_iter().filter(|k| shows(k)) {
            if ui.selectable_label(*k == key, key_name(machines, k)).clicked() {
                pick = Some(k.clone());
            }
        }
        if let Some(k) = pick {
            view.key = Some(k);
            view.search.clear();
            ui.close();
        }
    });
}

/// One profile's grid cells. Engrave line spacing is stored in mm but shown as lines per cm.
fn setting_row(ui: &mut egui::Ui, i: usize, s: &mut Setting, first: bool) {
    let hint = if first { "Default" } else { "Name" };
    ui.add(egui::TextEdit::singleline(&mut s.name).id_salt(("name", i)).hint_text(hint).desired_width(110.0)).on_hover_text("e.g. Light, Dark, Photo. The first one is the default; layers can choose another.");
    ui.add(DragValue::new(&mut s.power).range(0.0..=100.0).suffix(" %"));
    ui.add(DragValue::new(&mut s.speed).range(0.1..=2000.0).suffix(" mm/s"));
    ui.add(DragValue::new(&mut s.passes).range(1..=50).suffix("×"));
    if s.operation == Operation::Engrave {
        let interval = s.line_interval.get_or_insert(0.1);
        let mut per_cm = 10.0 / *interval;
        if ui.add(DragValue::new(&mut per_cm).range(5.0..=500.0).speed(1.0).max_decimals(1)).on_hover_text(LINES_HELP).changed() {
            *interval = 10.0 / per_cm;
        }
        ui.vertical_centered(|ui| ui.add(Checkbox::without_text(s.bidirectional.get_or_insert(false))))
            .inner
            .on_hover_text("Engrave in both directions: faster, sometimes a little less crisp");
    }
}
