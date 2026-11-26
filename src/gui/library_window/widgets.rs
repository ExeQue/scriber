//! Small building blocks shared by the machine and material pages.

use crate::library::Library;
use crate::gui::lucide;
use eframe::egui::{self, Align, Button, Color32, Layout, RichText};
use std::cmp::Ordering;

/// Width of the labels in front of the form fields, so the fields line up.
pub(super) const LABEL_WIDTH: f32 = 120.0;
/// Width of the list on the left.
const SIDEBAR_WIDTH: f32 = 220.0;

pub(super) enum ListAction {
    Add,
    Duplicate,
    Delete,
}

/// A delete waiting for the user to say yes.
pub(super) struct Confirm {
    pub question: String,
    pub apply: Box<dyn FnOnce(&mut Library) + Send>,
}

impl Confirm {
    pub(super) fn ask(question: String, apply: impl FnOnce(&mut Library) + Send + 'static) -> Option<Confirm> {
        Some(Confirm { question, apply: Box::new(apply) })
    }
}

/// The list on the left, exactly `SIDEBAR_WIDTH` wide and as high as the space it gets: a header
/// (e.g. a search box), the rows in a scroll area, and Add / Duplicate / Delete in a row under it.
pub(super) fn sidebar(ui: &mut egui::Ui, noun: &str, has_selection: bool, header: impl FnOnce(&mut egui::Ui), rows: impl FnOnce(&mut egui::Ui)) -> Option<ListAction> {
    let mut action = None;
    let height = ui.available_height();
    // Its own top-down column: a frame would otherwise take the caller's horizontal layout and
    // put the header, the list and the buttons side by side.
    ui.allocate_ui_with_layout(egui::vec2(SIDEBAR_WIDTH, height), Layout::top_down(Align::Min), |ui| {
        egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(6).inner_margin(6).show(ui, |ui| {
            ui.set_width(SIDEBAR_WIDTH - 12.0);
            ui.set_height(height - 12.0);
            header(ui);
            let list = (ui.available_height() - 34.0).max(60.0);
            egui::ScrollArea::vertical().id_salt("list").max_height(list).min_scrolled_height(list).auto_shrink([false, false]).show(ui, |ui| ui.vertical(rows));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if ui.button(format!("{} Add", lucide::PLUS)).on_hover_text(format!("Add a new {noun}")).clicked() {
                    action = Some(ListAction::Add);
                }
                let duplicate = ui.add_enabled(has_selection, Button::new(format!("{} Duplicate", lucide::COPY)));
                if duplicate.on_hover_text(format!("Copy this {noun}")).clicked() {
                    action = Some(ListAction::Duplicate);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if lucide::delete(ui, has_selection, &format!("Delete this {noun}")).clicked() {
                        action = Some(ListAction::Delete);
                    }
                });
            });
        });
    });
    action
}

/// Right of the sidebar: `form` fills the rest of the page in a vertical scroll area.
pub(super) fn form_column(ui: &mut egui::Ui, id: &str, form: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(12.0);
    ui.allocate_ui_with_layout(ui.available_size(), Layout::top_down(Align::Min), |ui| {
        // The column's width is fixed: help text wraps to it and wide tables cannot widen it.
        let width = ui.available_width() - 14.0;
        egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(width);
            form(ui)
        });
    });
}

/// One row of a plain list: as wide as the list, text on the left. Returns whether it was clicked.
pub(super) fn list_row(ui: &mut egui::Ui, selected: bool, text: &str) -> bool {
    ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| ui.add(Button::selectable(selected, text).truncate().min_size(egui::vec2(0.0, 26.0))).on_hover_text(text).clicked()).inner
}

/// A titled block with a line of plain help under the title.
pub(super) fn section(ui: &mut egui::Ui, title: &str, help: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(title).size(16.0).strong());
    ui.label(RichText::new(help).weak());
    ui.add_space(6.0);
    add_contents(ui);
    ui.add_space(18.0);
}

/// A label of fixed width in a form grid, so the fields after it line up.
pub(super) fn field_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.allocate_ui_with_layout(egui::vec2(LABEL_WIDTH, 20.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(LABEL_WIDTH);
        ui.label(text)
    })
    .inner
}

/// A column title in a table.
pub(super) fn column_title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(RichText::new(text).weak())
}

/// The red ✕ that deletes something (after asking).
pub(super) fn remove_button(ui: &mut egui::Ui, what: &str) -> bool {
    lucide::delete(ui, ui.is_enabled(), &format!("Delete {what}")).clicked()
}

/// Compares names the way people count: "3 mm" < "10 mm", "1.5 mm" < "2 mm"; text ignores case.
pub(super) fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        let (Some(x), Some(y)) = (a.chars().next(), b.chars().next()) else { return a.len().cmp(&b.len()) };
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let ((n, rest_a), (m, rest_b)) = (number(a), number(b));
            match n.total_cmp(&m) {
                Ordering::Equal => (a, b) = (rest_a, rest_b),
                o => return o,
            }
        } else {
            match x.to_lowercase().cmp(y.to_lowercase()) {
                Ordering::Equal => (a, b) = (&a[x.len_utf8()..], &b[y.len_utf8()..]),
                o => return o,
            }
        }
    }
}

/// The number at the start of `s` (digits, optionally a point and more digits), and the rest.
fn number(s: &str) -> (f64, &str) {
    let digits = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let mut end = digits(s);
    if s[end..].starts_with('.') && s[end + 1..].starts_with(|c: char| c.is_ascii_digit()) {
        end += 1 + digits(&s[end + 1..]);
    }
    (s[..end].parse().unwrap_or(0.0), &s[end..])
}

/// The "Delete …?" question on top of the window. Returns Some(yes) once the user answers.
pub(super) fn confirm_modal(ctx: &egui::Context, question: &str) -> Option<bool> {
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::unique("library-confirm")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.label(RichText::new(question).strong());
        ui.label(RichText::new("Nothing is saved until you press Save.").weak());
        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let delete = Button::new(RichText::new("Delete").strong().color(Color32::WHITE)).fill(Color32::from_rgb(200, 70, 60)).min_size(egui::vec2(80.0, 26.0));
            if ui.add(delete).clicked() {
                answer = Some(true);
            }
            if ui.add(Button::new("Cancel").min_size(egui::vec2(80.0, 26.0))).clicked() {
                answer = Some(false);
            }
        });
    });
    answer.or(modal.should_close().then_some(false))
}

/// Notes and where the numbers come from, as full-width text boxes.
pub(super) fn notes_section(ui: &mut egui::Ui, notes: &mut String, source: &mut String) {
    section(ui, "Notes", "Anything worth remembering, and where the numbers come from.", |ui| {
        ui.label("Notes");
        ui.add(egui::TextEdit::multiline(notes).desired_width(f32::INFINITY).desired_rows(3).hint_text("e.g. tips, what worked well"));
        ui.add_space(6.0);
        ui.label("Where the numbers come from");
        ui.add(egui::TextEdit::multiline(source).desired_width(f32::INFINITY).desired_rows(3).hint_text("e.g. a web page or \"own tests\""));
    });
}

#[cfg(test)]
mod tests {
    use super::natural_cmp;

    #[test]
    fn natural_order() {
        let mut names = vec!["Basswood plywood 15 mm", "basswood plywood 3 mm", "Basswood plywood 10 mm", "Acrylic 1.5 mm", "Basswood plywood 8 mm", "Acrylic 10 mm", "Basswood plywood 6 mm", "Acrylic 2 mm", "Basswood plywood"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["Acrylic 1.5 mm", "Acrylic 2 mm", "Acrylic 10 mm", "Basswood plywood", "basswood plywood 3 mm", "Basswood plywood 6 mm", "Basswood plywood 8 mm", "Basswood plywood 10 mm", "Basswood plywood 15 mm"]);
    }
}
