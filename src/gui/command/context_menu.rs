//! The right-click menu on the canvas, like AutoCAD's shortcut menu: while a command runs it
//! offers Enter, Cancel and the command's options; otherwise it offers the last command and
//! what can be done with the selection.

use super::card::{next_label, title};
use super::parse::Entry;
use super::{App, Cmd, Step};
use crate::model::Kind;
use eframe::egui;

impl App {
    /// The menu stays open for a click into one of its value fields (radius, height, text, …),
    /// so a value can be typed there; a click on anything else (a button, an entry) closes it.
    pub(in crate::gui) fn show_canvas_menu(&mut self, response: &egui::Response) {
        let popup = egui::Popup::context_menu(response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        let id = popup.get_id();
        popup.show(|ui| {
            self.canvas_menu(ui);
            let ctx = ui.ctx().clone();
            // A clicked widget that took the keyboard focus is a field to type in; an open submenu
            // handles its own clicks.
            let clicked = ctx.viewport(|v| v.interact_widgets.clicked).filter(|_| ctx.input(|i| i.pointer.primary_clicked()));
            if clicked.is_some_and(|w| !ctx.memory(|m| m.has_focus(w))) && egui::containers::menu::MenuState::is_deepest_open_sub_menu(&ctx, id) {
                ui.close();
            }
        });
    }

    fn canvas_menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(190.0);
        if self.osnap_menu_open {
            return self.osnap_menu(ui);
        }
        match self.cmdline.active {
            Some((cmd, step)) => self.command_menu(ui, cmd, step),
            None => match self.menu_constraint {
                Some(id) => self.constraint_menu_for(ui, id),
                None => self.idle_menu(ui),
            },
        }
    }

    /// A right-click on a badge or dimension label opens its menu; on a shape (with no command
    /// running) it selects that shape, so "Add constraint" applies to it. With one other shape
    /// selected, both stay selected.
    pub(in crate::gui) fn right_click(&mut self, at: egui::Pos2) {
        // Shift+right-click while a point is asked for: the one-shot object snap menu.
        self.menu_at = Some(self.s2w(at));
        self.osnap_menu_open = self.shift && self.active_cmd().is_some();
        if self.osnap_menu_open {
            return;
        }
        self.menu_constraint = self.constraint_at(at);

        if self.menu_constraint.is_some() || self.active_cmd().is_some() {
            return;
        }
        let mut d = self.doc.lock();
        let Some(id) = self.hit(&d, self.s2w(at)) else { return };
        if !d.selection.contains(&id) {
            if d.selection.len() == 1 {
                d.selection.push(id);
            } else {
                d.selection = vec![id];
            }
        }
    }

    fn command_menu(&mut self, ui: &mut egui::Ui, cmd: Cmd, step: Step) {
        if ui.button(next_label(cmd, step)).clicked() {
            self.answer(Entry::Empty, None);
        }
        if ui.button("Cancel").clicked() {
            self.escape();
        }
        // The running tool's settings (radius, sides, height, mode, …), as on the command card.
        ui.separator();
        self.card_fields(ui, cmd, step);
        self.dyn_fields(ui);
        let placed = self.draft.len();
        let options: &[(&str, &str)] = match (cmd, step) {
            (Cmd::Pline, Step::Draw) if placed >= 3 => &[("Close shape", "CLOSE"), ("Undo last point", "UNDO")],
            (Cmd::Pline, Step::Draw) if placed >= 1 => &[("Undo last point", "UNDO")],
            (Cmd::Circle, Step::Draw) if placed == 1 => &[("Diameter", "DIAMETER")],
            (Cmd::Rectang, Step::Draw) if placed == 0 => &[("Rounded corners…", "FILLET")],
            (Cmd::Fillet | Cmd::Chamfer, Step::Pick) => &[("All corners of the selection", "ALL")],
            (Cmd::Mirror, Step::EraseSource) => &[("Keep the original", "NO"), ("Delete the original", "YES")],
            _ => &[],
        };
        if !options.is_empty() {
            ui.separator();
        }
        for (label, word) in options {
            if ui.button(*label).clicked() {
                self.answer(Entry::Word((*word).into()), None);
            }
        }
    }

    /// The common edits at the top, the clipboard and undo with their keys, the rest in submenus.
    fn idle_menu(&mut self, ui: &mut egui::Ui) {
        if let Some(last) = self.cmdline.last
            && ui.button(format!("Repeat {}", title(last))).clicked()
        {
            self.start(last);
        }
        let (selected, grouped) = {
            let d = self.doc.lock();
            (d.selection.len(), d.selection.iter().any(|id| d.s.groups.iter().any(|g| g.contains(id))))
        };
        if selected > 0 {
            ui.separator();
            // "Copy" is the clipboard's below; the COPY command places copies from a base point.
            for (cmd, label) in [(Cmd::Move, title(Cmd::Move)), (Cmd::Copy, "Copy with base point…"), (Cmd::Rotate, title(Cmd::Rotate)), (Cmd::Erase, title(Cmd::Erase))] {
                if ui.button(label).on_hover_text(cmd.name()).clicked() {
                    self.start(cmd);
                }
            }
            self.comment_menu(ui);
            ui.menu_button("Modify", |ui| {
                let mut cmds = vec![Cmd::Scale, Cmd::Mirror, Cmd::Offset, Cmd::Array, Cmd::Stretch, Cmd::Explode, Cmd::Align, Cmd::MatchProp];
                if selected >= 2 {
                    cmds.extend([Cmd::Join, Cmd::Group, Cmd::Union, Cmd::Subtract, Cmd::Intersect]);
                }
                if grouped {
                    cmds.push(Cmd::Ungroup);
                }
                self.cmd_buttons(ui, &cmds);
            });
        }
        ui.separator();
        if selected > 0 {
            if ui.add(egui::Button::new("Cut to clipboard").shortcut_text("Ctrl+X")).clicked() {
                self.cut_selection(ui.ctx());
            }
            if ui.add(egui::Button::new("Copy to clipboard").shortcut_text("Ctrl+C")).clicked() {
                self.copy_selection(ui.ctx());
            }
        }
        if self.clip.is_some() {
            if ui.add(egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
                self.request_paste(ui.ctx(), self.menu_at, false);
            }
            if ui.add(egui::Button::new("Paste in place").shortcut_text("Ctrl+Shift+V")).clicked() {
                self.request_paste(ui.ctx(), None, true);
            }
        }
        if ui.add(egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
            self.doc.lock().undo();
        }
        if ui.add(egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
            self.doc.lock().redo();
        }
        ui.separator();
        ui.menu_button("Select", |ui| {
            let mut cmds = if selected > 0 { vec![Cmd::SelectSimilar, Cmd::QuickSelect, Cmd::Isolate] } else { vec![Cmd::QuickSelect] };
            if self.isolated.is_some() {
                cmds.push(Cmd::Unisolate);
            }
            self.cmd_buttons(ui, &cmds);
            if ui.add(egui::Button::new("Select all").shortcut_text("Ctrl+A")).clicked() {
                self.select_all();
            }
            if selected > 0 && ui.button("Deselect all").clicked() {
                self.doc.lock().selection.clear();
            }
        });
        ui.menu_button("Constraints", |ui| self.constraint_menu(ui));
        ui.menu_button("View", |ui| {
            if ui.button("Zoom window").clicked() {
                self.start(Cmd::Zoom);
            }
            if ui.button("Previous view").clicked() {
                self.zoom_previous();
            }
            if selected > 0 && ui.button("Zoom to selection").clicked() {
                self.zoom_selection();
            }
            if ui.button("Show whole material").clicked() {
                self.view.zoom_extents();
            }
        });
    }

    /// One button per command, titled as on the ribbon, with the typed name as tooltip.
    fn cmd_buttons(&mut self, ui: &mut egui::Ui, cmds: &[Cmd]) {
        for &cmd in cmds {
            if ui.button(title(cmd)).on_hover_text(cmd.name()).clicked() {
                self.start(cmd);
            }
        }
    }

    /// "Add constraint" and the constraint commands that work on the selection.
    fn constraint_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button(title(Cmd::Dim)).on_hover_text("DIM: lock a length, distance, angle, radius or diameter").clicked() {
            self.start(Cmd::Dim);
        }
        ui.menu_button("Add constraint", |ui| {
            for kind in Kind::GEOMETRIC {
                if icon_item(ui, kind, title(Cmd::Constrain(kind))).clicked() {
                    self.start(Cmd::Constrain(kind));
                }
            }
            ui.separator();
            let dims = [(Cmd::DcLinear, Kind::DimHorizontal), (Cmd::Constrain(Kind::DimAligned), Kind::DimAligned), (Cmd::Constrain(Kind::DimAngular), Kind::DimAngular), (Cmd::Constrain(Kind::DimRadial), Kind::DimRadial), (Cmd::Constrain(Kind::DimDiameter), Kind::DimDiameter)];
            for (cmd, kind) in dims {
                if icon_item(ui, kind, title(cmd)).clicked() {
                    self.start(cmd);
                }
            }
        });
        self.cmd_buttons(ui, &[Cmd::AutoConstrain, Cmd::DelConstraint, Cmd::ConstraintBar, Cmd::Parameters]);
    }
}

/// A menu entry with the constraint's glyph before its name, like the ribbon shows it.
fn icon_item(ui: &mut egui::Ui, kind: Kind, label: &str) -> egui::Response {
    let height = 22.0;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), egui::Color32::WHITE);
    // As wide as the icon and the text need (the menu then sizes itself to its widest entry).
    let width = (4.0 + 16.0 + 8.0 + galley.size().x + 12.0).max(140.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
    }
    let icon = egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 3.0), egui::vec2(16.0, 16.0));
    crate::gui::glyphs::paint(ui.painter(), icon, visuals.text_color(), kind);
    ui.painter().text(egui::pos2(icon.right() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, label, font, visuals.text_color());
    if response.clicked() {
        ui.close();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Document;
    use egui::{Event, PointerButton, Pos2, RawInput, Rect};

    /// One frame of the canvas with its right-click menu; returns the labelled widgets' centres.
    fn frame(ctx: &egui::Context, app: &mut App, events: Vec<Event>) -> Vec<(String, Pos2)> {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            let r = ui.allocate_rect(ui.max_rect(), egui::Sense::click());
            app.show_canvas_menu(&r);
        });
        out.textures_delta.clear();
        let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
        nodes.iter().filter_map(|(_, n)| Some((n.label()?.to_string(), n.bounds()?))).map(|(l, b)| (l, Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))).collect()
    }

    fn click(ctx: &egui::Context, app: &mut App, at: Pos2, button: PointerButton) -> Vec<(String, Pos2)> {
        let press = |pressed| Event::PointerButton { pos: at, button, pressed, modifiers: Default::default() };
        frame(ctx, app, vec![Event::PointerMoved(at), press(true)]);
        frame(ctx, app, vec![press(false)]);
        frame(ctx, app, vec![])
    }

    fn find(widgets: &[(String, Pos2)], label: &str) -> Pos2 {
        widgets.iter().find(|(l, _)| l == label).unwrap_or_else(|| panic!("no {label:?} in {widgets:?}")).1
    }

    /// With a selection: the common edits and the clipboard (named apart from the laser's cut and
    /// the COPY command) on top, the rest in submenus that run their command and close.
    #[test]
    fn idle_menu_groups_the_long_tail() {
        let mut doc = Document::new(100.0, 100.0);
        let id = doc.add("disc", crate::model::Op::Cut, crate::model::Shape::path(crate::model::Contour::circle(50.0, 50.0, 10.0)));
        doc.selection = vec![id];
        let mut app = App::new(crate::model::Workspace::new(doc), Err(String::new()));
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        frame(&ctx, &mut app, vec![]);
        let menu = click(&ctx, &mut app, Pos2::new(600.0, 200.0), PointerButton::Secondary);
        let labels: Vec<&str> = menu.iter().map(|(l, _)| l.as_str()).collect();
        for want in ["Move", "Copy with base point…", "Rotate", "Delete", "Modify", "Select", "Constraints", "View"] {
            find(&menu, want);
        }
        assert!(labels.iter().any(|l| l.starts_with("Cut to clipboard")) && labels.iter().any(|l| l.starts_with("Copy to clipboard")), "{labels:?}");
        assert!(!labels.iter().any(|l| ["Copy", "Cut", "Scale", "Select all"].contains(l)), "ambiguous or long-tail entries on top: {labels:?}");
        // Hover Modify, then click Scale in its submenu.
        let modify = find(&menu, "Modify");
        frame(&ctx, &mut app, vec![Event::PointerMoved(modify)]);
        let sub = frame(&ctx, &mut app, vec![]);
        let scale = find(&sub, "Scale");
        assert!(scale.x > modify.x, "Scale sits in the submenu");
        frame(&ctx, &mut app, vec![Event::PointerMoved(Pos2::new(modify.x + 60.0, modify.y)), Event::PointerMoved(scale)]);
        let after = click(&ctx, &mut app, scale, PointerButton::Primary);
        assert_eq!(app.cmdline.active.map(|(c, _)| c), Some(Cmd::Scale));
        assert!(after.iter().all(|(l, _)| l != "Modify"), "running a submenu entry closes the menu");
    }

    #[test]
    fn menu_font_and_values_work() {
        let mut app = App::new(crate::model::Workspace::new(Document::new(100.0, 100.0)), Err(String::new()));
        app.start(Cmd::Text);
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        frame(&ctx, &mut app, vec![]);
        let menu = click(&ctx, &mut app, Pos2::new(200.0, 200.0), PointerButton::Secondary);
        // Hover the font entry: its submenu lists the fonts; pick one.
        let font = find(&menu, "sans-serif");
        frame(&ctx, &mut app, vec![Event::PointerMoved(font)]);
        let list = frame(&ctx, &mut app, vec![]);
        let serif = list.iter().filter(|(l, _)| l == "serif").map(|w| w.1).find(|p| p.x > font.x).expect("font list");
        frame(&ctx, &mut app, vec![Event::PointerMoved(Pos2::new(font.x + 60.0, font.y)), Event::PointerMoved(serif)]);
        let after = click(&ctx, &mut app, serif, PointerButton::Primary);
        assert_eq!(app.cmdline.text_font, "serif");
        assert!(after.iter().all(|(l, _)| l != "Cancel"), "picking a font closes the menu");

        // A value field in the menu takes a typed value without closing the menu.
        let menu = click(&ctx, &mut app, Pos2::new(200.0, 200.0), PointerButton::Secondary);
        let height = find(&menu, "mm");
        let still = click(&ctx, &mut app, height, PointerButton::Primary);
        find(&still, "Cancel");
        // A button still closes it.
        let gone = click(&ctx, &mut app, find(&still, "Cancel"), PointerButton::Primary);
        assert!(gone.iter().all(|(l, _)| l != "Cancel"));
        assert!(app.cmdline.active.is_none());
    }
}
