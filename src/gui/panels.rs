//! The panels around the canvas: menu bar, toolbar (left), properties (right) and status bar.

use super::App;
use super::snap::Aid;
use super::labels::{fill_help, op_color, op_help, op_label};
use crate::model::{BBox, Op, Shape, State, Transform};
use eframe::egui::{self, Color32, RichText};

impl App {
    pub(super) fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.new_doc();
                }
                if ui.button("Open…  (Ctrl+O)").clicked() {
                    self.open_dialog();
                }
                if ui.button("Save  (Ctrl+S)").clicked() {
                    self.save(false);
                }
                if ui.button("Save as…").clicked() {
                    self.save(true);
                }
                ui.separator();
                if ui.button("Import image…").clicked() {
                    self.import_image_dialog();
                }
                if ui.button("Import SVG…").clicked() {
                    self.import_svg_dialog();
                }
                if ui.button("Export SVG for the laser…").clicked() {
                    self.export_svg();
                }
                if ui.button("Export PNG…").clicked() {
                    self.export_png();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Undo  (Ctrl+Z)").clicked() {
                    self.doc.lock().unwrap().undo();
                }
                if ui.button("Redo  (Ctrl+Y)").clicked() {
                    self.doc.lock().unwrap().redo();
                }
                ui.separator();
                if ui.button("Duplicate  (Ctrl+D)").clicked() {
                    self.duplicate_selection();
                }
                if ui.button("Combine  (Ctrl+G)").clicked() {
                    self.combine_selection();
                }
                if ui.button("Break apart  (Ctrl+Shift+G)").clicked() {
                    self.break_apart_selection();
                }
                if ui.button("Delete  (Del)").clicked() {
                    self.delete_selection();
                }
                if ui.button("Select all  (Ctrl+A)").clicked() {
                    self.select_all();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Show whole material").clicked() {
                    self.view.zoom_extents();
                }
                ui.checkbox(&mut self.snap, "Snap to grid");
                ui.checkbox(&mut self.show_cmdline, "Command line (for CAD users)");
            });
            ui.menu_button("Laser", |ui| self.laser_menu(ui));
            ui.menu_button("AI agents", |ui| {
                if ui.button("Connect AI agents…").clicked() {
                    self.open_agents();
                }
            });
        });
    }

    pub(super) fn properties(&mut self, ui: &mut egui::Ui) {
        let doc = self.doc.clone();
        let mut d = doc.lock().unwrap();
        let before = d.s.clone();
        let sel = d.selection.clone();
        let mut changed = false;
        let mut action = None;
        let mut param_change = None;
        let param_edit = &mut self.param_edit;

        egui::ScrollArea::vertical().show(ui, |ui| {
            match sel.as_slice() {
                [] => {
                    ui.heading("Nothing selected");
                    ui.label("Pick a tool on the left and draw, or click a shape to change it.");
                }
                [id] => {
                    let metrics = d.item(*id).map(|i| i.shape.metrics());
                    let item = d.item_mut(*id).unwrap();
                    ui.heading(if item.name.is_empty() { "Shape" } else { &item.name });
                    egui::Grid::new("item").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("Name");
                        changed |= ui.text_edit_singleline(&mut item.name).changed();
                        ui.end_row();
                        if !matches!(item.shape, Shape::Image { .. }) {
                            ui.label("Laser");
                            egui::ComboBox::from_id_salt("op").selected_text(op_label(item.op)).show_ui(ui, |ui| {
                                for op in Op::ALL {
                                    changed |= ui.selectable_value(&mut item.op, op, op_label(op)).on_hover_text(op_help(op)).changed();
                                }
                            });
                            ui.end_row();
                            ui.label("");
                            changed |= ui.checkbox(&mut item.fill, "Fill (engrave area)").on_hover_text(fill_help()).changed();
                            ui.end_row();
                        }
                    });
                    ui.separator();
                    changed |= shape_editor(ui, &mut item.shape);
                    if d.s.constraints.iter().any(|c| c.kind.is_dimension() && c.refs.iter().any(|r| r.item() == *id)) {
                        ui.separator();
                        ui.label(RichText::new("Locked sizes").strong()).on_hover_text("Dimensional constraints: change a value and the shape follows.");
                        let state = d.s.clone();
                        param_change = super::parameters::parameter_rows(ui, param_edit, &state, Some(*id));
                    }
                    let item = d.item_mut(*id).unwrap();
                    ui.separator();
                    ui.label("Note");
                    changed |= ui.add(egui::TextEdit::multiline(&mut item.note).desired_rows(2).desired_width(f32::INFINITY)).changed();
                    if let Some(m) = metrics {
                        ui.separator();
                        metrics_ui(ui, m.bbox, m.length, m.area);
                    }
                }
                many => {
                    ui.heading(format!("{} {}", many.len(), "shapes selected"));
                    ui.horizontal_wrapped(|ui| {
                        for op in Op::ALL {
                            if ui.button(RichText::new(op_label(op)).color(op_color(op))).on_hover_text(op_help(op)).clicked() {
                                d.s.items.iter_mut().filter(|i| sel.contains(&i.id)).for_each(|i| i.op = op);
                                changed = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Fill all").on_hover_text(fill_help()).clicked() {
                            d.s.items.iter_mut().filter(|i| sel.contains(&i.id)).for_each(|i| i.fill = true);
                            changed = true;
                        }
                        if ui.button("No fill").clicked() {
                            d.s.items.iter_mut().filter(|i| sel.contains(&i.id)).for_each(|i| i.fill = false);
                            changed = true;
                        }
                    });
                    let len: f64 = d.s.items.iter().filter(|i| sel.contains(&i.id)).map(|i| i.shape.metrics().length).sum();
                    if let Some(b) = d.bbox(&sel) {
                        ui.separator();
                        metrics_ui(ui, b, len, None);
                    }
                }
            }
            if !sel.is_empty() {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    let buttons: [(&str, Action); 7] = [
                        ("⟳ Rotate 90°", Action::Transform(Transform { rotate: 90.0, ..Default::default() })),
                        ("⇆ Mirror", Action::Transform(Transform { mirror_x: true, ..Default::default() })),
                        ("⇅ Flip", Action::Transform(Transform { mirror_y: true, ..Default::default() })),
                        ("⧉ Duplicate", Action::Duplicate),
                        ("◫ Combine", Action::Combine),
                        ("◰ Break apart", Action::BreakApart),
                        ("🗑 Delete", Action::Delete),
                    ];
                    for (label, a) in buttons {
                        if ui.button(label).clicked() {
                            action = Some(a);
                        }
                    }
                });
            }

            ui.add_space(12.0);
            ui.heading("Material");
            ui.horizontal(|ui| {
                ui.label("Width");
                changed |= ui.add(egui::DragValue::new(&mut d.s.width).range(1.0..=5000.0).suffix(" mm")).changed();
                ui.label("Height");
                changed |= ui.add(egui::DragValue::new(&mut d.s.height).range(1.0..=5000.0).suffix(" mm")).changed();
            });
            let totals = laser_totals(&d.s);
            if !totals.is_empty() {
                ui.label(RichText::new(totals).small());
            }
            ui.add_space(8.0);
            ui.label("Notes");
            changed |= ui.add(egui::TextEdit::multiline(&mut d.s.notes).desired_rows(5).desired_width(f32::INFINITY)).changed();
        });

        // One undo step per edit: the first change after the panel was idle saves the state from before it.
        let busy = ui.ctx().memory(|m| m.focused().is_some()) || ui.input(|i| i.pointer.any_down());
        if changed {
            if !self.panel_editing {
                d.push_undo(before);
            }
            d.touch();
            self.panel_editing = true;
        } else if !busy {
            self.panel_editing = false;
        }
        drop(d);

        if let Some(c) = param_change {
            self.apply_param_change(c);
        }
        match action {
            Some(Action::Transform(t)) => self.transform_selection(t),
            Some(Action::Duplicate) => self.duplicate_selection(),
            Some(Action::Combine) => self.combine_selection(),
            Some(Action::BreakApart) => self.break_apart_selection(),
            Some(Action::Delete) => self.delete_selection(),
            None => {}
        }
    }

    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui, pointer: Option<[f64; 2]>) {
        // Right side first (fixed width), then the hint in whatever space is left, cut off if too long.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match &self.mcp {
                Ok(port) => ui.label(RichText::new(format!("● MCP :{port}")).color(Color32::from_rgb(40, 160, 70))).on_hover_text("AI agents can connect."),
                Err(e) => ui.label(RichText::new("● MCP").color(Color32::from_rgb(200, 50, 50))).on_hover_text(format!("AI agents cannot connect (is Scriber already running?): {e}")),
            };
            for a in Aid::ALL.into_iter().rev() {
                if ui.selectable_label(self.aid_on(a), RichText::new(a.label()).monospace()).on_hover_text(a.help()).clicked() {
                    self.toggle(a);
                }
            }
            // 96 DPI screen: 3.78 px per mm is 100 %.
            ui.label(format!("{:.0} %", self.view.zoom / 3.78 * 100.0));
            if let Some(p) = pointer {
                ui.label(RichText::new(format!("{:>8.2}, {:>8.2}", p[0], p[1])).monospace());
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let text = if self.message.is_empty() { self.tool.hint(self.draft.len()).to_string() } else { self.message.clone() };
                ui.add(egui::Label::new(RichText::new(text).weak()).truncate());
            });
        });
    }
}

/// Buttons under the properties that act on the selection (run after the panel lets go of the document).
enum Action {
    Transform(Transform),
    Duplicate,
    Combine,
    BreakApart,
    Delete,
}

fn metrics_ui(ui: &mut egui::Ui, b: BBox, length: f64, area: Option<f64>) {
    egui::Grid::new("metrics").num_columns(2).show(ui, |ui| {
        ui.label("Size");
        ui.label(format!("{:.2} × {:.2} mm", b[2], b[3]));
        ui.end_row();
        ui.label("Position");
        ui.label(format!("x {:.2}, y {:.2} mm", b[0], b[1]));
        ui.end_row();
        ui.label("Line length");
        ui.label(format!("{length:.2} mm"));
        ui.end_row();
        if let Some(a) = area {
            ui.label("Area");
            ui.label(format!("{:.2} cm²", a / 100.0));
            ui.end_row();
        }
    });
}

/// "Cut: 1234 mm   Score: 56 mm   Fill: 12 cm²" for the whole document.
fn laser_totals(s: &State) -> String {
    let mut parts: Vec<String> = [Op::Cut, Op::Score, Op::Engrave]
        .iter()
        .filter_map(|op| {
            let len: f64 = s.items.iter().filter(|i| i.op == *op && matches!(i.shape, Shape::Path { .. } | Shape::Text { .. })).map(|i| i.shape.metrics().length).sum();
            (len > 0.0).then(|| format!("{}: {:.0} mm", op_label(*op), len))
        })
        .collect();
    let fill: f64 = s.items.iter().filter(|i| i.engraves_area()).filter_map(|i| i.shape.metrics().area).sum();
    if fill > 0.0 {
        parts.push(format!("{}: {:.1} cm²", "Fill", fill / 100.0));
    }
    parts.join("   ")
}

fn field_label(key: &str) -> String {
    let label = match key {
        "w" => "Width",
        "h" => "Height",
        "size" => "Size",
        "text" => "Text",
        "rotation" => "Rotation",
        "offset" => "Distance",
        "font" => "Font",
        "bold" => "Bold",
        "italic" => "Italic",
        _ => return key.to_string(),
    };
    label.to_string()
}

/// Edit any shape's numbers and text through its JSON form, so every shape kind gets a form for free.
fn shape_editor(ui: &mut egui::Ui, shape: &mut Shape) -> bool {
    let mut v = serde_json::to_value(&*shape).unwrap();
    let mut changed = false;
    let obj = v.as_object_mut().unwrap();
    if obj["kind"] == "text" {
        // Left out of the JSON while false; show them anyway.
        for key in ["bold", "italic"] {
            obj.entry(key).or_insert(false.into());
        }
    }
    egui::Grid::new("shape").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (key, val) in obj.iter_mut().filter(|(k, _)| !matches!(k.as_str(), "kind" | "image" | "contours")) {
            ui.label(field_label(key));
            if key == "font" {
                let mut font = val.as_str().unwrap_or_default().to_string();
                if super::fonts::font_picker(ui, "shape-font", &mut font) {
                    *val = font.into();
                    changed = true;
                }
            } else if let Some(mut b) = val.as_bool() {
                if ui.checkbox(&mut b, "").changed() {
                    *val = b.into();
                    changed = true;
                }
            } else if let Some(n) = val.as_f64() {
                let mut x = n;
                let suffix = if key == "rotation" { "°" } else { " mm" };
                if ui.add(egui::DragValue::new(&mut x).speed(0.1).max_decimals(3).suffix(suffix)).changed() {
                    *val = x.into();
                    changed = true;
                }
            } else if let Some(s) = val.as_str() {
                let mut s = s.to_string();
                if ui.text_edit_singleline(&mut s).changed() {
                    *val = s.into();
                    changed = true;
                }
            }
            ui.end_row();
        }
    });
    if let Some(contours) = obj.get_mut("contours").and_then(|v| v.as_array_mut()) {
        for (ci, contour) in contours.iter_mut().enumerate() {
            let count = contour["vertices"].as_array().map_or(0, Vec::len);
            let title = format!("{} {} ({count} {})", "Contour", ci + 1, "points");
            egui::CollapsingHeader::new(title).id_salt(("contour", ci)).show(ui, |ui| {
                let mut closed = contour["closed"].as_bool().unwrap_or(false);
                if ui.checkbox(&mut closed, "Closed").changed() {
                    contour["closed"] = closed.into();
                    changed = true;
                }
                for (i, vert) in contour["vertices"].as_array_mut().into_iter().flatten().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}", i + 1));
                        for k in ["x", "y", "bulge"] {
                            let mut x = vert[k].as_f64().unwrap_or(0.0);
                            let prefix = if k == "bulge" { "arc " } else if k == "x" { "x " } else { "y " };
                            let speed = if k == "bulge" { 0.01 } else { 0.1 };
                            if ui.add(egui::DragValue::new(&mut x).speed(speed).max_decimals(3).prefix(prefix)).changed() {
                                vert[k] = x.into();
                                changed = true;
                            }
                        }
                    });
                }
            });
        }
    }
    if changed && let Ok(s) = serde_json::from_value(v) {
        *shape = s;
    }
    changed
}
