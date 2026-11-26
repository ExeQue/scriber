//! The Materials page: the list of materials and the form for the selected one,
//! with its laser settings grouped by machine and laser module.

use super::Editor;
use super::material_list::{category_combo, rows, search_box};
use super::widgets::{Confirm, ListAction, column_title, field_label, form_column, notes_section, remove_button, section, sidebar};
use crate::gui::lucide;
use crate::library::{Category, Library, Machine, Material, Operation, Setting};
use eframe::egui::{self, Align, Button, Checkbox, ComboBox, DragValue, Layout, RichText};

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
        let (index, confirm) = (ed.material, &mut ed.confirm);
        form_column(ui, "material", |ui| match materials.get_mut(index) {
            Some(m) => form(ui, m, machines, index, confirm),
            None => _ = ui.label(RichText::new(super::material_list::NO_MATERIALS).weak()),
        });
    });
}

fn form(ui: &mut egui::Ui, m: &mut Material, machines: &[Machine], index: usize, confirm: &mut Option<Confirm>) {
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
    section(ui, "Laser settings", "Power, speed and passes that work for this material, per machine and laser. Try them on a scrap piece first.", |ui| {
        settings(ui, &mut m.settings, machines, index, confirm);
    });
    notes_section(ui, &mut m.notes, &mut m.source);
}

/// One box per machine and laser module, in the order they first appear.
fn settings(ui: &mut egui::Ui, settings: &mut Vec<Setting>, machines: &[Machine], index: usize, confirm: &mut Option<Confirm>) {
    let mut groups: Vec<(String, String)> = Vec::new();
    for s in settings.iter() {
        let key = (s.machine.clone(), s.laser.clone());
        if !groups.contains(&key) {
            groups.push(key);
        }
    }
    for (g, key) in groups.iter().enumerate() {
        ui.push_id(g, |ui| group(ui, settings, key, machines, index, confirm));
        ui.add_space(8.0);
    }
    if groups.is_empty() {
        ui.label(RichText::new("No settings yet.").weak());
    }
    let free = machines.iter().flat_map(|mc| mc.lasers.iter().map(|l| (mc.id.clone(), l.id.clone()))).find(|k| !groups.contains(k));
    let add = ui.add_enabled(free.is_some(), Button::new(format!("{} Add settings for a machine or laser", lucide::PLUS)));
    if add.on_disabled_hover_text("Every laser already has settings here").clicked()
        && let Some((machine, laser)) = free
    {
        settings.push(new_setting(machine, laser, Operation::Cut));
    }
}

fn new_setting(machine: String, laser: String, operation: Operation) -> Setting {
    Setting { machine, laser, operation, power: 100.0, speed: 10.0, passes: 1, line_interval: None, bidirectional: None }
}

/// The settings for one machine and laser: which ones on top, then a row per operation.
fn group(ui: &mut egui::Ui, settings: &mut Vec<Setting>, key: &(String, String), machines: &[Machine], index: usize, confirm: &mut Option<Confirm>) {
    let in_group = |s: &Setting| (&s.machine, &s.laser) == (&key.0, &key.1);
    let (mut machine, mut laser) = key.clone();
    let (mut remove, mut remove_all, mut add) = (None, false, None);
    egui::Frame::group(ui.style()).inner_margin(10).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            machine_and_laser(ui, &mut machine, &mut laser, machines);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| remove_all = remove_button(ui, "all settings for this laser"));
        });
        ui.add_space(4.0);
        egui::Grid::new("operations").num_columns(7).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
            for title in ["Operation", "Power", "Speed", "Passes", "Lines per cm"] {
                column_title(ui, title);
            }
            ui.vertical_centered(|ui| column_title(ui, "Both ways"));
            ui.label(""); // the remove column
            ui.end_row();
            for (i, s) in settings.iter_mut().enumerate().filter(|(_, s)| in_group(s)) {
                setting_row(ui, i, s);
                if remove_button(ui, "this setting") {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for op in Operation::ALL.into_iter().filter(|op| !settings.iter().any(|s| in_group(s) && s.operation == *op)) {
                if ui.button(format!("{} Add {}", lucide::PLUS, op.label().to_lowercase())).clicked() {
                    add = Some(op);
                }
            }
        });
    });
    let laser_name = machines.iter().find(|m| m.id == key.0).and_then(|m| m.laser(&key.1)).map_or(key.1.as_str(), |l| &l.name);
    if remove_all {
        let key = key.clone();
        *confirm = Confirm::ask(format!("Delete all settings for \"{laser_name}\"?"), move |lib| {
            lib.materials[index].settings.retain(|s| (&s.machine, &s.laser) != (&key.0, &key.1));
        });
    }
    if let Some(i) = remove {
        *confirm = Confirm::ask(format!("Delete the {} setting for \"{laser_name}\"?", settings[i].operation.label().to_lowercase()), move |lib| {
            lib.materials[index].settings.remove(i);
        });
    }
    if let Some(op) = add {
        settings.push(new_setting(key.0.clone(), key.1.clone(), op));
    }
    // A different machine or laser moves the whole group.
    if (&machine, &laser) != (&key.0, &key.1) {
        for s in settings.iter_mut().filter(|s| in_group(s)) {
            (s.machine, s.laser) = (machine.clone(), laser.clone());
        }
    }
}

fn machine_and_laser(ui: &mut egui::Ui, machine_id: &mut String, laser_id: &mut String, machines: &[Machine]) {
    let machine = machines.iter().find(|m| m.id == *machine_id);
    ui.label(RichText::new("Machine").weak());
    ComboBox::from_id_salt("machine").selected_text(machine.map_or(machine_id.as_str(), |m| &m.name)).show_ui(ui, |ui| {
        for m in machines {
            if ui.selectable_label(*machine_id == m.id, &m.name).clicked() && *machine_id != m.id {
                *machine_id = m.id.clone();
                *laser_id = m.lasers.first().map(|l| l.id.clone()).unwrap_or_default();
            }
        }
    });
    ui.add_space(8.0);
    let lasers = machine.map(|m| m.lasers.as_slice()).unwrap_or_default();
    let name = lasers.iter().find(|l| l.id == *laser_id).map_or(laser_id.as_str(), |l| &l.name);
    ui.label(RichText::new("Laser").weak());
    ComboBox::from_id_salt("laser").selected_text(name).show_ui(ui, |ui| {
        for l in lasers {
            ui.selectable_value(laser_id, l.id.clone(), &l.name);
        }
    });
}

/// Six grid cells for one setting. Engrave line spacing is stored in mm but shown as lines per cm.
fn setting_row(ui: &mut egui::Ui, i: usize, s: &mut Setting) {
    ComboBox::from_id_salt(("op", i)).selected_text(s.operation.label()).show_ui(ui, |ui| {
        for op in Operation::ALL {
            ui.selectable_value(&mut s.operation, op, op.label());
        }
    });
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
    } else {
        (s.line_interval, s.bidirectional) = (None, None);
        ui.label("");
        ui.label("");
    }
}
