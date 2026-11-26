//! The "Machines & materials" window, and the machine/material pickers in the ribbon's Laser tab.

use super::App;
use crate::library::{self, Library, Operation};
use crate::model::{Document, State};
use eframe::egui::{self, Color32, ComboBox, RichText};
use material_list::{Filter, Picked};
use widgets::Confirm;

mod machines;
mod material_list;
mod materials;
mod widgets;

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
    /// The document's machine and laser: the material list's dots show the settings for them.
    doc_machine: Option<String>,
    doc_laser: Option<String>,
    filter: Filter,
    /// A delete waiting for "yes".
    confirm: Option<Confirm>,
}

impl Editor {
    /// Opens on the document's machine and material.
    pub(super) fn new(s: &State) -> Self {
        let lib = library::get().clone();
        let index = |list: Vec<&String>, id: &Option<String>| list.iter().position(|i| Some(*i) == id.as_ref()).unwrap_or(0);
        let machine = index(lib.machines.iter().map(|m| &m.id).collect(), &s.machine);
        let material = index(lib.materials.iter().map(|m| &m.id).collect(), &s.material);
        Self { lib, tab: Tab::Machines, machine, material, error: String::new(), doc_machine: s.machine.clone(), doc_laser: s.laser.clone(), filter: Filter::default(), confirm: None }
    }
}

impl App {
    pub(super) fn library_window(&mut self, ctx: &egui::Context) {
        let Some(ed) = &mut self.library else { return };
        let mut close = false;
        // A fixed size that still fits small screens, so the layout does not jump between pages.
        let screen = ctx.content_rect().size();
        let size = egui::vec2((screen.x - 80.0).clamp(560.0, 980.0), (screen.y - 220.0).clamp(300.0, 560.0));
        // A centred modal: the drawing waits until the user saves or cancels.
        let modal = egui::Modal::new(egui::Id::unique("library-modal")).show(ctx, |ui| {
            ui.set_width(size.x);
            ui.horizontal(|ui| {
                ui.heading("Machines & materials");
                ui.add_space(24.0);
                ui.selectable_value(&mut ed.tab, Tab::Machines, RichText::new("Machines").size(15.0));
                ui.selectable_value(&mut ed.tab, Tab::Materials, RichText::new("Materials").size(15.0));
            });
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            ui.allocate_ui(size, |ui| {
                ui.set_min_size(size);
                match ed.tab {
                    Tab::Machines => machines::page(ui, ed),
                    Tab::Materials => materials::page(ui, ed),
                }
            });
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new(&ed.error).color(Color32::from_rgb(230, 90, 80)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = egui::Button::new(RichText::new("Save").strong().color(Color32::WHITE)).fill(ui.visuals().selection.bg_fill).min_size(egui::vec2(90.0, 28.0));
                    if ui.add(save).clicked() {
                        let lib = ed.lib.clone();
                        match library::update(|l| {
                            *l = lib;
                            Ok(())
                        }) {
                            Ok(()) => close = true,
                            Err(e) => ed.error = e,
                        }
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 28.0))).clicked() {
                        close = true;
                    }
                });
            });
        });
        // Shown after the window, so it sits on top of it.
        let answer = ed.confirm.as_ref().and_then(|c| widgets::confirm_modal(ctx, &c.question));
        match (answer, ed.confirm.take()) {
            (Some(true), Some(c)) => (c.apply)(&mut ed.lib),
            (None, c) => ed.confirm = c,
            _ => {}
        }
        let open = !modal.should_close();
        if !open || close {
            self.library = None;
        }
    }
}

/// The ribbon's Machine & material panel: the document's machine, laser and material, and the settings that apply.
pub(super) fn pickers(ui: &mut egui::Ui, d: &mut Document) {
    let lib = library::get();
    let s = &d.s;
    let machine = s.machine.as_deref().and_then(|id| lib.machine(id));
    let material = s.material.as_deref().and_then(|id| lib.material(id));
    let (mut new_machine, mut new_laser, mut new_material) = (s.machine.clone(), s.laser.clone(), s.material.clone());
    let mut use_area = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Machine").weak());
            ComboBox::from_id_salt("doc_machine").selected_text(machine.map_or("None", |m| &m.name)).width(150.0).show_ui(ui, |ui| {
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
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Material").weak());
            // A button that opens the picker with search and kinds; its state lives in egui's memory.
            let picker = egui::Id::unique("material-picker-state");
            let text = format!("{}  ▾", material.map_or("None", |m| &m.name));
            if ui.add(egui::Button::new(text).min_size(egui::vec2(150.0, 0.0))).on_hover_text("Choose the material").clicked() {
                ui.data_mut(|data| data.insert_temp(picker, Filter { only_ready: s.machine.is_some(), ..Filter::default() }));
            }
            if let Some(mut f) = ui.data(|data| data.get_temp::<Filter>(picker)) {
                match material_list::picker(ui.ctx(), &mut f, &lib, s.machine.as_deref(), s.laser.as_deref(), s.material.as_deref()) {
                    Picked::Nothing => {
                        ui.data_mut(|data| data.insert_temp(picker, f));
                    }
                    Picked::Closed => {
                        ui.data_mut(|data| data.remove::<Filter>(picker));
                    }
                    Picked::Chose(id) => {
                        new_material = id;
                        ui.data_mut(|data| data.remove::<Filter>(picker));
                    }
                }
            }
        });
        if let Some(m) = machine {
            let [w, h] = m.area(s.laser.as_deref());
            if [w, h] != [s.width, s.height] && ui.small_button(format!("Use work area {w} × {h} mm")).on_hover_text("Make the material as large as the machine's work area").clicked() {
                use_area = Some([w, h]);
            }
        }
    });
    // The settings for the chosen material, one line per operation.
    if let (Some(m), Some(mat)) = (machine, material) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let hints: Vec<String> = Operation::ALL.iter().filter_map(|op| mat.setting(&m.id, s.laser.as_deref(), *op).map(|x| format!("{}: {}", op.label(), x.hint()))).collect();
            if hints.is_empty() {
                ui.label(RichText::new("No settings for this material\non this machine yet\n(Machines & materials…)").weak().small());
            }
            for h in hints {
                ui.label(RichText::new(h).weak().small());
            }
        });
    }
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
