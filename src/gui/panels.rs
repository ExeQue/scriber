//! The panels around the canvas: menu bar, properties (right) and status bar (the ribbon is in ribbon/).

use super::App;
use super::snap::Aid;
use super::labels::{OPEN_SCORED, op_help, op_label};
use super::layers::layer_combo;
use crate::model::turn;
use crate::model::{BBox, Op, Shape, State, Transform};
use eframe::egui::{self, Color32, RichText};

impl App {
    pub(super) fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New  (Ctrl+N)").clicked() {
                    self.new_tab();
                }
                if ui.button("Open…  (Ctrl+O)").clicked() {
                    self.open_dialog();
                }
                let recent = self.ws.recent();
                ui.add_enabled_ui(!recent.is_empty(), |ui| {
                    ui.menu_button("Open recent", |ui| {
                        for f in recent {
                            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let b = ui.add_enabled(f.is_file(), egui::Button::new(name)).on_hover_text(f.display().to_string());
                            if b.clicked() {
                                self.open_path(&f);
                            }
                        }
                    });
                });
                if ui.button("Close  (Ctrl+W)").clicked() {
                    self.close_tab(self.doc_id);
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
                if ui.button("Cut  (Ctrl+X)").clicked() {
                    self.cut_selection(ui.ctx());
                }
                if ui.button("Copy  (Ctrl+C)").clicked() {
                    self.copy_selection(ui.ctx());
                }
                if ui.button("Paste  (Ctrl+V)").clicked() {
                    self.request_paste(ui.ctx(), None, false);
                }
                if ui.button("Paste in place  (Ctrl+Shift+V)").clicked() {
                    self.request_paste(ui.ctx(), None, true);
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
            ui.menu_button("AI agents", |ui| {
                if ui.button("Connect AI agents…").clicked() {
                    self.open_agents();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts").clicked() {
                    self.show_shortcuts = true;
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
        let hot = &mut self.hot_constraint;
        *hot = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            match sel.as_slice() {
                [] => {
                    ui.heading("Nothing selected");
                    ui.label("Pick a tool in the ribbon at the top and draw, or click a shape to change it.");
                }
                [id] => {
                    let metrics = d.item(*id).map(|i| i.shape.metrics());
                    let state = d.s.clone();
                    let item = d.item_mut(*id).unwrap();
                    ui.heading(if item.name.is_empty() { "Shape" } else { &item.name });
                    egui::Grid::new("item").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("Name");
                        changed |= ui.text_edit_singleline(&mut item.name).changed();
                        ui.end_row();
                        ui.label("Layer");
                        if let Some(l) = layer_combo(ui, "item-layer", &state, Some(item.layer)) {
                            item.layer = l;
                            changed = true;
                        }
                        ui.end_row();
                        let op = state.layer(item.layer).map_or(Op::Cut, |l| l.op);
                        ui.label("");
                        let what = if op == Op::Engrave && !item.shape.has_area() { OPEN_SCORED.to_string() } else { format!("Laser: {}", op_label(op)) };
                        ui.label(RichText::new(what).weak().small()).on_hover_text(op_help(op));
                        ui.end_row();
                    });
                    ui.separator();
                    changed |= shape_editor(ui, &mut item.shape);
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
                    let layers: Vec<u64> = d.s.items.iter().filter(|i| sel.contains(&i.id)).map(|i| i.layer).collect();
                    let shared = layers.first().copied().filter(|l| layers.iter().all(|m| m == l));
                    ui.horizontal(|ui| {
                        ui.label("Move to layer");
                        let state = d.s.clone();
                        if let Some(l) = layer_combo(ui, "sel-layer", &state, shared) {
                            d.s.items.iter_mut().filter(|i| sel.contains(&i.id)).for_each(|i| i.layer = l);
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
            if !sel.is_empty() && d.s.constraints.iter().any(|c| c.refs.iter().any(|r| sel.contains(&r.item()))) {
                ui.separator();
                let state = d.s.clone();
                param_change = super::constraint_list::constraint_rows(ui, param_edit, &state, &sel, hot);
            }
            if !sel.is_empty() {
                ui.separator();
                // The angle the selection shares; typing turns it about its box centre.
                if let Some(a) = turn::shared_rotation(d.s.items.iter().filter(|i| sel.contains(&i.id))) {
                    ui.horizontal(|ui| {
                        ui.label("Angle");
                        let mut x = a;
                        let r = ui.add(egui::DragValue::new(&mut x).speed(0.5).max_decimals(2).suffix("°"));
                        if r.on_hover_text("Turns the selection about the centre of its box (clockwise is positive).").changed() {
                            changed |= d.set_rotation(&sel, x).is_ok();
                        }
                    });
                }
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
                // POLAR's angle step: ▾ next to it (or a right-click on it).
                if a == Aid::Polar {
                    ui.menu_button("▾", |ui| self.polar_menu(ui)).response.on_hover_text("Polar angle step");
                }
                let r = ui.selectable_label(self.aid_on(a), RichText::new(a.label()).monospace()).on_hover_text(a.help());
                if r.clicked() {
                    self.toggle(a);
                }
                if a == Aid::Polar {
                    r.context_menu(|ui| self.polar_menu(ui));
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

/// "Cut: 1234 mm   Score: 56 mm   Engrave: 12 cm²" for the whole document (open engrave lines count as score).
fn laser_totals(s: &State) -> String {
    let mut parts: Vec<String> = [Op::Cut, Op::Score]
        .iter()
        .filter_map(|op| {
            let len: f64 = s.shown().filter(|i| s.effective_op(i) == *op && matches!(i.shape, Shape::Path { .. } | Shape::Text { .. })).map(|i| i.shape.metrics().length).sum();
            (len > 0.0).then(|| format!("{}: {:.0} mm", op_label(*op), len))
        })
        .collect();
    let area: f64 = s.shown().filter(|i| s.engraves_area(i)).filter_map(|i| i.shape.metrics().area).sum();
    if area > 0.0 {
        parts.push(format!("{}: {:.1} cm²", op_label(Op::Engrave), area / 100.0));
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
        for (key, val) in obj.iter_mut().filter(|(k, _)| !matches!(k.as_str(), "kind" | "image" | "contours" | "rotation")) {
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
