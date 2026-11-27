//! The Machines page: the list of machines and the form for the selected one.

use super::Editor;
use super::widgets::{Confirm, ListAction, column_title, field_label, form_column, list_row, natural_cmp, notes_section, read_only_if_source, remove_button, section, sidebar};
use crate::gui::lucide;
use crate::library::{Laser, LaserKind, Machine, Operation};
use eframe::egui::{self, Checkbox, ComboBox, DragValue};

pub(super) fn page(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.horizontal_top(|ui| {
        let deletable = ed.lib.machines.get(ed.machine).is_some_and(|m| !ed.lib.machine_origin(&m.id).is_source());
        let (machines, selected) = (&ed.lib.machines, &mut ed.machine);
        let action = sidebar(ui, "machine", !machines.is_empty(), deletable, |_| {}, |ui| {
            let mut order: Vec<(usize, &Machine)> = machines.iter().enumerate().collect();
            order.sort_by(|a, b| natural_cmp(&a.1.name, &b.1.name));
            for (i, m) in order {
                if list_row(ui, *selected == i, &m.name) {
                    *selected = i;
                }
            }
        });
        let lib = &mut ed.lib;
        match action {
            Some(ListAction::Add) => {
                let m = Machine { id: lib.new_id("New machine"), name: "New machine".into(), vendor: String::new(), model: String::new(), width: 400.0, height: 300.0, lasers: vec![new_laser("laser-1".into())], notes: String::new(), source: String::new(), xtool: None, image_modes: None };
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
                let m = &lib.machines[ed.machine];
                let id = m.id.clone();
                ed.confirm = Confirm::ask(format!("Delete the machine \"{}\"?", m.name), move |lib| lib.remove_machine(&id));
            }
            None => {}
        }
        ed.machine = ed.machine.min(lib.machines.len().saturating_sub(1));
        let (index, confirm) = (ed.machine, &mut ed.confirm);
        let origin = lib.machines.get(index).map(|m| lib.machine_origin(&m.id)).unwrap_or_default();
        form_column(ui, "machine", |ui| match lib.machines.get_mut(index) {
            Some(m) => read_only_if_source(ui, &origin, |ui| form(ui, m, index, confirm)),
            None => _ = ui.label(egui::RichText::new("No machines yet. Press Add to make one.").weak()),
        });
    });
}

fn new_laser(id: String) -> Laser {
    Laser { id, name: "Laser".into(), kind: LaserKind::Diode, power_w: 10.0, ops: Operation::ALL.to_vec(), area: None, xtool_laser: None }
}

fn form(ui: &mut egui::Ui, m: &mut Machine, index: usize, confirm: &mut Option<Confirm>) {
    section(ui, "General", "What the machine is called and how large an area it can reach.", |ui| {
        egui::Grid::new("machine_general").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
            field_label(ui, "Name");
            ui.add(egui::TextEdit::singleline(&mut m.name).desired_width(260.0));
            ui.end_row();
            field_label(ui, "Brand");
            ui.add(egui::TextEdit::singleline(&mut m.vendor).desired_width(260.0).hint_text("e.g. xTool"));
            ui.end_row();
            field_label(ui, "Model");
            ui.add(egui::TextEdit::singleline(&mut m.model).desired_width(260.0).hint_text("e.g. M2"));
            ui.end_row();
            field_label(ui, "Work area").on_hover_text("The largest area the laser can reach");
            ui.horizontal(|ui| {
                ui.add(DragValue::new(&mut m.width).range(1.0..=5000.0).suffix(" mm"));
                ui.label("×");
                ui.add(DragValue::new(&mut m.height).range(1.0..=5000.0).suffix(" mm"));
            });
            ui.end_row();
        });
    });
    section(ui, "Lasers", "The laser modules this machine can use, and what each one can do.", |ui| lasers(ui, m, index, confirm));
    section(ui, "Other software", "How other laser software knows this machine; needed to export projects for it.", |ui| other_software(ui, m));
    notes_section(ui, &mut m.notes, &mut m.source);
}

/// xTool Studio's device code and name, and each laser's module string (empty = derived, shown greyed).
fn other_software(ui: &mut egui::Ui, m: &mut Machine) {
    egui::Grid::new("machine_other").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
        let x = m.xtool.get_or_insert_with(Default::default);
        field_label(ui, "xTool device code").on_hover_text("xTool Studio's code for this machine, e.g. JS002 for the M2; needed for .xs export");
        ui.add(egui::TextEdit::singleline(&mut x.device).desired_width(120.0).hint_text("e.g. JS002"));
        ui.end_row();
        field_label(ui, "xTool name").on_hover_text("xTool Studio's short name; empty = the model");
        ui.add(egui::TextEdit::singleline(&mut x.name).desired_width(120.0).hint_text(m.model.as_str()));
        ui.end_row();
        for (i, l) in m.lasers.iter_mut().enumerate() {
            field_label(ui, &format!("xTool module: {}", l.name)).on_hover_text("xTool Studio's name for this laser module; empty = derived from type and power");
            let mut text = l.xtool_laser.clone().unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut text).id_salt(("xtool_laser", i)).desired_width(160.0).hint_text(l.xtool_default())).changed() {
                l.xtool_laser = (!text.trim().is_empty()).then_some(text);
            }
            ui.end_row();
        }
    });
    // An untouched section adds nothing to the machine.
    if m.xtool.as_ref().is_some_and(|x| x.device.is_empty() && x.name.is_empty()) {
        m.xtool = None;
    }
}

/// One table row per laser module.
fn lasers(ui: &mut egui::Ui, m: &mut Machine, index: usize, confirm: &mut Option<Confirm>) {
    let mut remove = None;
    // The name takes what the other columns leave (about 330 pt with their spacing), up to 240.
    let name_width = (ui.available_width() - 340.0).clamp(80.0, 240.0);
    egui::Grid::new("lasers").num_columns(4 + Operation::ALL.len()).min_col_width(0.0).striped(true).spacing([10.0, 8.0]).show(ui, |ui| {
        for title in ["Name", "Type", "Power"] {
            column_title(ui, title);
        }
        for op in Operation::ALL {
            ui.vertical_centered(|ui| column_title(ui, op.label())).inner.on_hover_text(format!("Tick if this laser can {}", op.label().to_lowercase()));
        }
        ui.label(""); // the remove column
        ui.end_row();
        for (i, l) in m.lasers.iter_mut().enumerate() {
            // A grid cell only offers last frame's column width, so a plain field would keep its
            // first, tiny width: claim the size outright.
            ui.add_sized([name_width, ui.spacing().interact_size.y], egui::TextEdit::singleline(&mut l.name).id_salt(("laser_name", i)));
            ComboBox::from_id_salt(("kind", i)).selected_text(l.kind.label()).width(100.0).show_ui(ui, |ui| {
                for k in LaserKind::ALL {
                    ui.selectable_value(&mut l.kind, k, k.label());
                }
            });
            ui.add(DragValue::new(&mut l.power_w).range(0.1..=500.0).suffix(" W"));
            for op in Operation::ALL {
                let mut on = l.ops.contains(&op);
                if ui.vertical_centered(|ui| ui.add(Checkbox::without_text(&mut on))).inner.changed() {
                    l.ops.retain(|o| *o != op);
                    if on {
                        l.ops.push(op);
                    }
                }
            }
            if remove_button(ui, "this laser") {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        *confirm = Confirm::ask(format!("Delete the laser \"{}\"?", m.lasers[i].name), move |lib| {
            lib.machines[index].lasers.remove(i);
        });
    }
    ui.add_space(4.0);
    if ui.button(format!("{} Add laser", lucide::PLUS)).clicked() {
        let id = (1..).map(|n| format!("laser-{n}")).find(|id| m.laser(id).is_none()).unwrap();
        m.lasers.push(new_laser(id));
    }
}
