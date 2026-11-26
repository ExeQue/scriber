//! STRETCH, ALIGN, MATCHPROP, BREAK, EXTEND and LENGTHEN on the canvas. The geometry is in
//! `model::editing`; this file asks for the points and picks, AutoCAD style. Each run is one undo step.

use super::parse::{Entry, is_option};
use super::ux::window;
use super::{App, Cmd};
use crate::gui::tools::Tool;
use crate::model::Point;
use eframe::egui::{self, DragValue, RichText};

impl App {
    pub(super) fn reshape_step(&mut self, cmd: Cmd, entry: Entry, pointer: Option<Point>) -> Result<(), String> {
        match (cmd, &entry) {
            (Cmd::Align, Entry::Empty) if self.cmdline.ux.pts.len() == 2 => return self.align_now(),
            (Cmd::MatchProp, Entry::Empty) if self.cmdline.ux.pick.is_some() && !self.doc.lock().unwrap().selection.is_empty() => {
                let (source, _) = self.cmdline.ux.pick.unwrap();
                self.apply(|d, ids| d.match_properties(source, ids))?;
                self.end();
                return Ok(());
            }
            (_, Entry::Empty) => {
                self.end();
                return Ok(());
            }
            (Cmd::Lengthen, Entry::Word(w)) if is_option(w, "DElta") || is_option(w, "Total") => {
                self.cmdline.ux.lengthen_total = is_option(w, "Total");
                return Ok(());
            }
            (Cmd::Lengthen, Entry::Number(n)) => {
                let u = &mut self.cmdline.ux;
                if u.lengthen_total { u.total = n.abs() } else { u.delta = *n }
                return Ok(());
            }
            (Cmd::Break, Entry::Word(w)) if is_option(w, "At") => {
                let (id, p) = self.cmdline.ux.pick.take().ok_or("Select the object first.")?;
                return self.apply_ids(&[], |d, _| d.break_item(id, p, None).map(drop));
            }
            _ => {}
        }
        let p = self.point_of(&entry, pointer).ok_or("Specify a point.")?;
        self.cmdline.last_point = Some(p);
        match cmd {
            Cmd::Stretch => self.stretch_point(p),
            Cmd::Align => {
                let u = &mut self.cmdline.ux;
                u.pts.push(p);
                // Show the line from each source point to where it goes.
                self.draft = if u.pts.len() % 2 == 1 { vec![p] } else { Vec::new() };
                if u.pts.len() == 4 {
                    return self.align_now();
                }
                Ok(())
            }
            Cmd::MatchProp => {
                let id = self.item_at(p)?;
                match self.cmdline.ux.pick {
                    None => self.cmdline.ux.pick = Some((id, p)),
                    Some((source, _)) => self.apply_ids(&[id], |d, ids| d.match_properties(source, ids))?,
                }
                Ok(())
            }
            Cmd::Break => {
                let Some((id, first)) = self.cmdline.ux.pick.take() else {
                    let id = self.item_at(p)?;
                    if self.cmdline.ux.break_two {
                        self.cmdline.ux.pick = Some((id, p));
                        self.draft = vec![p];
                        return Ok(());
                    }
                    return self.apply_ids(&[], |d, _| d.break_item(id, p, None).map(drop));
                };
                self.draft.clear();
                self.apply_ids(&[], |d, _| d.break_item(id, first, Some(p)).map(drop))
            }
            Cmd::Extend => {
                let id = self.item_at(p)?;
                self.apply_ids(&[], |d, _| d.extend(id, p, &[]))
            }
            Cmd::Lengthen => {
                let id = self.item_at(p)?;
                let u = &self.cmdline.ux;
                let (delta, total) = if u.lengthen_total { (None, Some(u.total)) } else { (Some(u.delta), None) };
                self.apply_ids(&[], |d, _| d.lengthen(id, p, delta, total))
            }
            _ => Ok(()),
        }
    }

    /// STRETCH: two window corners, then the base point and the second point.
    fn stretch_point(&mut self, p: Point) -> Result<(), String> {
        let Some(w) = self.cmdline.ux.window else {
            self.draft.push(p);
            if let [a, b, ..] = self.draft[..] {
                self.cmdline.ux.window = Some(window(a, b));
                self.draft.clear();
                self.tool = Tool::Pick;
            }
            return Ok(());
        };
        let Some(&base) = self.cmdline.ux.pts.first() else {
            self.cmdline.ux.pts.push(p);
            self.draft = vec![p];
            return Ok(());
        };
        let (dx, dy) = (p[0] - base[0], p[1] - base[1]);
        // With shapes selected only those stretch; otherwise everything the window catches.
        self.apply(|d, ids| d.stretch(ids, w, dx, dy).map(drop))?;
        self.end();
        Ok(())
    }

    fn align_now(&mut self) -> Result<(), String> {
        let (pts, scale) = (self.cmdline.ux.pts.clone(), self.cmdline.ux.align_scale);
        let (source, dest) = match pts.as_slice() {
            [s1, d1] => ((*s1, None), (*d1, None)),
            [s1, d1, s2, d2] => ((*s1, Some(*s2)), (*d1, Some(*d2))),
            _ => return Ok(()),
        };
        self.apply(|d, ids| d.align(ids, source, dest, scale))?;
        self.end();
        Ok(())
    }

    pub(super) fn reshape_prompt(&self, cmd: Cmd) -> String {
        let u = &self.cmdline.ux;
        match cmd {
            Cmd::Stretch => match (u.window.is_some(), self.draft.len(), u.pts.len()) {
                (false, 0, _) => "Select objects to stretch by crossing-window: first corner:",
                (false, _, _) => "Specify opposite corner:",
                (true, _, 0) => "Specify base point:",
                (true, _, _) => "Specify second point:",
            },
            Cmd::Align => match u.pts.len() {
                0 => "Specify first source point:",
                1 => "Specify first destination point:",
                2 => "Specify second source point or <continue>:",
                _ => "Specify second destination point:",
            },
            Cmd::MatchProp if u.pick.is_none() => "Select source object:",
            Cmd::MatchProp => "Select destination object(s) (Enter when done):",
            Cmd::Break if u.pick.is_none() => "Select object (where you click is the first break point):",
            Cmd::Break => "Specify second break point or [At this point]:",
            Cmd::Extend => "Select object to extend (click near its end) or <exit>:",
            Cmd::Lengthen if u.lengthen_total => "Select an end to change or [DElta/Total] (Total):",
            Cmd::Lengthen => "Select an end to change or [DElta/Total] (DElta):",
            _ => "",
        }
        .into()
    }

    pub(super) fn reshape_fields(&mut self, ui: &mut egui::Ui, cmd: Cmd) {
        let u = &mut self.cmdline.ux;
        match cmd {
            Cmd::Align => {
                ui.checkbox(&mut u.align_scale, "Scale to fit the destination points");
                if u.pts.len() == 2 && ui.button("Only move (one pair of points)").clicked() {
                    self.answer(Entry::Empty, None);
                }
            }
            Cmd::Break => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut u.break_two, true, "Remove a piece (2 points)");
                    ui.selectable_value(&mut u.break_two, false, "Split at a point");
                });
                if u.pick.is_some() && ui.button("Split at the first point").clicked() {
                    self.answer(Entry::Word("AT".into()), None);
                }
            }
            Cmd::Lengthen => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut u.lengthen_total, false, "Change by");
                    ui.selectable_value(&mut u.lengthen_total, true, "Make it");
                });
                ui.horizontal(|ui| {
                    if u.lengthen_total {
                        ui.label("Total length");
                        ui.add(DragValue::new(&mut u.total).speed(0.1).range(0.01..=100_000.0).suffix(" mm"));
                    } else {
                        ui.label("Change");
                        ui.add(DragValue::new(&mut u.delta).speed(0.1).suffix(" mm"));
                    }
                });
                ui.label(RichText::new("Click near the end of a line or arc; a negative change shortens it.").small().weak());
            }
            Cmd::MatchProp if u.pick.is_some() => {
                let has_selection = !self.doc.lock().unwrap().selection.is_empty();
                if has_selection && ui.button("Apply to the selection").clicked() {
                    self.answer(Entry::Empty, None);
                }
                ui.label(RichText::new("Copies the laser operation (and the font, for text).").small().weak());
            }
            Cmd::Stretch => _ = ui.label(RichText::new("Drag a box over the corners to move; everything outside it stays.").small().weak()),
            Cmd::Extend => _ = ui.label(RichText::new("Each line grows to the nearest shape or guide line ahead of it.").small().weak()),
            _ => {}
        }
    }
}
