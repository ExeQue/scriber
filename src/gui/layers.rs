//! The Layers panel (right side, above the properties): one row per layer with the current-layer
//! marker, colour, name, show/lock toggles, laser operation, order, delete and the layer's own
//! power, speed and passes. Plus the layer drop-down used by the properties panel.

use super::App;
use super::labels::{op_help, op_label, rgb};
use crate::library::{self, Setting};
use crate::model::layer::op_operation;
use crate::model::{Layer, Op, Overrides, State};
use eframe::egui::{self, RichText, emath::Numeric};
use std::ops::RangeInclusive;

enum Action {
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
        let doc = self.doc.clone();
        let mut d = doc.lock().unwrap();
        let before = d.s.clone();
        let current = d.current_layer();
        let used: Vec<u64> = d.s.items.iter().map(|i| i.layer).collect();
        // The material library's preset per layer, to show and to start an override from.
        let presets: Vec<Option<Setting>> = {
            let lib = library::get();
            d.s.layers.iter().map(|l| op_operation(l.op).and_then(|op| Layer { settings: Overrides::default(), ..l.clone() }.setting(&d.s, &lib, op))).collect()
        };
        let count = d.s.layers.len();
        let mut changed = false;
        let mut action = None;
        egui::CollapsingHeader::new(RichText::new("Layers").heading()).default_open(true).show(ui, |ui| {
            for (k, (layer, preset)) in d.s.layers.iter_mut().zip(presets).enumerate() {
                ui.push_id(layer.id, |ui| {
                    changed |= layer_row(ui, layer, preset, current, used.contains(&layer.id), k, count, &mut action);
                });
            }
            if ui.button("➕ Add layer").on_hover_text("A new layer with the operation of the current one; new shapes go on it").clicked() {
                action = Some(Action::Add);
            }
        });

        if changed {
            if !self.layers_editing {
                d.push_undo(before);
            }
            d.touch();
            self.layers_editing = true;
        } else if !(ui.ctx().memory(|m| m.focused().is_some()) || ui.input(|i| i.pointer.any_down())) {
            self.layers_editing = false;
        }
        match action {
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
fn layer_row(ui: &mut egui::Ui, layer: &mut Layer, preset: Option<Setting>, current: u64, used: bool, k: usize, count: usize, action: &mut Option<Action>) -> bool {
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
        let eye = if layer.visible { "👁" } else { "—" };
        if ui.selectable_label(!layer.visible, eye).on_hover_text("Show or hide; hidden layers are not lasered or exported").clicked() {
            layer.visible = !layer.visible;
            changed = true;
        }
        if ui.selectable_label(layer.locked, "🔒").on_hover_text("Lock: shapes on it cannot be selected or edited").clicked() {
            layer.locked = !layer.locked;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(22.0);
        egui::ComboBox::from_id_salt("op").width(90.0).selected_text(op_label(layer.op)).show_ui(ui, |ui| {
            for op in Op::ALL {
                changed |= ui.selectable_value(&mut layer.op, op, op_label(op)).on_hover_text(op_help(op)).changed();
            }
        });
        if ui.add_enabled(k > 0, egui::Button::new("⏶").small()).on_hover_text("Move up").clicked() {
            *action = Some(Action::Move(layer.id, k - 1));
        }
        if ui.add_enabled(k + 1 < count, egui::Button::new("⏷").small()).on_hover_text("Move down").clicked() {
            *action = Some(Action::Move(layer.id, k + 1));
        }
        let why = if count == 1 { "The last layer cannot be deleted" } else if used { "Move its shapes to another layer first" } else { "Delete this layer" };
        if ui.add_enabled(count > 1 && !used, egui::Button::new("🗑").small()).on_hover_text(why).on_disabled_hover_text(why).clicked() {
            *action = Some(Action::Delete(layer.id));
        }
    });
    if op_operation(layer.op).is_some() {
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            egui::CollapsingHeader::new(RichText::new(settings_title(layer, preset.as_ref())).small()).id_salt("settings").show(ui, |ui| {
                changed |= settings_ui(ui, &mut layer.settings, preset.as_ref());
            });
        });
    }
    ui.add_space(4.0);
    changed
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
