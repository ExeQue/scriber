//! The parameters window (AutoCAD's PARAMETERS): each dimensional constraint's name, expression
//! and value, editable. A change drives the geometry.

use super::App;
use crate::model::constraints::expr::fmt_num;
use crate::model::constraints::parameter_values;
use crate::model::State;
use eframe::egui::{self, RichText, TextEdit};

/// A text being typed: (constraint id, the name field (else the expression), text).
pub type ParamEdit = Option<(u64, bool, String)>;

pub enum ParamChange {
    Rename(u64, String),
    Set(u64, String),
    Delete(u64),
}

/// One editable field; returns the new text when an edit is finished (Enter or leaving the field).
pub(super) fn field(ui: &mut egui::Ui, edit: &mut ParamEdit, id: u64, name: bool, current: &str, width: f32) -> Option<String> {
    let mut text = match edit {
        Some((i, f, t)) if *i == id && *f == name => t.clone(),
        _ => current.to_string(),
    };
    let r = ui.add(TextEdit::singleline(&mut text).desired_width(width));
    if r.has_focus() {
        *edit = Some((id, name, text.clone()));
    }
    if r.lost_focus() {
        *edit = None;
        return (text.trim() != current).then(|| text.trim().to_string());
    }
    None
}

/// Rows for the dimensional constraints. Returns a finished change.
pub fn parameter_rows(ui: &mut egui::Ui, edit: &mut ParamEdit, s: &State) -> Option<ParamChange> {
    let values = parameter_values(s).unwrap_or_default();
    let mut change = None;
    let dims = s.constraints.iter().filter(|c| c.kind.is_dimension());
    egui::Grid::new("params").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
        ui.label(RichText::new("Name").weak());
        ui.label(RichText::new("Expression").weak());
        ui.label(RichText::new("Value").weak());
        ui.end_row();
        for c in dims {
            if let Some(n) = field(ui, edit, c.id, true, &c.name, 52.0) {
                change = Some(ParamChange::Rename(c.id, n));
            }
            if let Some(e) = field(ui, edit, c.id, false, &c.expr, 110.0) {
                change = Some(ParamChange::Set(c.id, e));
            }
            ui.label(values.get(&c.name).map_or("?".into(), |v| fmt_num(*v)));
            if ui.small_button("✕").on_hover_text("Remove this dimension (the shape stays as it is)").clicked() {
                change = Some(ParamChange::Delete(c.id));
            }
            ui.end_row();
        }
    });
    change
}

impl App {
    pub(super) fn parameters_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let s = self.doc.lock().unwrap().s.clone();
        let mut change = None;
        egui::Window::new("Parameters").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).open(&mut open).resizable(false).show(ctx, |ui| {
            if !s.constraints.iter().any(|c| c.kind.is_dimension()) {
                ui.label("No locked sizes yet. Use the Lock sizes buttons (DCLINEAR, DCRADIUS, …) to add some.");
                return;
            }
            change = parameter_rows(ui, &mut self.param_edit, &s);
            ui.label(RichText::new("Values are mm (degrees for angles). An expression can use other names: d1 / 2 + 3.").small().weak());
        });
        self.show_parameters = open;
        if let Some(c) = change {
            self.apply_param_change(c);
        }
    }

    /// One undo step; the geometry follows at once, or the change is refused with the reason.
    pub(super) fn apply_param_change(&mut self, change: ParamChange) {
        let mut d = self.doc.lock().unwrap();
        let before = d.s.clone();
        let name = |id: u64| before.constraints.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
        let result = match change {
            ParamChange::Rename(id, new) => d.rename_parameter(&name(id), &new),
            ParamChange::Set(id, value) => d.set_parameter(&name(id), &value),
            ParamChange::Delete(id) => d.delete_constraints(&[id]),
        };
        let result = result.and_then(|()| {
            d.push_undo(before);
            d.settle(true)
        });
        drop(d);
        if let Err(e) = result {
            self.message = format!("Not changed: {e}.");
            self.cmdline.log(self.message.clone());
        }
    }
}
