//! The command card: a small panel on the canvas while a command runs. It says what to do in
//! plain words and has fields and buttons for everything the command line asks for, so the
//! commands can be used without typing. It drives the same command engine as the command line.

use super::parse::Entry;
use super::{App, Cmd, Step};
use eframe::egui::{self, DragValue, RichText};

impl App {
    pub(in crate::gui) fn command_card(&mut self, ctx: &egui::Context) {
        let Some((cmd, step)) = self.cmdline.active else { return };
        let at = self.canvas_rect.left_top() + egui::vec2(12.0, 12.0);
        egui::Area::new(egui::Id::unique("command-card")).fixed_pos(at).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(12.0).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.label(RichText::new(title(cmd)).strong().size(16.0));
                ui.label(RichText::new(self.prompt()).weak());
                ui.add_space(6.0);
                self.card_fields(ui, cmd, step);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    // ARRAY is finished with its own "Create copies" button.
                    let next = !(cmd == Cmd::Array && step == Step::Value);
                    if next && ui.button(next_label(cmd, step)).on_hover_text("Same as Enter").clicked() {
                        self.answer(Entry::Empty, None);
                    }
                    if ui.button("Cancel").on_hover_text("Same as Esc").clicked() {
                        self.escape();
                    }
                });
            });
        });
    }

    /// The settings and options for this command and step.
    fn card_fields(&mut self, ui: &mut egui::Ui, cmd: Cmd, step: Step) {
        match (cmd, step) {
            (Cmd::Rectang, _) => {
                ui.horizontal(|ui| {
                    ui.label("Rounded corners");
                    ui.add(mm_value(&mut self.cmdline.fillet));
                });
            }
            (Cmd::Polygon, _) => {
                ui.horizontal(|ui| {
                    ui.label("Sides");
                    ui.add(DragValue::new(&mut self.sides).range(3..=64));
                });
            }
            (Cmd::Text, Step::Content) => {
                ui.label("Text");
                // Edits the command line's input, so Enter here or there places the text.
                let field = ui.text_edit_singleline(&mut self.cmdline.input);
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.submit(None);
                } else if !field.has_focus() && !ui.ctx().text_edit_focused() {
                    field.request_focus();
                }
            }
            (Cmd::Text, _) => {
                ui.horizontal(|ui| {
                    ui.label("Height");
                    ui.add(mm_value(&mut self.cmdline.text_height));
                });
            }
            (Cmd::Pline, Step::Draw) => {
                ui.horizontal(|ui| {
                    if self.draft.len() >= 3 && ui.button("Close shape").clicked() {
                        self.answer(Entry::Word("CLOSE".into()), None);
                    }
                    if !self.draft.is_empty() && ui.button("Undo last point").clicked() {
                        self.answer(Entry::Word("UNDO".into()), None);
                    }
                });
            }
            (Cmd::Circle, Step::Draw) if !self.draft.is_empty() => {
                if ui.button("Give the diameter instead").clicked() {
                    self.answer(Entry::Word("DIAMETER".into()), None);
                }
            }
            (Cmd::Fillet, _) => {
                ui.horizontal(|ui| {
                    ui.label("Radius");
                    ui.add(mm_value(&mut self.cmdline.modify.radius));
                });
                self.corner_buttons(ui, step);
            }
            (Cmd::Chamfer, _) => {
                ui.horizontal(|ui| {
                    ui.label("Distance");
                    ui.add(mm_value(&mut self.cmdline.modify.chamfer));
                });
                self.corner_buttons(ui, step);
            }
            (Cmd::Offset, _) => {
                ui.horizontal(|ui| {
                    ui.label("Distance");
                    ui.add(mm_value(&mut self.cmdline.modify.offset).range(0.01..=10000.0));
                });
            }
            (Cmd::Rotate, Step::Second) => {
                ui.horizontal(|ui| {
                    ui.label("Angle");
                    ui.add(DragValue::new(&mut self.cmdline.card_angle).speed(1.0).suffix("°"));
                    if ui.button("Rotate").clicked() {
                        let a = self.cmdline.card_angle;
                        self.answer(Entry::Number(a), None);
                    }
                });
                ui.label(RichText::new("Counter-clockwise; or click a point on the drawing.").small().weak());
            }
            (Cmd::Scale, Step::Second) => {
                ui.horizontal(|ui| {
                    ui.label("Factor");
                    ui.add(DragValue::new(&mut self.cmdline.card_factor).speed(0.01).range(0.001..=1000.0));
                    if ui.button("Scale").clicked() {
                        let f = self.cmdline.card_factor;
                        self.answer(Entry::Number(f), None);
                    }
                });
            }
            (Cmd::Mirror, Step::EraseSource) => {
                ui.horizontal(|ui| {
                    if ui.button("Keep the original").clicked() {
                        self.answer(Entry::Word("NO".into()), None);
                    }
                    if ui.button("Delete the original").clicked() {
                        self.answer(Entry::Word("YES".into()), None);
                    }
                });
            }
            (Cmd::Array, Step::Value) => self.array_fields(ui),
            _ => {}
        }
    }

    fn corner_buttons(&mut self, ui: &mut egui::Ui, step: Step) {
        if step != Step::Pick {
            return;
        }
        let has_selection = !self.doc.lock().unwrap().selection.is_empty();
        if has_selection && ui.button("Round every corner of the selection").clicked() {
            self.answer(Entry::Word("ALL".into()), None);
        }
        ui.label(RichText::new("Or click a corner, or click two lines one after the other.").small().weak());
    }

    fn array_fields(&mut self, ui: &mut egui::Ui) {
        let m = &mut self.cmdline.modify;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut m.polar, false, "Rows and columns");
            ui.selectable_value(&mut m.polar, true, "Around a centre");
        });
        egui::Grid::new("array").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            if m.polar {
                ui.label("Copies (incl. original)");
                ui.add(DragValue::new(&mut m.count).range(2..=1000));
                ui.end_row();
                ui.label("Angle to fill");
                ui.add(DragValue::new(&mut m.angle).range(-360.0..=360.0).suffix("°"));
                ui.end_row();
                ui.label("Centre");
                ui.label(m.centre.map_or("click it on the drawing".into(), |c| format!("{:.1}, {:.1}", c[0], c[1])));
                ui.end_row();
            } else {
                ui.label("Rows");
                ui.add(DragValue::new(&mut m.rows).range(1..=1000));
                ui.end_row();
                ui.label("Columns");
                ui.add(DragValue::new(&mut m.columns).range(1..=1000));
                ui.end_row();
                ui.label("Distance between rows");
                ui.add(DragValue::new(&mut m.row_gap).speed(0.1).suffix(" mm"));
                ui.end_row();
                ui.label("Distance between columns");
                ui.add(DragValue::new(&mut m.column_gap).speed(0.1).suffix(" mm"));
                ui.end_row();
            }
        });
        // Polar needs a centre: let canvas clicks go to the command while it is missing.
        self.tool = if m.polar && m.centre.is_none() { super::Tool::Pick } else { super::Tool::Select };
        if ui.button("Create copies").clicked()
            && let Err(e) = self.array_now()
        {
            self.cmdline.log(e);
        }
    }
}

fn mm_value(v: &mut f64) -> DragValue<'_> {
    DragValue::new(v).speed(0.1).range(0.0..=10000.0).suffix(" mm")
}

pub(super) fn title(cmd: Cmd) -> &'static str {
    match cmd {
        Cmd::Line => "Line",
        Cmd::Circle => "Circle",
        Cmd::Arc => "Arc",
        Cmd::Rectang => "Rectangle",
        Cmd::Polygon => "Polygon",
        Cmd::Pline => "Polyline",
        Cmd::Text => "Text",
        Cmd::DimLinear => "Dimension",
        Cmd::Dist => "Measure distance",
        Cmd::Erase => "Delete",
        Cmd::Move => "Move",
        Cmd::Copy => "Copy",
        Cmd::Rotate => "Rotate",
        Cmd::Scale => "Scale",
        Cmd::Mirror => "Mirror",
        Cmd::Undo => "Undo",
        Cmd::Redo => "Redo",
        Cmd::Fillet => "Round corner (fillet)",
        Cmd::Chamfer => "Cut corner (chamfer)",
        Cmd::Offset => "Offset",
        Cmd::Trim => "Trim",
        Cmd::Array => "Copies in a pattern (array)",
        Cmd::Union => "Merge shapes",
        Cmd::Subtract => "Cut out",
        Cmd::Intersect => "Keep the overlap",
    }
}

/// The text of the button that does what Enter does at this point.
pub(super) fn next_label(cmd: Cmd, step: Step) -> &'static str {
    match step {
        Step::Select | Step::Choose => "Done selecting",
        Step::Value | Step::Sides | Step::Fillet | Step::Height => "Next",
        Step::Content => "Place text",
        Step::EraseSource => "Keep the original",
        _ if matches!(cmd, Cmd::Line | Cmd::Pline | Cmd::Copy | Cmd::Fillet | Cmd::Chamfer | Cmd::Offset | Cmd::Trim) => "Done",
        _ => "Finish",
    }
}
