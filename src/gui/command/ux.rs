//! The commands of the AutoCAD UX pack: where they start, which handler answers each input, their
//! prompts and command-card fields. Guide lines are in `guide.rs`, the editing commands in
//! `reshape.rs`; ZOOM, ISOLATE and the selection commands are here.

use super::parse::{Entry, is_option};
use super::{App, Cmd, Step};
use crate::gui::tools::Tool;
use crate::model::editing::KINDS;
use crate::model::{BBox, Op, Point};
use eframe::egui::{self, DragValue, RichText};

/// XLINE's ways of placing lines (AutoCAD's options).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GuideMode {
    /// Through a first point and each further point.
    #[default]
    Points,
    Hor,
    Ver,
    Ang,
    /// Bisect the angle at a vertex between two points.
    Bisect,
    /// Parallel to a picked line at a distance.
    Offset,
}

impl GuideMode {
    pub const ALL: [GuideMode; 6] = [GuideMode::Points, GuideMode::Hor, GuideMode::Ver, GuideMode::Ang, GuideMode::Bisect, GuideMode::Offset];

    pub fn label(self) -> &'static str {
        match self {
            GuideMode::Points => "Through 2 points",
            GuideMode::Hor => "Horizontal",
            GuideMode::Ver => "Vertical",
            GuideMode::Ang => "At an angle",
            GuideMode::Bisect => "Halve an angle",
            GuideMode::Offset => "Parallel",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            GuideMode::Points => "Points",
            GuideMode::Hor => "Hor",
            GuideMode::Ver => "Ver",
            GuideMode::Ang => "Ang",
            GuideMode::Bisect => "Bisect",
            GuideMode::Offset => "Offset",
        }
    }
}

/// What the UX-pack commands remember between steps (and the settings kept for the next run).
pub struct UxState {
    /// Points given so far (STRETCH base, ALIGN pairs, XLINE first point, BISECT vertex…).
    pub(super) pts: Vec<Point>,
    /// The object picked first (CENTERLINE's first line, MATCHPROP's source, BREAK's object, XLINE Offset's line).
    pub(super) pick: Option<(u64, Point)>,
    /// STRETCH's crossing window, once given.
    pub(super) window: Option<BBox>,
    pub(super) guide: GuideMode,
    /// XLINE Ang (counter-clockwise, as typed) and Offset distance.
    pub(super) angle: f64,
    pub(super) offset: f64,
    pub(super) lengthen_total: bool,
    pub(super) delta: f64,
    pub(super) total: f64,
    pub(super) align_scale: bool,
    pub(super) break_two: bool,
    pub(super) q_op: Option<Op>,
    pub(super) q_kind: Option<&'static str>,
    /// Quick select size range, mm; 0 = no limit.
    pub(super) q_min: f64,
    pub(super) q_max: f64,
}

impl Default for UxState {
    fn default() -> Self {
        Self {
            pts: Vec::new(),
            pick: None,
            window: None,
            guide: GuideMode::Points,
            angle: 45.0,
            offset: 10.0,
            lengthen_total: false,
            delta: 5.0,
            total: 50.0,
            align_scale: false,
            break_two: true,
            q_op: None,
            q_kind: None,
            q_min: 0.0,
            q_max: 0.0,
        }
    }
}

impl Cmd {
    pub(super) fn ux(self) -> bool {
        matches!(
            self,
            Cmd::Xline
                | Cmd::Ray
                | Cmd::Centerline
                | Cmd::Centermark
                | Cmd::Zoom
                | Cmd::Stretch
                | Cmd::Join
                | Cmd::Explode
                | Cmd::Align
                | Cmd::MatchProp
                | Cmd::Break
                | Cmd::Extend
                | Cmd::Lengthen
                | Cmd::Group
                | Cmd::Ungroup
                | Cmd::Isolate
                | Cmd::Unisolate
                | Cmd::SelectSimilar
                | Cmd::QuickSelect
        )
    }

    /// Commands that act on the selection (asking for one when nothing is selected).
    fn on_selection(self) -> bool {
        matches!(self, Cmd::Join | Cmd::Explode | Cmd::Group | Cmd::Ungroup | Cmd::Isolate | Cmd::SelectSimilar | Cmd::Align)
    }
}

pub(super) fn title(cmd: Cmd) -> &'static str {
    match cmd {
        Cmd::Xline => "Guide line",
        Cmd::Ray => "Guide ray",
        Cmd::Centerline => "Centre line",
        Cmd::Centermark => "Centre mark",
        Cmd::Zoom => "Zoom",
        Cmd::Stretch => "Stretch",
        Cmd::Join => "Join",
        Cmd::Explode => "Explode",
        Cmd::Align => "Align",
        Cmd::MatchProp => "Copy laser settings",
        Cmd::Break => "Break",
        Cmd::Extend => "Extend",
        Cmd::Lengthen => "Lengthen",
        Cmd::Group => "Group",
        Cmd::Ungroup => "Ungroup",
        Cmd::Isolate => "Show only the selection",
        Cmd::Unisolate => "Show everything",
        Cmd::SelectSimilar => "Select similar",
        Cmd::QuickSelect => "Quick select",
        _ => "",
    }
}

/// Window from two corners, in any order.
pub(super) fn window(a: Point, b: Point) -> BBox {
    [a[0].min(b[0]), a[1].min(b[1]), (a[0] - b[0]).abs(), (a[1] - b[1]).abs()]
}

impl App {
    pub(super) fn start_ux(&mut self, cmd: Cmd) {
        let u = &mut self.cmdline.ux;
        u.pts.clear();
        u.pick = None;
        u.window = None;
        match cmd {
            Cmd::Unisolate => {
                self.isolated = None;
                self.end();
            }
            Cmd::Zoom | Cmd::Stretch => {
                self.begin(cmd, Step::Ux);
                self.tool = Tool::Rect;
            }
            Cmd::QuickSelect => {
                self.begin(cmd, Step::Ux);
                self.tool = Tool::Select;
            }
            c if c.on_selection() => {
                self.begin(cmd, Step::UxSelect);
                self.tool = Tool::Select;
                if !self.doc.lock().unwrap().selection.is_empty() {
                    self.ux_chosen(cmd);
                }
            }
            _ => {
                self.begin(cmd, Step::Ux);
                self.tool = Tool::Pick;
            }
        }
    }

    pub(super) fn ux_step(&mut self, cmd: Cmd, step: Step, entry: Entry, pointer: Option<Point>) -> Result<(), String> {
        if step == Step::UxSelect {
            if entry != Entry::Empty {
                return Err("Click the objects (or drag a box around them), then press Enter.".into());
            }
            if self.doc.lock().unwrap().selection.is_empty() {
                self.end();
                return Err("Nothing selected.".into());
            }
            self.ux_chosen(cmd);
            return Ok(());
        }
        match cmd {
            Cmd::Xline | Cmd::Ray | Cmd::Centerline | Cmd::Centermark => self.guide_step(cmd, entry, pointer),
            Cmd::Zoom => self.zoom_step(entry),
            Cmd::QuickSelect => {
                if entry == Entry::Empty {
                    self.quick_select_now();
                }
                Ok(())
            }
            _ => self.reshape_step(cmd, entry, pointer),
        }
    }

    /// The selection is made: run the command on it.
    fn ux_chosen(&mut self, cmd: Cmd) {
        let ids = self.doc.lock().unwrap().selection.clone();
        let result = match cmd {
            Cmd::Isolate => {
                self.isolated = Some(ids);
                Ok(())
            }
            Cmd::SelectSimilar => {
                let mut d = self.doc.lock().unwrap();
                d.selection = d.similar(&ids);
                d.rev += 1;
                Ok(())
            }
            Cmd::Join => self.apply(|d, ids| d.join(ids).map(drop)),
            Cmd::Explode => self.apply(|d, ids| d.explode(ids).map(drop)),
            Cmd::Group => self.apply(|d, ids| d.group(ids).map(drop)),
            Cmd::Ungroup => self.apply(|d, ids| d.ungroup(ids)),
            Cmd::Align => {
                self.set_step(Step::Ux);
                self.tool = Tool::Pick;
                return;
            }
            _ => Ok(()),
        };
        self.end();
        if let Err(e) = result {
            self.cmdline.log(e);
        }
    }

    fn zoom_step(&mut self, entry: Entry) -> Result<(), String> {
        match entry {
            Entry::Empty => self.end(),
            Entry::Word(w) if is_option(&w, "All") || is_option(&w, "Extents") => {
                self.view.zoom_extents();
                self.end();
            }
            Entry::Word(w) if is_option(&w, "Previous") => {
                if !self.zoom_previous() {
                    return Err("There is no previous view.".into());
                }
                self.end();
            }
            Entry::Word(w) if is_option(&w, "Selection") || is_option(&w, "Object") => {
                if !self.zoom_selection() {
                    return Err("Nothing is selected.".into());
                }
                self.end();
            }
            Entry::Word(w) if is_option(&w, "Window") => {}
            Entry::Point(p) => {
                self.draft.push(p);
                if let [a, b, ..] = self.draft[..] {
                    self.zoom_to(window(a, b));
                    self.end();
                }
            }
            _ => return Err("Click two corners of the area to show, or answer All, Previous or Selection.".into()),
        }
        Ok(())
    }

    fn quick_select_now(&mut self) {
        let u = &self.cmdline.ux;
        let limit = |v: f64| (v > 0.0).then_some(v);
        let mut d = self.doc.lock().unwrap();
        d.selection = d.quick_select(u.q_op, u.q_kind, limit(u.q_min), limit(u.q_max));
        d.rev += 1;
        let n = d.selection.len();
        drop(d);
        self.cmdline.log(format!("{n} selected."));
        self.end();
    }

    pub(super) fn ux_prompt(&self, cmd: Cmd, step: Step) -> String {
        if step == Step::UxSelect {
            return "Select objects (Enter when done):".into();
        }
        let n = self.draft.len();
        match cmd {
            Cmd::Zoom if n == 0 => "Specify corner of window or [All/Previous/Selection]:".into(),
            Cmd::Zoom => "Specify opposite corner:".into(),
            Cmd::QuickSelect => "Choose what to select, then Select (Enter):".into(),
            Cmd::Xline | Cmd::Ray | Cmd::Centerline | Cmd::Centermark => self.guide_prompt(cmd),
            _ => self.reshape_prompt(cmd),
        }
    }

    pub(super) fn ux_fields(&mut self, ui: &mut egui::Ui, cmd: Cmd, step: Step) {
        if step == Step::UxSelect {
            return;
        }
        match cmd {
            Cmd::Xline => self.xline_fields(ui),
            Cmd::Zoom => {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Whole material").clicked() {
                        self.answer(Entry::Word("ALL".into()), None);
                    }
                    if ui.button("Previous view").clicked() {
                        self.answer(Entry::Word("PREVIOUS".into()), None);
                    }
                    if ui.button("Selection").clicked() {
                        self.answer(Entry::Word("SELECTION".into()), None);
                    }
                });
                ui.label(RichText::new("Or drag a box around the part to show.").small().weak());
            }
            Cmd::QuickSelect => self.quick_select_fields(ui),
            _ => self.reshape_fields(ui, cmd),
        }
    }

    fn quick_select_fields(&mut self, ui: &mut egui::Ui) {
        let u = &mut self.cmdline.ux;
        egui::Grid::new("qselect").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Laser operation");
            egui::ComboBox::from_id_salt("q-op").selected_text(u.q_op.map_or("Any", crate::gui::labels::op_label)).show_ui(ui, |ui| {
                ui.selectable_value(&mut u.q_op, None, "Any");
                for o in Op::ALL {
                    ui.selectable_value(&mut u.q_op, Some(o), crate::gui::labels::op_label(o));
                }
            });
            ui.end_row();
            ui.label("Kind of shape");
            egui::ComboBox::from_id_salt("q-kind").selected_text(u.q_kind.unwrap_or("Any")).show_ui(ui, |ui| {
                ui.selectable_value(&mut u.q_kind, None, "Any");
                for k in KINDS {
                    ui.selectable_value(&mut u.q_kind, Some(k), k);
                }
            });
            ui.end_row();
            ui.label("Size from");
            ui.add(DragValue::new(&mut u.q_min).speed(0.5).range(0.0..=100_000.0).suffix(" mm"));
            ui.end_row();
            ui.label("Size up to");
            ui.add(DragValue::new(&mut u.q_max).speed(0.5).range(0.0..=100_000.0).suffix(" mm"));
            ui.end_row();
        });
        ui.label(RichText::new("Size is the longer side of a shape's box; 0 means no limit.").small().weak());
        if ui.button("Select").clicked() {
            self.quick_select_now();
        }
    }
}
