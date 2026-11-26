//! The desktop window. This file holds the app state, the layout and the file/edit actions;
//! the canvas, side panels and agent window live in their own files.

mod agents_window;
mod canvas;
mod check_panel;
mod command;
mod constraint_bars;
mod constraint_list;
mod cycle;
mod dimensions;
mod fonts;
mod geometry_cache;
mod glyphs;
mod grip_menu;
mod grips;
mod guides;
mod hover;
mod labels;
mod library_window;
mod overrides;
mod panels;
mod parameters;
mod picked;
mod preview;
mod select;
mod settings;
mod snap;
mod svg_import;
mod toolbar;
mod toolbar_ux;
mod tools;
mod tracking;
mod zoom;

use crate::model::{Document, Op, Point, Shape, Shared, Transform};
use crate::{agents, export, mcp};
use eframe::egui::{self, Event, Key, Modifiers, Pos2, Rect, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use select::Drag;
use tools::Tool;

struct View {
    /// Screen position of the material's origin, relative to the canvas' top-left.
    offset: Vec2,
    /// Pixels per mm.
    zoom: f32,
    /// Keep the whole material in view (until the user pans or zooms).
    auto_fit: bool,
    /// Canvas size the last fit was made for.
    fit_size: Vec2,
    /// Earlier views (offset, zoom) for ZOOM Previous, newest last.
    history: Vec<(Vec2, f32)>,
    /// The wheel zoomed last frame (one wheel gesture is one history step).
    wheeling: bool,
}

impl View {
    /// Show the whole material again, and keep it in view when the window changes size.
    fn zoom_extents(&mut self) {
        self.remember();
        self.auto_fit = true;
        self.fit_size = Vec2::ZERO;
    }
}

struct App {
    doc: Shared,
    mcp: Result<u16, String>,
    tool: Tool,
    /// What new shapes become.
    new_op: Op,
    sides: u32,
    snap: bool,
    /// GRID, ORTHO, OSNAP and POLAR (status bar).
    aids: snap::Aids,
    /// Points placed so far with the current tool.
    draft: Vec<Point>,
    /// The snaps the draft's points were placed with (INFER), by point.
    draft_links: Vec<crate::model::constraints::attach::Link>,
    /// The tracking paths the draft's points were locked to (INFER), by point.
    draft_along: Vec<crate::model::constraints::attach::Along>,
    drag: Option<Drag>,
    cmdline: command::CommandLine,
    /// The AutoCAD-style command line; hidden unless the user turns it on (View menu).
    show_cmdline: bool,
    measured: Option<(Point, Point)>,
    /// Where the mouse is on the material (snapped), for the command card's live measurements.
    pointer_mm: Option<Point>,
    /// Tab presses taken for dynamic input before egui could use them for focus.
    dyn_tabs: usize,
    /// What the running command has picked so far, numbered on the canvas.
    picked: Vec<picked::Picked>,
    view: View,
    canvas_rect: Rect,
    textures: HashMap<u64, egui::TextureHandle>,
    /// Flattened outlines and fill meshes per item, kept between frames.
    geometry: geometry_cache::GeometryCache,
    /// The properties panel is in the middle of an edit (one undo step per edit).
    panel_editing: bool,
    message: String,
    show_agents: bool,
    /// Constraint bars on (CONSTRAINTBAR), and items whose bars are hidden.
    bars: bool,
    hidden_bars: Vec<u64>,
    /// The parameters window (PARAMETERS) is open.
    show_parameters: bool,
    /// A parameter name or expression being typed in the parameters window or properties.
    param_edit: parameters::ParamEdit,
    /// Where each dimensional constraint's label was drawn last frame (for double-click editing).
    dim_labels: Vec<(u64, Rect)>,
    /// Where each constraint badge was drawn last frame (for the right-click menu).
    bar_badges: Vec<(u64, Rect)>,
    /// The constraint the open right-click menu is for.
    menu_constraint: Option<u64>,
    /// The constraint pointed at in the properties list, highlighted on the canvas.
    hot_constraint: Option<u64>,
    agents: Arc<Mutex<Option<Vec<agents::Target>>>>,
    confirm_close: bool,
    allow_close: bool,
    svg_dialog: Option<svg_import::SvgDialog>,
    /// The open "Machines & materials" window.
    library: Option<library_window::Editor>,
    /// View > Show guide lines.
    show_guides: bool,
    /// ISOLATE: only these items are shown (view only, not saved).
    isolated: Option<Vec<u64>>,
    /// Space was held for a pan drag, so its release is not Enter.
    space_panned: bool,
    /// Preview mode is on (see preview.rs).
    preview: Option<preview::Preview>,
    /// The pre-flight check and its window.
    check: check_panel::Check,
    /// Shift is held (over the canvas): flips ORTHO while a command runs.
    shift: bool,
    /// OTRACK, Extension and Parallel: what was acquired for the running command.
    tracker: tracking::Tracker,
    /// The one-shot object snap override for the next point (Shift+right-click).
    once: Option<overrides::Once>,
    /// The right-click menu is the object snap override menu (opened with Shift).
    osnap_menu_open: bool,
    /// The shapes under the cursor and the one Shift+Space cycled to.
    cycle: cycle::Cycle,
    /// The shape a click would pick, and since when the pointer is on it.
    hover: Option<(u64, std::time::Instant)>,
    /// View > Measure on hover (else hold Ctrl), and the last two snap points pointed at.
    measure_hover: bool,
    /// The settings as last saved (status bar toggles, view choices).
    saved_settings: settings::Settings,
    measure_pts: (Option<Point>, Option<Point>),
    /// The grip the pointer rests on (since when), and the grip whose menu is open.
    grip_rest: Option<(grips::Grip, std::time::Instant)>,
    grip_menu: Option<(grips::Grip, Point)>,
    /// POLAR's additional angles as typed in its menu.
    polar_extra_text: String,
}

pub fn run(file: Option<PathBuf>) -> anyhow::Result<()> {
    #[cfg(windows)]
    detach_console();
    let doc = match &file {
        Some(f) => Document::open(f)?,
        None => Document::new(300.0, 200.0),
    };
    let shared: Shared = Arc::new(Mutex::new(doc));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Scriber")
            .with_app_id("scriber")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([800.0, 500.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Scriber",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let mcp = mcp::listen(shared.clone(), Arc::new(move || ctx.request_repaint())).map_err(|e| e.to_string());
            let mut app = App::new(shared, mcp);
            app.apply_settings(settings::load());
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// A console exe started from Explorer gets its own console window; drop it for the GUI.
#[cfg(windows)]
fn detach_console() {
    use windows_sys::Win32::System::Console::{FreeConsole, GetConsoleProcessList};
    let mut pids = [0u32; 2];
    // Only us attached = the console was made for us, not a terminal the user started us from.
    if unsafe { GetConsoleProcessList(pids.as_mut_ptr(), 2) } == 1 {
        unsafe { FreeConsole() };
    }
}

impl App {
    fn new(doc: Shared, mcp: Result<u16, String>) -> Self {
        Self {
            doc,
            mcp,
            tool: Tool::Select,
            new_op: Op::Cut,
            sides: 6,
            snap: true,
            aids: snap::Aids::default(),
            draft: Vec::new(),
            draft_links: Vec::new(),
            draft_along: Vec::new(),
            drag: None,
            cmdline: command::CommandLine::new(),
            show_cmdline: false,
            measured: None,
            pointer_mm: None,
            dyn_tabs: 0,
            picked: Vec::new(),
            view: View { offset: Vec2::ZERO, zoom: 3.0, auto_fit: true, fit_size: Vec2::ZERO, history: Vec::new(), wheeling: false },
            canvas_rect: Rect::NOTHING,
            textures: HashMap::new(),
            geometry: geometry_cache::GeometryCache::default(),
            panel_editing: false,
            message: String::new(),
            show_agents: false,
            bars: true,
            hidden_bars: Vec::new(),
            show_parameters: false,
            param_edit: None,
            dim_labels: Vec::new(),
            bar_badges: Vec::new(),
            menu_constraint: None,
            hot_constraint: None,
            agents: Arc::new(Mutex::new(None)),
            confirm_close: false,
            allow_close: false,
            svg_dialog: None,
            library: None,
            show_guides: true,
            isolated: None,
            space_panned: false,
            preview: None,
            check: check_panel::Check::default(),
            shift: false,
            tracker: tracking::Tracker::default(),
            once: None,
            osnap_menu_open: false,
            cycle: cycle::Cycle::default(),
            hover: None,
            measure_hover: false,
            saved_settings: settings::Settings::default(),
            measure_pts: (None, None),
            grip_rest: None,
            grip_menu: None,
            polar_extra_text: String::new(),
        }
    }

    // ---------- files ----------

    fn new_doc(&mut self) {
        let mut d = self.doc.lock().unwrap();
        let (w, h) = (d.s.width, d.s.height);
        *d = Document::new(w, h);
        self.textures.clear();
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Scriber", &["scrbr"]).pick_file() {
            self.open_path(&path);
        }
    }

    fn open_path(&mut self, path: &Path) {
        match Document::open(path) {
            Ok(d) => {
                *self.doc.lock().unwrap() = d;
                self.textures.clear();
                self.view.zoom_extents();
                self.message = format!("{} {}", "Opened", path.display());
            }
            Err(e) => self.message = format!("{}: {e}", "Could not open the file"),
        }
    }

    /// Save (asking for a file name when needed). Returns whether it was saved.
    fn save(&mut self, save_as: bool) -> bool {
        let current = self.doc.lock().unwrap().path.clone();
        let path = match current {
            Some(p) if !save_as => p,
            _ => match rfd::FileDialog::new().add_filter("Scriber", &["scrbr"]).set_file_name("design.scrbr").save_file() {
                Some(p) => p.with_extension("scrbr"),
                None => return false,
            },
        };
        let result = self.doc.lock().unwrap().save(&path);
        let saved = result.is_ok();
        self.message = match result {
            Ok(()) => format!("{} {}", "Saved", path.display()),
            Err(e) => format!("{}: {e}", "Could not save"),
        };
        saved
    }

    fn import_image_dialog(&mut self) {
        let exts = ["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"];
        if let Some(path) = rfd::FileDialog::new().add_filter("Images", &exts).pick_file() {
            self.import_image(&path, [10.0, 10.0]);
        }
    }

    fn import_image(&mut self, path: &Path, at: Point) {
        match std::fs::read(path).map_err(anyhow::Error::from).and_then(|b| export::image::to_grey(&b)) {
            Ok(img) => {
                let name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                {
                    let mut d = self.doc.lock().unwrap();
                    d.checkpoint();
                    let id = d.place_image(img, at, None, &name);
                    d.selection = vec![id];
                }
                self.set_tool(Tool::Select);
                self.message = "Image imported in greyscale. Change its size in the panel on the right.".into();
            }
            Err(e) => self.message = format!("{}: {e}", "Could not import the image"),
        }
    }

    fn export_svg(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("SVG", &["svg"]).set_file_name("design.svg").save_file() else { return };
        let svg = export::svg(&self.doc.lock().unwrap().s, false);
        self.message = match std::fs::write(&path, svg) {
            Ok(()) => format!("{} {}", "Exported", path.display()),
            Err(e) => format!("{}: {e}", "Export failed"),
        };
    }

    fn export_png(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("design.png").save_file() else { return };
        let state = self.doc.lock().unwrap().s.clone();
        self.message = match export::png(&state, 254.0, false).and_then(|png| Ok(std::fs::write(&path, png)?)) {
            Ok(()) => format!("{} {} (254 DPI)", "Exported", path.display()),
            Err(e) => format!("{}: {e}", "Export failed"),
        };
    }

    // ---------- edits on the selection ----------

    /// Run a command on the selection as one undo step; does nothing when nothing is selected.
    fn on_selection(&mut self, f: impl FnOnce(&mut Document, Vec<u64>)) {
        let mut d = self.doc.lock().unwrap();
        let ids = d.selection.clone();
        if !ids.is_empty() {
            d.checkpoint();
            f(&mut d, ids);
        }
    }

    fn delete_selection(&mut self) {
        self.on_selection(|d, ids| d.delete(&ids));
    }

    fn duplicate_selection(&mut self) {
        self.on_selection(|d, ids| d.selection = d.duplicate(&ids, 5.0, 5.0));
    }

    fn transform_selection(&mut self, t: Transform) {
        self.on_selection(|d, ids| d.transform(&ids, &t));
    }

    fn combine_selection(&mut self) {
        let mut err = None;
        self.on_selection(|d, ids| err = d.combine(&ids).err());
        if let Some(e) = err {
            self.doc.lock().unwrap().undo();
            self.message = e;
        }
    }

    fn break_apart_selection(&mut self) {
        let mut err = None;
        self.on_selection(|d, ids| err = ids.first().and_then(|id| d.break_apart(*id).err()));
        if let Some(e) = err {
            self.doc.lock().unwrap().undo();
            self.message = e;
        }
    }

    fn select_all(&mut self) {
        let mut d = self.doc.lock().unwrap();
        d.selection = d.s.items.iter().map(|i| i.id).collect();
    }

    fn create(&mut self, name: &str, op: Op, shape: Shape) -> u64 {
        let mut d = self.doc.lock().unwrap();
        d.checkpoint();
        let id = d.add(name, op, shape);
        d.selection = vec![id];
        id
    }

    // ---------- keyboard ----------

    /// With the command line shown, the keyboard belongs to it unless another text field has it
    /// (as in AutoCAD, typing anywhere goes to the command line). Runs before the panels, so the keys handled
    /// here do not also reach the command line's text field.
    fn keys(&mut self, ctx: &egui::Context, pointer: Option<Point>) {
        let input = command::input_id();
        let focused = ctx.memory(|m| m.focused());
        if self.confirm_close || (focused.is_some_and(|f| f != input) && ctx.text_edit_focused()) {
            return;
        }
        if self.show_cmdline && focused != Some(input) {
            ctx.memory_mut(|m| m.request_focus(input));
        }
        let consume = |mods: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(mods, k));
        let (ctrl, ctrl_shift) = (Modifiers::COMMAND, Modifiers::COMMAND | Modifiers::SHIFT);
        // Most specific first: Ctrl+Shift+Z must not also count as Ctrl+Z.
        match () {
            _ if consume(ctrl_shift, Key::Z) => _ = self.doc.lock().unwrap().redo(),
            _ if consume(ctrl, Key::Z) => _ = self.doc.lock().unwrap().undo(),
            _ if consume(ctrl, Key::Y) => _ = self.doc.lock().unwrap().redo(),
            _ if consume(ctrl_shift, Key::S) => _ = self.save(true),
            _ if consume(ctrl, Key::S) => _ = self.save(false),
            _ if consume(ctrl, Key::O) => self.open_dialog(),
            _ if consume(ctrl, Key::D) => self.duplicate_selection(),
            _ if consume(ctrl, Key::A) => self.select_all(),
            _ if consume(ctrl_shift, Key::G) => self.break_apart_selection(),
            _ if consume(ctrl, Key::G) => self.combine_selection(),
            _ if consume(Modifiers::NONE, Key::Escape) => self.escape(),
            _ if self.cycle_key(ctx) => {}
            _ if consume(Modifiers::NONE, Key::Enter) || self.space_as_enter(ctx) => self.submit(pointer),
            _ => {}
        }
        if !self.cmdline.input.is_empty() {
            return;
        }
        if consume(Modifiers::NONE, Key::Delete) || consume(Modifiers::NONE, Key::Backspace) {
            self.delete_selection();
        }
        // Arrow keys nudge the selection 1 mm (10 mm with Shift).
        for (k, dx, dy) in [(Key::ArrowLeft, -1.0, 0.0), (Key::ArrowRight, 1.0, 0.0), (Key::ArrowUp, 0.0, -1.0), (Key::ArrowDown, 0.0, 1.0)] {
            let step = if consume(Modifiers::SHIFT, k) { 10.0 } else if consume(Modifiers::NONE, k) { 1.0 } else { continue };
            self.transform_selection(Transform::translate(dx * step, dy * step));
        }
    }

    /// Space acts as Enter, except where the prompt asks for text. Takes the space out of the input.
    /// It acts on release, so Space held for a pan drag (see `zoom.rs`) is not Enter.
    fn space_as_enter(&mut self, ctx: &egui::Context) -> bool {
        if self.takes_text() {
            return false;
        }
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { key: Key::Space, pressed: true, .. }) && !matches!(e, Event::Text(t) if t == " ")));
        ctx.input(|i| i.key_released(Key::Space)) && !std::mem::take(&mut self.space_panned)
    }

    fn close_dialog(&mut self, ctx: &egui::Context) {
        egui::Modal::new(egui::Id::unique("close")).show(ctx, |ui| {
            ui.heading("Save changes?");
            ui.label("The drawing has changes that are not saved.");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() && self.save(false) {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Don't save").clicked() {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_close = false;
                }
            });
        });
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.take_tabs(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (dirty, title) = {
            let d = self.doc.lock().unwrap();
            let name = d.path.as_ref().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().into_owned());
            let name = name.unwrap_or_else(|| "Untitled".into());
            (d.dirty, format!("{name}{} — Scriber", if d.dirty { " •" } else { "" }))
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        if ctx.input(|i| i.viewport().close_requested()) && dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }

        let hover = ctx.pointer_hover_pos().filter(|p| self.canvas_rect.contains(*p));
        let pointer = hover.map(|p| self.snap(self.s2w(p)));

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::top("laser").show(ui, |ui| self.laser_bar(ui));
        self.aid_keys(&ctx);
        self.dyn_keys(&ctx);
        self.preview_key(&ctx);
        // No editing in preview mode.
        let editing = self.preview.is_none();
        if editing {
            self.keys(&ctx, pointer);
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, pointer));
        if self.show_cmdline {
            egui::Panel::bottom("command").show(ui, |ui| self.command_line(ui));
        }
        egui::Panel::left("tools").exact_size(146.0).show(ui, |ui| ui.add_enabled_ui(editing, |ui| self.toolbar(ui)));
        egui::Panel::right("props").default_size(290.0).show(ui, |ui| ui.add_enabled_ui(editing, |ui| self.properties(ui)));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
        self.command_card(&ctx);
        self.dyn_cursor(&ctx);
        self.grip_menu_window(&ctx);
        self.override_window(&ctx);


        if self.show_parameters {
            self.parameters_window(&ctx);
        }
        if self.show_agents {
            self.agents_window(&ctx);
        }
        self.check_window(&ctx);
        self.svg_import_window(&ctx);
        self.library_window(&ctx);
        if self.confirm_close {
            self.close_dialog(&ctx);
        }
        self.save_settings_if_changed();
    }
}

/// Converting between screen pixels and material millimetres.
impl App {
    fn w2s(&self, p: Point) -> Pos2 {
        self.canvas_rect.min + self.view.offset + egui::vec2(p[0] as f32, p[1] as f32) * self.view.zoom
    }

    fn s2w(&self, p: Pos2) -> Point {
        let v = (p - self.canvas_rect.min - self.view.offset) / self.view.zoom;
        [v.x as f64, v.y as f64]
    }

    /// Round to the grid when snapping is on. The grid follows the zoom: 1 mm close up, coarser further out.
    fn snap(&self, p: Point) -> Point {
        if !self.snap {
            return p;
        }
        let g = if self.view.zoom >= 4.0 { 1.0 } else if self.view.zoom >= 1.0 { 5.0 } else { 10.0 };
        [(p[0] / g).round() * g, (p[1] / g).round() * g]
    }
}
