//! Small building blocks shared by the machine and material pages.

use crate::library::Library;
use eframe::egui::{self, Align, Button, Color32, Layout, RichText};

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

/// The list on the left: a header (e.g. a search box), the rows in a scroll area, and
/// Add / Duplicate / Delete under it.
pub(super) fn sidebar(ui: &mut egui::Ui, noun: &str, has_selection: bool, header: impl FnOnce(&mut egui::Ui), rows: impl FnOnce(&mut egui::Ui)) -> Option<ListAction> {
    let mut action = None;
    let height = ui.available_height();
    egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(6).inner_margin(6).show(ui, |ui| {
        ui.set_width(SIDEBAR_WIDTH);
        ui.set_height(height - 12.0);
        header(ui);
        egui::ScrollArea::vertical().id_salt("list").max_height(ui.available_height() - 40.0).auto_shrink([false, true]).show(ui, |ui| ui.vertical(rows));
        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.horizontal(|ui| {
                if ui.button("+ Add").on_hover_text(format!("Add a new {noun}")).clicked() {
                    action = Some(ListAction::Add);
                }
                ui.add_enabled_ui(has_selection, |ui| {
                    if ui.button("Duplicate").on_hover_text(format!("Copy this {noun}")).clicked() {
                        action = Some(ListAction::Duplicate);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if remove_button(ui, &format!("this {noun}")) {
                            action = Some(ListAction::Delete);
                        }
                    });
                });
            });
        });
    });
    action
}

/// One row of a plain list: as wide as the list, text on the left. Returns whether it was clicked.
pub(super) fn list_row(ui: &mut egui::Ui, selected: bool, text: &str) -> bool {
    ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| ui.add(Button::selectable(selected, text).min_size(egui::vec2(0.0, 26.0))).clicked()).inner
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

/// The red cross that deletes something (after asking).
pub(super) fn remove_button(ui: &mut egui::Ui, what: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    let enabled = ui.is_enabled();
    let color = match (enabled, response.hovered()) {
        (false, _) => ui.visuals().weak_text_color(),
        (true, true) => Color32::from_rgb(240, 70, 60),
        (true, false) => Color32::from_rgb(200, 80, 70),
    };
    let r = rect.shrink(5.0);
    let stroke = egui::Stroke::new(2.0, color);
    ui.painter().line_segment([r.left_top(), r.right_bottom()], stroke);
    ui.painter().line_segment([r.right_top(), r.left_bottom()], stroke);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("Delete {what}")).clicked()
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
