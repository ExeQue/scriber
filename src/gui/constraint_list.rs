//! Constraints as the user sees them outside the canvas badges: the list in the properties panel
//! ("Parallel to Line 3", "Locked length d1 = 40 mm") and the right-click menu on a badge or a
//! dimension's label.

use super::App;
use super::parameters::{ParamChange, ParamEdit, field};
use crate::model::constraints::expr::fmt_num;
use crate::model::constraints::parameter_values;
use crate::model::{Constraint, Kind, State};
use eframe::egui::{self, Pos2, RichText};

/// "Line 3": the item's name with its id.
fn item_name(s: &State, id: u64) -> String {
    let name = s.items.iter().find(|i| i.id == id).map_or("", |i| i.name.as_str());
    let mut chars = name.chars();
    match chars.next() {
        Some(f) => format!("{}{} {id}", f.to_uppercase(), chars.as_str()),
        None => format!("Shape {id}"),
    }
}

/// What a kind of constraint does, and the word before what it binds to ("Parallel", "to").
fn words(kind: Kind) -> (&'static str, &'static str) {
    match kind {
        Kind::Coincident => ("Joined", "to"),
        Kind::Collinear => ("In line", "with"),
        Kind::Concentric => ("Same centre", "as"),
        Kind::Fix => ("Locked in place", ""),
        Kind::Parallel => ("Parallel", "to"),
        Kind::Perpendicular => ("Perpendicular", "to"),
        Kind::Horizontal => ("Horizontal", ""),
        Kind::Vertical => ("Vertical", ""),
        Kind::Tangent => ("Tangent", "to"),
        Kind::Smooth => ("Joined smoothly", "to"),
        Kind::Symmetric => ("Symmetric", "with"),
        Kind::Equal => ("Equal", "to"),
        Kind::Regular => ("Regular (equal sides and corners)", ""),
        Kind::DimHorizontal => ("Locked horizontal distance", ""),
        Kind::DimVertical => ("Locked vertical distance", ""),
        Kind::DimAligned => ("Locked length", ""),
        Kind::DimAngular => ("Locked angle", ""),
        Kind::DimRadial => ("Locked radius", ""),
        Kind::DimDiameter => ("Locked diameter", ""),
    }
}

/// What the constraint does, in plain words, from the point of view of `items`.
pub fn describe(s: &State, c: &Constraint, items: &[u64]) -> String {
    let (what, to) = words(c.kind);
    if c.kind.is_dimension() {
        return format!("{what} {}", c.name);
    }
    let mut others: Vec<u64> = c.refs.iter().map(|r| r.item()).filter(|i| !items.contains(i)).collect();
    others.dedup();
    if others.is_empty() || to.is_empty() {
        return what.to_string();
    }
    format!("{what} {to} {}", others.iter().map(|i| item_name(s, *i)).collect::<Vec<_>>().join(" and "))
}

/// The properties panel's list of the constraints on `items`: each with a remove button, the
/// dimensional ones with their name and value to edit. Hovering a row sets `hot` (highlighted on the canvas).
pub fn constraint_rows(ui: &mut egui::Ui, edit: &mut ParamEdit, s: &State, items: &[u64], hot: &mut Option<u64>) -> Option<ParamChange> {
    let on: Vec<&Constraint> = s.constraints.iter().filter(|c| c.refs.iter().any(|r| items.contains(&r.item()))).collect();
    if on.is_empty() {
        return None;
    }
    let values = parameter_values(s).unwrap_or_default();
    let mut change = None;
    ui.label(RichText::new("Constraints").strong()).on_hover_text("What keeps this shape bound to others or to a size. Point at one to see it on the drawing.");
    for c in on {
        let row = ui.horizontal(|ui| {
            if ui.small_button("✕").on_hover_text("Remove this constraint (the shape stays as it is)").clicked() {
                change = Some(ParamChange::Delete(c.id));
            }
            if !c.kind.is_dimension() {
                ui.label(describe(s, c, items));
            } else {
                ui.label(words(c.kind).0);
                if let Some(n) = field(ui, edit, c.id, true, &c.name, 52.0) {
                    change = Some(ParamChange::Rename(c.id, n));
                }
                ui.label("=");
                if let Some(e) = field(ui, edit, c.id, false, &c.expr, 70.0) {
                    change = Some(ParamChange::Set(c.id, e));
                }
                let unit = if c.kind == Kind::DimAngular { "°" } else { "mm" };
                let value = values.get(&c.name).map_or("?".into(), |v| fmt_num(*v));
                ui.label(if c.expr.trim() == value { unit.to_string() } else { format!("({value} {unit})") });
            }
        });
        if row.response.contains_pointer() {
            *hot = Some(c.id);
        }
    }
    change
}

impl App {
    /// The constraint whose badge or dimension label is at screen position `at`.
    pub(super) fn constraint_at(&self, at: Pos2) -> Option<u64> {
        self.bar_badges.iter().chain(&self.dim_labels).find(|(_, r)| r.contains(at)).map(|(id, _)| *id)
    }

    /// Right-click on a badge or dimension label.
    pub(super) fn constraint_menu_for(&mut self, ui: &mut egui::Ui, id: u64) {
        let (label, dimension) = {
            let d = self.doc.lock();
            let Some(c) = d.constraint(id) else { return };
            (describe(&d.s, c, &[]), c.kind.is_dimension())
        };
        ui.label(RichText::new(label).weak());
        if dimension && ui.button("Edit value…").clicked() {
            self.edit_dimension(id);
            ui.close();
        }
        if ui.button("Delete constraint").clicked() {
            self.apply_param_change(ParamChange::Delete(id));
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Contour, Document, Op, Ref, Shape};

    #[test]
    fn plain_names() {
        let mut d = Document::new(100.0, 100.0);
        let a = d.add("line", Op::Cut, Shape::path(Contour::line([0.0, 0.0], [10.0, 0.0])));
        let b = d.add("line", Op::Cut, Shape::path(Contour::line([0.0, 5.0], [10.0, 5.0])));
        let id = d.add_constraint(Kind::Parallel, vec![Ref::Segment { item: a, contour: 0, segment: 0 }, Ref::Segment { item: b, contour: 0, segment: 0 }]).unwrap();
        assert_eq!(describe(&d.s, d.constraint(id).unwrap(), &[a]), format!("Parallel to Line {b}"));
        let id = d.add_dimension(Kind::DimAligned, vec![Ref::Segment { item: a, contour: 0, segment: 0 }], None, None, 5.0).unwrap();
        assert_eq!(describe(&d.s, d.constraint(id).unwrap(), &[a]), "Locked length d1");
    }
}
