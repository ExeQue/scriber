//! The AutoCAD-style command line: commands and their aliases, the step each command is at,
//! and the prompt for it. Every input ends up here: typed lines, canvas clicks (through
//! `place_point`), Enter/Space, the right-click menu and the toolbar buttons.

mod card;
mod constrain;
mod curves;
mod dynamic;
mod context_menu;
mod draw;
mod edit;
mod guide;
mod infer;
mod modify;
mod parse;
mod polygons;
mod rects;
mod preview;
mod reshape;
mod ux;

use super::App;
use super::tools::Tool;
use crate::model::{Kind, Point};
use eframe::egui::{self, RichText, TextEdit, TextStyle};
use parse::Entry;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Line,
    Circle,
    Arc,
    Rectang,
    Polygon,
    Pline,
    Text,
    DimLinear,
    Dist,
    Erase,
    Move,
    Copy,
    Rotate,
    Scale,
    Mirror,
    Undo,
    Redo,
    Fillet,
    Chamfer,
    Offset,
    Trim,
    Array,
    Union,
    Subtract,
    Intersect,
    /// GC* and DC* (DCLINEAR is `DcLinear`): add one constraint.
    Constrain(Kind),
    DcLinear,
    AutoConstrain,
    DelConstraint,
    ConstraintBar,
    Parameters,
    // The AutoCAD UX pack (see command/ux.rs).
    Xline,
    Ray,
    Centerline,
    Centermark,
    Zoom,
    Stretch,
    Join,
    Explode,
    Align,
    MatchProp,
    Break,
    Extend,
    Lengthen,
    Group,
    Ungroup,
    Isolate,
    Unisolate,
    SelectSimilar,
    QuickSelect,
}

/// Name, alias and the canvas tool used while the command asks for points.
const COMMANDS: [(Cmd, &str, &str, Tool); 67] = [
    (Cmd::Line, "LINE", "L", Tool::Line),
    (Cmd::Circle, "CIRCLE", "C", Tool::Circle),
    (Cmd::Arc, "ARC", "A", Tool::Arc),
    (Cmd::Rectang, "RECTANG", "REC", Tool::Rect),
    (Cmd::Polygon, "POLYGON", "POL", Tool::Polygon),
    (Cmd::Pline, "PLINE", "PL", Tool::Polyline),
    (Cmd::Text, "TEXT", "T", Tool::Text),
    (Cmd::DimLinear, "DIMLINEAR", "DLI", Tool::Dimension),
    (Cmd::Dist, "DIST", "DI", Tool::Measure),
    (Cmd::Erase, "ERASE", "E", Tool::Select),
    (Cmd::Move, "MOVE", "M", Tool::Select),
    (Cmd::Copy, "COPY", "CO", Tool::Select),
    (Cmd::Rotate, "ROTATE", "RO", Tool::Select),
    (Cmd::Scale, "SCALE", "SC", Tool::Select),
    (Cmd::Mirror, "MIRROR", "MI", Tool::Select),
    (Cmd::Undo, "UNDO", "U", Tool::Select),
    (Cmd::Redo, "REDO", "REDO", Tool::Select),
    (Cmd::Fillet, "FILLET", "F", Tool::Pick),
    (Cmd::Chamfer, "CHAMFER", "CHA", Tool::Pick),
    (Cmd::Offset, "OFFSET", "O", Tool::Pick),
    (Cmd::Trim, "TRIM", "TR", Tool::Pick),
    (Cmd::Array, "ARRAY", "AR", Tool::Select),
    (Cmd::Union, "UNION", "UNI", Tool::Select),
    (Cmd::Subtract, "SUBTRACT", "SU", Tool::Select),
    (Cmd::Intersect, "INTERSECT", "IN", Tool::Select),
    (Cmd::Constrain(Kind::Coincident), "GCCOINCIDENT", "GCCOINCIDENT", Tool::Pick),
    (Cmd::Constrain(Kind::Collinear), "GCCOLLINEAR", "GCCOLLINEAR", Tool::Pick),
    (Cmd::Constrain(Kind::Concentric), "GCCONCENTRIC", "GCCONCENTRIC", Tool::Pick),
    (Cmd::Constrain(Kind::Fix), "GCFIX", "GCFIX", Tool::Pick),
    (Cmd::Constrain(Kind::Parallel), "GCPARALLEL", "GCPARALLEL", Tool::Pick),
    (Cmd::Constrain(Kind::Perpendicular), "GCPERPENDICULAR", "GCPERPENDICULAR", Tool::Pick),
    (Cmd::Constrain(Kind::Horizontal), "GCHORIZONTAL", "GCHORIZONTAL", Tool::Pick),
    (Cmd::Constrain(Kind::Vertical), "GCVERTICAL", "GCVERTICAL", Tool::Pick),
    (Cmd::Constrain(Kind::Tangent), "GCTANGENT", "GCTANGENT", Tool::Pick),
    (Cmd::Constrain(Kind::Smooth), "GCSMOOTH", "GCSMOOTH", Tool::Pick),
    (Cmd::Constrain(Kind::Symmetric), "GCSYMMETRIC", "GCSYMMETRIC", Tool::Pick),
    (Cmd::Constrain(Kind::Equal), "GCEQUAL", "GCEQUAL", Tool::Pick),
    (Cmd::DcLinear, "DCLINEAR", "DCLINEAR", Tool::Pick),
    (Cmd::Constrain(Kind::DimHorizontal), "DCHORIZONTAL", "DCHORIZONTAL", Tool::Pick),
    (Cmd::Constrain(Kind::DimVertical), "DCVERTICAL", "DCVERTICAL", Tool::Pick),
    (Cmd::Constrain(Kind::DimAligned), "DCALIGNED", "DCALIGNED", Tool::Pick),
    (Cmd::Constrain(Kind::DimAngular), "DCANGULAR", "DCANGULAR", Tool::Pick),
    (Cmd::Constrain(Kind::DimRadial), "DCRADIUS", "DCRADIUS", Tool::Pick),
    (Cmd::Constrain(Kind::DimDiameter), "DCDIAMETER", "DCDIAMETER", Tool::Pick),
    (Cmd::AutoConstrain, "AUTOCONSTRAIN", "AUTOCONSTRAIN", Tool::Select),
    (Cmd::DelConstraint, "DELCONSTRAINT", "DELCON", Tool::Select),
    (Cmd::ConstraintBar, "CONSTRAINTBAR", "CBAR", Tool::Select),
    (Cmd::Parameters, "PARAMETERS", "PAR", Tool::Select),
    (Cmd::Xline, "XLINE", "XL", Tool::Pick),
    (Cmd::Ray, "RAY", "RAY", Tool::Pick),
    (Cmd::Centerline, "CENTERLINE", "CENTERLINE", Tool::Pick),
    (Cmd::Centermark, "CENTERMARK", "CENTERMARK", Tool::Pick),
    (Cmd::Zoom, "ZOOM", "Z", Tool::Rect),
    (Cmd::Stretch, "STRETCH", "S", Tool::Rect),
    (Cmd::Join, "JOIN", "J", Tool::Select),
    (Cmd::Explode, "EXPLODE", "X", Tool::Select),
    (Cmd::Align, "ALIGN", "AL", Tool::Pick),
    (Cmd::MatchProp, "MATCHPROP", "MA", Tool::Pick),
    (Cmd::Break, "BREAK", "BR", Tool::Pick),
    (Cmd::Extend, "EXTEND", "EX", Tool::Pick),
    (Cmd::Lengthen, "LENGTHEN", "LEN", Tool::Pick),
    (Cmd::Group, "GROUP", "G", Tool::Select),
    (Cmd::Ungroup, "UNGROUP", "UNG", Tool::Select),
    (Cmd::Isolate, "ISOLATEOBJECTS", "ISOLATE", Tool::Select),
    (Cmd::Unisolate, "UNISOLATEOBJECTS", "UNISOLATE", Tool::Select),
    (Cmd::SelectSimilar, "SELECTSIMILAR", "SELECTSIMILAR", Tool::Select),
    (Cmd::QuickSelect, "QSELECT", "QSELECT", Tool::Select),
];

impl Cmd {
    /// A command by its name or alias, in any case.
    pub fn find(word: &str) -> Option<Cmd> {
        let word = word.to_ascii_uppercase();
        COMMANDS.iter().find(|c| c.1 == word || c.2 == word).map(|c| c.0)
    }

    /// The command a toolbar button runs.
    pub fn for_tool(t: Tool) -> Option<Cmd> {
        COMMANDS.iter().find(|c| c.3 == t && !matches!(t, Tool::Select | Tool::Pick)).map(|c| c.0)
    }

    fn row(self) -> &'static (Cmd, &'static str, &'static str, Tool) {
        COMMANDS.iter().find(|c| c.0 == self).expect("every command is in the table")
    }

    pub fn name(self) -> &'static str {
        self.row().1
    }

    /// Plain-language name for buttons and menus.
    pub fn title(self) -> &'static str {
        card::title(self)
    }

    /// For constraint commands: a constraint command (its tooltip names the AutoCAD command).
    pub fn is_constraint(self) -> bool {
        self.constrains()
    }

    pub fn alias(self) -> &'static str {
        self.row().2
    }

    fn tool(self) -> Tool {
        self.row().3
    }

    fn edits(self) -> bool {
        matches!(self, Cmd::Erase | Cmd::Move | Cmd::Copy | Cmd::Rotate | Cmd::Scale | Cmd::Mirror)
    }
}

/// Where a command is: which input its prompt asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// Points for a drawing command (into `App::draft`).
    Draw,
    Sides,
    Diameter,
    Fillet,
    Height,
    /// Free text: Space is a character here, not Enter.
    Content,
    Select,
    Base,
    /// Second point, rotation angle or scale factor.
    Second,
    EraseSource,
    /// A distance, or the next number/option of ARRAY.
    Value,
    /// Clicking on an item (a corner, the part to trim, the object to offset).
    Pick,
    /// OFFSET: the side to offset to.
    Side,
    /// Selecting the objects for ARRAY or a boolean.
    Choose,
    /// Constraints: clicking the geometry, then where a dimension goes, then its value (free text).
    Constrain,
    Location,
    Expr,
    /// UX-pack commands: choosing the objects, then their own points and picks (see command/ux.rs).
    UxSelect,
    Ux,
}

pub struct CommandLine {
    pub input: String,
    history: Vec<String>,
    active: Option<(Cmd, Step)>,
    /// Repeated by Enter on an empty line.
    last: Option<Cmd>,
    last_point: Option<Point>,
    /// Corner radius for RECTANG.
    fillet: f64,
    text_height: f64,
    /// Font family for new text; the last one picked.
    text_font: String,
    /// The running command has made its undo step (one per command).
    checkpointed: bool,
    modify: modify::ModifyState,
    /// How ARC and CIRCLE are drawn (the last way used).
    arc_mode: curves::ArcMode,
    rect_mode: rects::RectMode,
    /// POLYGON measures to the middle of a side instead of a corner.
    polygon_flat: bool,
    /// Measurements on the command card (radius, length and angle, …).
    dynamic: dynamic::DynInput,
    circle_mode: curves::CircleMode,
    constrain: constrain::ConstrainState,
    ux: ux::UxState,
    /// ROTATE angle and SCALE factor as set on the command card.
    card_angle: f64,
    card_factor: f64,
}

impl CommandLine {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            history: Vec::new(),
            active: None,
            last: None,
            last_point: None,
            fillet: 0.0,
            text_height: 10.0,
            text_font: crate::model::text::DEFAULT_FONT.to_owned(),
            checkpointed: false,
            modify: modify::ModifyState::new(),
            arc_mode: curves::ArcMode::default(),
            rect_mode: rects::RectMode::default(),
            polygon_flat: false,
            dynamic: dynamic::DynInput::default(),
            circle_mode: curves::CircleMode::default(),
            constrain: constrain::ConstrainState::default(),
            ux: ux::UxState::default(),
            card_angle: 90.0,
            card_factor: 2.0,
        }
    }

    pub(in crate::gui) fn log(&mut self, line: impl Into<String>) {
        self.history.push(line.into());
        if self.history.len() > 200 {
            self.history.remove(0);
        }
    }
}

/// The id of the command line's text field, which has the keyboard unless another text field does.
pub fn input_id() -> egui::Id {
    egui::Id::unique("command-line-input")
}

impl App {
    /// A toolbar button: runs the same command as typing its name. Select cancels.
    pub(super) fn set_tool(&mut self, t: Tool) {
        match Cmd::for_tool(t) {
            Some(cmd) => self.start(cmd),
            None => self.cancel(),
        }
    }

    pub(super) fn start(&mut self, cmd: Cmd) {
        self.cancel();
        self.dyn_reset();
        self.measured = None;
        self.cmdline.last = Some(cmd);
        self.cmdline.checkpointed = false;
        self.cmdline.log(format!("Command: {}", cmd.name()));
        match cmd {
            Cmd::Undo | Cmd::Redo => {
                let mut d = self.doc.lock().unwrap();
                let done = if cmd == Cmd::Undo { d.undo() } else { d.redo() };
                drop(d);
                if !done {
                    self.cmdline.log(format!("Nothing to {}.", cmd.name().to_lowercase()));
                }
            }
            c if c.edits() => self.start_edit(c),
            c if c.modifies() => self.start_modify(c),
            c if c.constrains() => self.start_constrain(c),
            c if c.ux() => self.start_ux(c),
            _ => self.begin(cmd, Step::Draw),
        }
    }

    fn begin(&mut self, cmd: Cmd, step: Step) {
        self.cmdline.active = Some((cmd, step));
        self.tool = cmd.tool();
        self.draft.clear();
        self.draft_links.clear();
    }

    fn set_step(&mut self, step: Step) {
        if let Some((_, s)) = &mut self.cmdline.active {
            *s = step;
        }
    }

    /// The command is done: back to selecting.
    fn end(&mut self) {
        self.clear_picked();
        self.cmdline.active = None;
        self.tool = Tool::Select;
        self.draft.clear();
        self.draft_links.clear();
        self.tracker.clear();
        self.once = None;
    }

    pub(super) fn cancel(&mut self) {
        if self.cmdline.active.is_some() {
            self.cmdline.log("*Cancel*");
        }
        self.end();
    }

    /// Esc: cancel the command, or clear the selection when none runs.
    pub(super) fn escape(&mut self) {
        self.cmdline.input.clear();
        if self.cmdline.active.is_some() {
            self.cancel();
        } else {
            self.doc.lock().unwrap().selection.clear();
            self.measured = None;
        }
    }

    /// The command that is running, for highlighting its toolbar button.
    pub(super) fn active_cmd(&self) -> Option<Cmd> {
        self.cmdline.active.map(|(c, _)| c)
    }

    /// The prompt asks for free text, so Space types a space instead of acting as Enter.
    pub(super) fn takes_text(&self) -> bool {
        matches!(self.cmdline.active, Some((_, Step::Content | Step::Expr)))
    }

    fn last_point(&self) -> Option<Point> {
        self.draft.last().copied().or(self.cmdline.last_point)
    }

    /// A point from a typed or clicked entry; a bare number is a distance towards the pointer.
    fn point_of(&self, entry: &Entry, pointer: Option<Point>) -> Option<Point> {
        match entry {
            Entry::Point(p) => Some(*p),
            Entry::Number(d) => self.last_point().map(|from| parse::toward(from, pointer, *d)),
            _ => None,
        }
    }

    /// Enter (or Space): answer the prompt with what is typed.
    pub(super) fn submit(&mut self, pointer: Option<Point>) {
        let text = std::mem::take(&mut self.cmdline.input);
        if self.cmdline.active.is_none() {
            return self.idle(&text);
        }
        self.cmdline.log(format!("{} {text}", self.prompt()).trim_end());
        if self.takes_text() {
            return match self.cmdline.active {
                Some((_, Step::Expr)) => self.enter_value(&text),
                _ => self.enter_text(text),
            };
        }
        match parse::parse(&text, self.last_point()) {
            Ok(entry) => self.answer(entry, pointer),
            Err(e) => self.cmdline.log(e),
        }
    }

    /// Feed one point to the running command. Every way of giving a point ends here: a click on
    /// the canvas, a snap, or coordinates typed on the command line.
    pub(super) fn place_point(&mut self, p: Point) {
        // Locked measurements from dynamic input win over the mouse, also for clicks.
        let p = self.dyn_point(p).unwrap_or(p);
        // The first pick of "Mid between 2 points" or "From" is the override's, not the command's.
        if self.override_pick(p) {
            return;
        }

        self.dyn_reset();
        self.cmdline.log(format!("{} {:.2},{:.2}", self.prompt(), p[0], p[1]));
        self.answer(Entry::Point(p), Some(p));
    }

    /// A line typed with no command running: a command name, or Enter to repeat the last one.
    fn idle(&mut self, text: &str) {
        match (text.trim(), self.cmdline.last) {
            ("", Some(cmd)) => self.start(cmd),
            ("", None) => {}
            (word, _) => match Cmd::find(word) {
                Some(cmd) => self.start(cmd),
                None => self.cmdline.log(format!("Unknown command \"{word}\". Try LINE, CIRCLE, RECTANG, MOVE, FILLET, OFFSET, …")),
            },
        }
    }

    fn answer(&mut self, entry: Entry, pointer: Option<Point>) {
        let Some((cmd, step)) = self.cmdline.active else { return };
        let result = match step {
            Step::Select | Step::Base | Step::Second | Step::EraseSource => self.edit_step(cmd, step, entry, pointer),
            Step::Value | Step::Pick | Step::Side | Step::Choose => self.modify_step(cmd, step, entry, pointer),
            Step::Constrain | Step::Location | Step::Expr => self.constrain_step(cmd, step, entry),
            Step::Ux | Step::UxSelect => self.ux_step(cmd, step, entry, pointer),
            _ => self.draw_step(cmd, step, entry, pointer),
        };
        if let Err(e) = result {
            self.cmdline.log(e);
        }
    }

    pub(super) fn prompt(&self) -> String {
        let Some((cmd, step)) = self.cmdline.active else { return "Command:".into() };
        if cmd.modifies() {
            return self.modify_prompt(cmd, step);
        }
        if cmd.constrains() {
            return self.constrain_prompt(cmd, step);
        }
        if cmd.ux() {
            return self.ux_prompt(cmd, step);
        }
        let n = self.draft.len();
        let text = match (step, cmd) {
            (Step::Sides, _) => return format!("Enter number of sides <{}>:", self.sides),
            (Step::Fillet, _) => return format!("Specify fillet radius for rectangles <{}>:", self.cmdline.fillet),
            (Step::Height, _) => return format!("Specify height <{}>:", self.cmdline.text_height),
            (Step::Diameter, _) => "Specify diameter of circle:",
            (Step::Content, _) => "Enter text:",
            (Step::Select, _) => "Select objects (Enter when done):",
            (Step::Base, Cmd::Mirror) => "Specify first point of mirror line:",
            (Step::Base, _) => "Specify base point:",
            (Step::Second, Cmd::Copy) => "Specify second point or <exit>:",
            (Step::Second, Cmd::Rotate) => "Specify rotation angle (counter-clockwise):",
            (Step::Second, Cmd::Scale) => "Specify scale factor:",
            (Step::Second, Cmd::Mirror) => "Specify second point of mirror line:",
            (Step::Second, _) => "Specify second point:",
            (Step::EraseSource, _) => "Erase source objects? [Yes/No] <No>:",
            (Step::Draw, _) => self.curve_prompt(cmd, n).unwrap_or_else(|| draw::prompt(cmd, n)),
            (Step::Value | Step::Pick | Step::Side | Step::Choose | Step::Constrain | Step::Location | Step::Expr | Step::Ux | Step::UxSelect, _) => "",
        };
        text.into()
    }

    /// The command line panel: the last few lines, the prompt and the input field.
    pub(super) fn command_line(&mut self, ui: &mut egui::Ui) {
        let history = &self.cmdline.history;
        for line in &history[history.len().saturating_sub(3)..] {
            ui.label(RichText::new(line).monospace().weak());
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.prompt()).monospace().strong());
            let field = TextEdit::singleline(&mut self.cmdline.input).id(input_id()).font(TextStyle::Monospace).frame(egui::Frame::NONE);
            ui.add(field.desired_width(f32::INFINITY));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_aliases() {
        assert_eq!(Cmd::find("rec"), Some(Cmd::Rectang));
        assert_eq!(Cmd::find("Line"), Some(Cmd::Line));
        assert_eq!(Cmd::find("DLI"), Some(Cmd::DimLinear));
        assert_eq!(Cmd::find("gcparallel"), Some(Cmd::Constrain(Kind::Parallel)));
        assert_eq!(Cmd::find("PAR"), Some(Cmd::Parameters));
        assert_eq!(Cmd::find("nope"), None);
        assert_eq!(Cmd::for_tool(Tool::Polyline), Some(Cmd::Pline));
        assert_eq!(Cmd::for_tool(Tool::Select), None);
        // Every alias is unique.
        for a in COMMANDS {
            assert_eq!(COMMANDS.iter().filter(|b| b.2 == a.2 || b.1 == a.2).count(), 1, "{}", a.2);
        }
    }
}
