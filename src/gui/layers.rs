//! Layers: a short list in the right panel (current layer, colour, name, operation, show and lock)
//! and the Edit layers dialog with everything else (colour, name, operation, order, delete and the
//! layer's own power, speed and passes). Plus the layer drop-down used by the properties panel.

use super::App;
use super::lucide;
use super::labels::{op_help, op_label, rgb};
use crate::library::{self, Setting};
use crate::model::layer::op_operation;
use crate::model::{Layer, Op, Overrides, State};
use eframe::egui::{self, RichText, emath::Numeric};
use std::ops::RangeInclusive;

pub(super) enum Action {
    Current(u64),
    Add,
    Delete(u64),
    Move(u64, usize),
}

/// A drop-down of the document's layers (colour, name, op); returns the layer picked.
/// `current` None shows "Mixed" (a selection on several layers).
pub(super) fn layer_combo(ui: &mut egui::Ui, salt: &str, s: &State, current: Option<u64>) -> Option<u64> {
    let text = current.and_then(|id| s.layer(id)).map_or("Mixed".to_string(), |l| l.name.clone());
    let mut picked = None;
    egui::ComboBox::from_id_salt(salt).selected_text(text).show_ui(ui, |ui| {
        for l in &s.layers {
            ui.horizontal(|ui| {
                ui.colored_label(rgb(l.rgb()), "■");
                let label = format!("{}  ·  {}", l.name, op_label(l.op));
                if ui.selectable_label(current == Some(l.id), label).on_hover_text(op_help(l.op)).clicked() {
                    picked = Some(l.id);
                }
            });
        }
    });
    picked
}

impl App {
    pub(super) fn layers_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("Layers").heading()).default_open(true).show(ui, |ui| {
            self.layers_ui(ui, false);
            if ui.button(format!("{}  Edit layers…", lucide::PENCIL)).on_hover_text("Colours, names, operations, order and each layer's power, speed and passes").clicked() {
                self.show_layers = true;
            }
        });
    }

    pub(super) fn layers_dialog(&mut self, ctx: &egui::Context) {
        let modal = egui::Modal::new(egui::Id::unique("layers-modal")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.heading("Layers");
            ui.label(RichText::new("Each layer has its laser operation. Power, speed and passes come from the material library unless a layer sets its own.").weak());
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.7).show(ui, |ui| self.layers_ui(ui, true));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(format!("{}  Add layer", lucide::PLUS)).on_hover_text("A new layer with the operation of the current one; new shapes go on it").clicked() {
                    self.layer_action = Some(Action::Add);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.button("Close").clicked())
                    .inner
            })
            .inner
        });
        self.apply_layer_action();
        if modal.inner || modal.should_close() {
            self.show_layers = false;
        }
    }

    /// The layer rows: short in the panel, everything in the dialog.
    fn layers_ui(&mut self, ui: &mut egui::Ui, full: bool) {
        let doc = self.doc.clone();
        let mut d = doc.lock().unwrap();
        let before = d.s.clone();
        let current = d.current_layer();
        let used: Vec<u64> = d.s.items.iter().map(|i| i.layer).collect();
        // The material library's preset per layer, to show and to start an override from.
        // And the material's profiles for the layer's op on the document's machine and laser.
        let (presets, profiles): (Vec<Option<Setting>>, Vec<Vec<String>>) = {
            let lib = library::get();
            let material = d.s.material.as_deref().and_then(|id| lib.material(id));
            d.s.layers
                .iter()
                .map(|l| {
                    let op = op_operation(l.op);
                    let preset = op.and_then(|op| Layer { settings: Overrides::default(), ..l.clone() }.setting(&d.s, &lib, op));
                    let names = match (op, material, d.s.machine.as_deref()) {
                        (Some(op), Some(m), Some(machine)) => m.profile_names(machine, d.s.laser.as_deref(), op),
                        _ => Vec::new(),
                    };
                    (preset, names)
                })
                .unzip()
        };
        let count = d.s.layers.len();
        let mut changed = false;
        let mut action = None;
        for (k, ((layer, preset), names)) in d.s.layers.iter_mut().zip(presets).zip(profiles).enumerate() {
            ui.push_id(layer.id, |ui| {
                changed |= if full { layer_row(ui, layer, preset, &names, current, used.contains(&layer.id), k, count, &mut action) } else { short_row(ui, layer, current, &mut action) };
            });
        }

        if changed {
            if !self.layers_editing {
                d.push_undo(before);
            }
            d.touch();
            self.layers_editing = true;
        } else if !(ui.ctx().memory(|m| m.focused().is_some()) || ui.input(|i| i.pointer.any_down())) {
            self.layers_editing = false;
        }
        drop(d);
        if action.is_some() {
            self.layer_action = action;
        }
        if !full {
            self.apply_layer_action();
        }
    }

    fn apply_layer_action(&mut self) {
        let doc = self.doc.clone();
        let mut d = doc.lock().unwrap();
        let current = d.current_layer();
        match self.layer_action.take() {
            Some(Action::Current(id)) => d.set_current_layer(id),
            Some(Action::Add) => {
                d.checkpoint();
                let op = d.s.layer(current).map_or(Op::Cut, |l| l.op);
                let id = d.add_layer(op.label(), op);
                d.set_current_layer(id);
            }
            Some(Action::Delete(id)) => {
                d.checkpoint();
                if let Err(e) = d.delete_layer(id, None) {
                    d.undo();
                    self.message = e;
                }
            }
            Some(Action::Move(id, to)) => {
                d.checkpoint();
                d.move_layer(id, to).ok();
            }
            None => {}
        }
        // Shapes on hidden or locked layers cannot stay selected.
        let s = d.s.clone();
        d.selection.retain(|id| s.items.iter().any(|i| i.id == *id && s.editable(i)));
    }
}

/// One layer: returns whether it changed; buttons that need the whole document set `action`.
#[allow(clippy::too_many_arguments)]
fn layer_row(ui: &mut egui::Ui, layer: &mut Layer, preset: Option<Setting>, profiles: &[String], current: u64, used: bool, k: usize, count: usize, action: &mut Option<Action>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.radio(layer.id == current, "").on_hover_text("Current layer: new shapes go here").clicked() {
            *action = Some(Action::Current(layer.id));
        }
        let mut c = layer.rgb();
        if ui.color_edit_button_srgb(&mut c).on_hover_text("Colour on the canvas").changed() {
            layer.color = Some(c);
            changed = true;
        }
        changed |= ui.add(egui::TextEdit::singleline(&mut layer.name).desired_width(96.0)).changed();
        changed |= show_lock(ui, layer);
        egui::ComboBox::from_id_salt("op").width(90.0).selected_text(op_label(layer.op)).show_ui(ui, |ui| {
            for op in Op::ALL {
                changed |= ui.selectable_value(&mut layer.op, op, op_label(op)).on_hover_text(op_help(op)).changed();
            }
        });
        if ui.add_enabled(k > 0, egui::Button::new(lucide::CHEVRON_UP).frame(false)).on_hover_text("Move up").clicked() {
            *action = Some(Action::Move(layer.id, k - 1));
        }
        if ui.add_enabled(k + 1 < count, egui::Button::new(lucide::CHEVRON_DOWN).frame(false)).on_hover_text("Move down").clicked() {
            *action = Some(Action::Move(layer.id, k + 1));
        }
        let why = if count == 1 { "The last layer cannot be deleted" } else if used { "Move its shapes to another layer first" } else { "Delete this layer" };
        if lucide::delete(ui, count > 1 && !used, why).clicked() {
            *action = Some(Action::Delete(layer.id));
        }
    });
    if op_operation(layer.op).is_some() {
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            changed |= profile_combo(ui, layer, profiles);
            egui::CollapsingHeader::new(RichText::new(settings_title(layer, preset.as_ref())).small()).id_salt("settings").show(ui, |ui| {
                changed |= settings_ui(ui, &mut layer.settings, preset.as_ref());
            });
        });
    }
    ui.add_space(4.0);
    changed
}

/// Show/hide and lock toggles.
fn show_lock(ui: &mut egui::Ui, layer: &mut Layer) -> bool {
    let mut changed = false;
    let eye = if layer.visible { lucide::EYE } else { lucide::EYE_OFF };
    if lucide::button(ui, eye, !layer.visible, "Show or hide; hidden layers are not lasered or exported").clicked() {
        layer.visible = !layer.visible;
        changed = true;
    }
    let lock = if layer.locked { lucide::LOCK } else { lucide::LOCK_OPEN };
    if lucide::button(ui, lock, layer.locked, "Lock: shapes on it cannot be selected or edited").clicked() {
        layer.locked = !layer.locked;
        changed = true;
    }
    changed
}

/// The panel's row: current-layer marker, colour, name and operation, show and lock.
fn short_row(ui: &mut egui::Ui, layer: &mut Layer, current: u64, action: &mut Option<Action>) -> bool {
    ui.horizontal(|ui| {
        if ui.radio(layer.id == current, "").on_hover_text("Current layer: new shapes go here").clicked() {
            *action = Some(Action::Current(layer.id));
        }
        let (swatch, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(swatch, 3.0, rgb(layer.rgb()));
        let name = ui.add(egui::Label::new(&layer.name).sense(egui::Sense::click())).on_hover_text(op_help(layer.op));
        if name.clicked() {
            *action = Some(Action::Current(layer.id));
        }
        ui.label(RichText::new(op_label(layer.op)).weak().small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Right to left: lock first.
            let mut changed = false;
            let lock = if layer.locked { lucide::LOCK } else { lucide::LOCK_OPEN };
            if lucide::button(ui, lock, layer.locked, "Lock: shapes on it cannot be selected or edited").clicked() {
                layer.locked = !layer.locked;
                changed = true;
            }
            let eye = if layer.visible { lucide::EYE } else { lucide::EYE_OFF };
            if lucide::button(ui, eye, !layer.visible, "Show or hide; hidden layers are not lasered or exported").clicked() {
                layer.visible = !layer.visible;
                changed = true;
            }
            changed
        })
        .inner
    })
    .inner
}

/// "100 % · 4 mm/s · 2×" with the layer's own numbers, or why there are none.
fn settings_title(layer: &Layer, preset: Option<&Setting>) -> String {
    let o = layer.settings;
    let num = |own: Option<String>, lib: Option<String>| own.or(lib).unwrap_or("?".into());
    if o.is_empty() && preset.is_none() {
        return "No setting for this material".into();
    }
    let power = num(o.power.map(|p| p.to_string()), preset.map(|p| p.power.to_string()));
    let speed = num(o.speed.map(|p| p.to_string()), preset.map(|p| p.speed.to_string()));
    let passes = num(o.passes.map(|p| p.to_string()), preset.map(|p| p.passes.to_string()));
    let own = if o.is_empty() { "" } else { " (own)" };
    format!("{power} % · {speed} mm/s · {passes}×{own}")
}

/// The material's profile for this layer (e.g. "Light", "Photo"), when it has more than one.
fn profile_combo(ui: &mut egui::Ui, layer: &mut Layer, profiles: &[String]) -> bool {
    if profiles.len() < 2 {
        return false;
    }
    let label = |name: &str, first: bool| match (name.is_empty(), first) {
        (true, _) => "Default".to_string(),
        (false, true) => format!("{name} (default)"),
        (false, false) => name.to_string(),
    };
    // A missing or unknown name means the default, the first one.
    let chosen = layer.profile.as_deref().and_then(|p| profiles.iter().position(|n| n == p)).unwrap_or(0);
    let mut pick = chosen;
    egui::ComboBox::from_id_salt("profile").width(110.0).selected_text(label(&profiles[chosen], chosen == 0)).show_ui(ui, |ui| {
        for (k, name) in profiles.iter().enumerate() {
            ui.selectable_value(&mut pick, k, label(name, k == 0));
        }
    })
    .response
    .on_hover_text("Which of the material's settings this layer uses");
    if pick == chosen {
        return false;
    }
    layer.profile = (pick > 0).then(|| profiles[pick].clone());
    true
}

fn settings_ui(ui: &mut egui::Ui, o: &mut Overrides, preset: Option<&Setting>) -> bool {
    let from = |f: fn(&Setting) -> f64, or: f64| preset.map_or(or, f);
    let mut changed = false;
    egui::Grid::new("own").num_columns(2).show(ui, |ui| {
        changed |= own(ui, "Power", &mut o.power, from(|s| s.power, 50.0), 0.0..=100.0, " %");
        changed |= own(ui, "Speed", &mut o.speed, from(|s| s.speed, 10.0), 0.1..=1000.0, " mm/s");
        changed |= own(ui, "Passes", &mut o.passes, preset.map_or(1, |s| s.passes), 1..=50, "×");
    });
    let source = match preset {
        Some(p) => format!("Unticked: the material library's {} ({})", p.operation.label().to_lowercase(), p.hint()),
        None => "Unticked: the material library (no setting for this material, machine and operation)".into(),
    };
    ui.label(RichText::new(source).weak().small());
    changed
}

/// A ticked number overrides the library; unticked shows the library's value greyed out.
fn own<T: Numeric>(ui: &mut egui::Ui, label: &str, v: &mut Option<T>, default: T, range: RangeInclusive<T>, suffix: &str) -> bool {
    let mut on = v.is_some();
    let mut changed = ui.checkbox(&mut on, label).changed();
    if changed {
        *v = on.then_some(default);
    }
    let mut x = v.unwrap_or(default);
    if ui.add_enabled(on, egui::DragValue::new(&mut x).range(range).suffix(suffix)).changed() {
        *v = Some(x);
        changed = true;
    }
    ui.end_row();
    changed
}
