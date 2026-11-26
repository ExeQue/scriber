//! The "Machines & materials" window, and the machine/material pickers in the laser bar.

use super::App;
use crate::library::{self, Laser, LaserKind, Library, Machine, Material, Operation, Setting};
use crate::model::Document;
use eframe::egui::{self, Color32, ComboBox, DragValue, RichText};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Machines,
    Materials,
}

/// The window's working copy; nothing is saved until the user presses Save.
pub(super) struct Editor {
    lib: Library,
    tab: Tab,
    machine: usize,
    material: usize,
    error: String,
}

impl Editor {
    pub(super) fn new() -> Self {
        Self { lib: library::get().clone(), tab: Tab::Machines, machine: 0, material: 0, error: String::new() }
    }
}

enum ListAction {
    Add,
    Duplicate,
    Delete,
}

impl App {
    pub(super) fn laser_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Machines & materials…").on_hover_text("Your laser cutters and the power and speed to use for each material").clicked() {
            self.library = Some(Editor::new());
        }
    }

    pub(super) fn library_window(&mut self, ctx: &egui::Context) {
        let Some(ed) = &mut self.library else { return };
        let (mut open, mut close) = (true, false);
        egui::Window::new("Machines & materials").open(&mut open).default_size([760.0, 520.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut ed.tab, Tab::Machines, "Machines");
                ui.selectable_value(&mut ed.tab, Tab::Materials, "Materials");
            });
            ui.separator();
            match ed.tab {
                Tab::Machines => machines_tab(ui, ed),
                Tab::Materials => materials_tab(ui, ed),
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    let lib = ed.lib.clone();
                    match library::update(|l| {
                        *l = lib;
                        Ok(())
                    }) {
                        Ok(()) => close = true,
                        Err(e) => ed.error = e,
                    }
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                ui.label(RichText::new(&ed.error).color(Color32::from_rgb(230, 90, 80)));
            });
        });
        if !open || close {
            self.library = None;
        }
    }
}

/// The list on the left with Add / Duplicate / Delete.
fn list(ui: &mut egui::Ui, names: Vec<&str>, selected: &mut usize) -> Option<ListAction> {
    let mut action = None;
    ui.vertical(|ui| {
        ui.set_width(200.0);
        egui::ScrollArea::vertical().id_salt("list").max_height(380.0).show(ui, |ui| {
            for (i, name) in names.iter().enumerate() {
                if ui.selectable_label(*selected == i, *name).clicked() {
                    *selected = i;
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Add").clicked() {
                action = Some(ListAction::Add);
            }
            if !names.is_empty() && ui.button("Duplicate").clicked() {
                action = Some(ListAction::Duplicate);
            }
            if !names.is_empty() && ui.button("Delete").clicked() {
                action = Some(ListAction::Delete);
            }
        });
    });
    action
}

fn machines_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.horizontal_top(|ui| {
        let names = ed.lib.machines.iter().map(|m| m.name.as_str()).collect();
        let action = list(ui, names, &mut ed.machine);
        let lib = &mut ed.lib;
        match action {
            Some(ListAction::Add) => {
                let laser = Laser { id: "laser-1".into(), name: "Laser".into(), kind: LaserKind::Diode, power_w: 10.0, ops: Operation::ALL.to_vec(), area: None };
                let m = Machine { id: lib.new_id("New machine"), name: "New machine".into(), vendor: String::new(), model: String::new(), width: 400.0, height: 300.0, lasers: vec![laser], notes: String::new(), source: String::new() };
                lib.machines.push(m);
                ed.machine = lib.machines.len() - 1;
            }
            Some(ListAction::Duplicate) => {
                let mut m = lib.machines[ed.machine].clone();
                m.name += " (copy)";
                m.id = lib.new_id(&m.name);
                lib.machines.push(m);
                ed.machine = lib.machines.len() - 1;
            }
            Some(ListAction::Delete) => {
                let id = lib.machines[ed.machine].id.clone();
                lib.remove_machine(&id);
            }
            None => {}
        }
        ed.machine = ed.machine.min(lib.machines.len().saturating_sub(1));
        ui.separator();
        if let Some(m) = lib.machines.get_mut(ed.machine) {
            egui::ScrollArea::vertical().id_salt("machine").show(ui, |ui| machine_form(ui, m));
        }
    });
}

fn machine_form(ui: &mut egui::Ui, m: &mut Machine) {
    egui::Grid::new("machine_form").num_columns(2).show(ui, |ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut m.name);
        ui.end_row();
        ui.label("Brand");
        ui.text_edit_singleline(&mut m.vendor);
        ui.end_row();
        ui.label("Model");
        ui.text_edit_singleline(&mut m.model);
        ui.end_row();
        ui.label("Work area").on_hover_text("The largest area the laser can reach, in mm");
        ui.horizontal(|ui| {
            ui.add(DragValue::new(&mut m.width).range(1.0..=5000.0).suffix(" mm"));
            ui.label("×");
            ui.add(DragValue::new(&mut m.height).range(1.0..=5000.0).suffix(" mm"));
        });
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.strong("Lasers");
    let mut remove = None;
    for (i, l) in m.lasers.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(130.0));
                ComboBox::from_id_salt("kind").selected_text(l.kind.label()).show_ui(ui, |ui| {
                    for k in LaserKind::ALL {
                        ui.selectable_value(&mut l.kind, k, k.label());
                    }
                });
                ui.add(DragValue::new(&mut l.power_w).range(0.1..=500.0).suffix(" W"));
                if ui.button("Remove").clicked() {
                    remove = Some(i);
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Can do:").weak());
                for op in Operation::ALL {
                    let mut on = l.ops.contains(&op);
                    if ui.checkbox(&mut on, op.label()).changed() {
                        l.ops.retain(|o| *o != op);
                        if on {
                            l.ops.push(op);
                        }
                    }
                }
            });
        });
    }
    if let Some(i) = remove {
        m.lasers.remove(i);
    }
    if ui.button("Add laser").clicked() {
        let id = (1..).map(|n| format!("laser-{n}")).find(|id| m.laser(id).is_none()).unwrap();
        m.lasers.push(Laser { id, name: "Laser".into(), kind: LaserKind::Diode, power_w: 10.0, ops: Operation::ALL.to_vec(), area: None });
    }
    ui.add_space(6.0);
    ui.label("Notes");
    ui.text_edit_multiline(&mut m.notes);
    ui.label("Where the numbers come from");
    ui.text_edit_multiline(&mut m.source);
}

fn materials_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.horizontal_top(|ui| {
        let names = ed.lib.materials.iter().map(|m| m.name.as_str()).collect();
        let action = list(ui, names, &mut ed.material);
        let lib = &mut ed.lib;
        match action {
            Some(ListAction::Add) => {
                let m = Material { id: lib.new_id("New material"), name: "New material".into(), thickness: 3.0, notes: String::new(), source: String::new(), settings: Vec::new() };
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
                let id = lib.materials[ed.material].id.clone();
                lib.remove_material(&id);
            }
            None => {}
        }
        ed.material = ed.material.min(lib.materials.len().saturating_sub(1));
        ui.separator();
        let Library { machines, materials, .. } = lib;
        if let Some(m) = materials.get_mut(ed.material) {
            egui::ScrollArea::vertical().id_salt("material").show(ui, |ui| material_form(ui, m, machines));
        }
    });
}

fn material_form(ui: &mut egui::Ui, m: &mut Material, machines: &[Machine]) {
    egui::Grid::new("material_form").num_columns(2).show(ui, |ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut m.name);
        ui.end_row();
        ui.label("Thickness");
        ui.add(DragValue::new(&mut m.thickness).range(0.0..=100.0).speed(0.1).suffix(" mm"));
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.strong("Laser settings").on_hover_text("Power, speed and passes for each machine, laser and operation");
    let mut remove = None;
    egui::Grid::new("settings").num_columns(9).striped(true).show(ui, |ui| {
        for h in ["Machine", "Laser", "Operation", "Power", "Speed", "Passes", "Line spacing", "Both ways", ""] {
            ui.label(RichText::new(h).weak());
        }
        ui.end_row();
        for (i, s) in m.settings.iter_mut().enumerate() {
            ui.push_id(i, |ui| setting_row(ui, s, machines));
            if ui.button("Remove").clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        m.settings.remove(i);
    }
    if let Some(machine) = machines.first()
        && ui.button("Add setting").clicked()
    {
        let laser = machine.lasers.first().map(|l| l.id.clone()).unwrap_or_default();
        m.settings.push(Setting { machine: machine.id.clone(), laser, operation: Operation::Cut, power: 100.0, speed: 10.0, passes: 1, line_interval: None, bidirectional: None });
    }
    ui.add_space(6.0);
    ui.label("Notes");
    ui.text_edit_multiline(&mut m.notes);
    ui.label("Where the settings come from");
    ui.text_edit_multiline(&mut m.source);
}

/// Eight grid cells for one setting.
fn setting_row(ui: &mut egui::Ui, s: &mut Setting, machines: &[Machine]) {
    let machine = machines.iter().find(|m| m.id == s.machine);
    ComboBox::from_id_salt("machine").selected_text(machine.map_or(s.machine.as_str(), |m| &m.name)).show_ui(ui, |ui| {
        for m in machines {
            if ui.selectable_label(s.machine == m.id, &m.name).clicked() && s.machine != m.id {
                s.machine = m.id.clone();
                s.laser = m.lasers.first().map(|l| l.id.clone()).unwrap_or_default();
            }
        }
    });
    let lasers = machine.map(|m| m.lasers.as_slice()).unwrap_or_default();
    let laser_name = lasers.iter().find(|l| l.id == s.laser).map_or(s.laser.as_str(), |l| &l.name);
    ComboBox::from_id_salt("laser").selected_text(laser_name).show_ui(ui, |ui| {
        for l in lasers {
            ui.selectable_value(&mut s.laser, l.id.clone(), &l.name);
        }
    });
    ComboBox::from_id_salt("op").selected_text(s.operation.label()).show_ui(ui, |ui| {
        for op in Operation::ALL {
            ui.selectable_value(&mut s.operation, op, op.label());
        }
    });
    ui.add(DragValue::new(&mut s.power).range(0.0..=100.0).suffix(" %"));
    ui.add(DragValue::new(&mut s.speed).range(0.1..=2000.0).suffix(" mm/s"));
    ui.add(DragValue::new(&mut s.passes).range(1..=50).suffix("×"));
    if s.operation == Operation::Engrave {
        let interval = s.line_interval.get_or_insert(0.1);
        ui.add(DragValue::new(interval).range(0.01..=2.0).speed(0.005).suffix(" mm"))
            .on_hover_text(format!("Distance between engraved lines: {:.0} lines per cm", 10.0 / *interval));
        ui.checkbox(s.bidirectional.get_or_insert(false), "").on_hover_text("Engrave in both directions (faster)");
    } else {
        (s.line_interval, s.bidirectional) = (None, None);
        ui.label("");
        ui.label("");
    }
}

/// The laser bar's second row: the document's machine, laser and material, and the settings that apply.
pub(super) fn pickers(ui: &mut egui::Ui, d: &mut Document) {
    let lib = library::get();
    let s = &d.s;
    let machine = s.machine.as_deref().and_then(|id| lib.machine(id));
    let material = s.material.as_deref().and_then(|id| lib.material(id));
    let (mut new_machine, mut new_laser, mut new_material) = (s.machine.clone(), s.laser.clone(), s.material.clone());
    let mut use_area = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Machine").weak());
        ComboBox::from_id_salt("doc_machine").selected_text(machine.map_or("None", |m| &m.name)).show_ui(ui, |ui| {
            ui.selectable_value(&mut new_machine, None, "None");
            for m in &lib.machines {
                ui.selectable_value(&mut new_machine, Some(m.id.clone()), &m.name);
            }
        });
        if let Some(m) = machine
            && m.lasers.len() > 1
        {
            let name = s.laser.as_deref().and_then(|l| m.laser(l)).map_or("Choose", |l| &l.name);
            ComboBox::from_id_salt("doc_laser").selected_text(name).show_ui(ui, |ui| {
                for l in &m.lasers {
                    ui.selectable_value(&mut new_laser, Some(l.id.clone()), &l.name);
                }
            });
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Material").weak());
        ComboBox::from_id_salt("doc_material").selected_text(material.map_or("None", |m| &m.name)).width(180.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut new_material, None, "None");
            for m in &lib.materials {
                ui.selectable_value(&mut new_material, Some(m.id.clone()), &m.name);
            }
        });
        if let Some(m) = machine {
            let [w, h] = m.area(s.laser.as_deref());
            if [w, h] != [s.width, s.height] && ui.button(format!("Use work area {w} × {h} mm")).on_hover_text("Make the material as large as the machine's work area").clicked() {
                use_area = Some([w, h]);
            }
        }
        if let (Some(m), Some(mat)) = (machine, material) {
            ui.add_space(8.0);
            let hints: Vec<String> = Operation::ALL.iter().filter_map(|op| mat.setting(&m.id, s.laser.as_deref(), *op).map(|x| format!("{}: {}", op.label(), x.hint()))).collect();
            if hints.is_empty() {
                ui.label(RichText::new("No settings for this material on this machine yet (Laser > Machines & materials)").weak());
            } else {
                ui.label(RichText::new(hints.join("   ")).weak());
            }
        }
    });
    if new_machine != s.machine {
        new_laser = new_machine.as_deref().and_then(|id| lib.machine(id)).and_then(|m| m.lasers.first()).map(|l| l.id.clone());
    }
    if (&new_machine, &new_laser, &new_material) == (&s.machine, &s.laser, &s.material) && use_area.is_none() {
        return;
    }
    d.checkpoint();
    (d.s.machine, d.s.laser, d.s.material) = (new_machine, new_laser, new_material);
    if let Some([w, h]) = use_area {
        (d.s.width, d.s.height) = (w, h);
    }
}
