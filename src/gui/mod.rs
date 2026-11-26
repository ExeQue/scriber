//! The desktop window. This file holds the app state, the layout and the file/edit actions;
//! the canvas, side panels and agent window live in their own files.

mod agents_window;
mod canvas;
mod command;
mod constraint_bars;
mod dimensions;
mod fonts;
mod geometry_cache;
mod glyphs;
mod grips;
mod labels;
mod panels;
mod parameters;
mod select;
mod snap;
mod svg_import;
mod toolbar;
mod tools;

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
}

impl View {
    /// Show the whole material again, and keep it in view when the window changes size.
    fn zoom_extents(&mut self) {
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
    new_fill: bool,
    sides: u32,
    snap: bool,
    /// GRID, ORTHO, OSNAP and POLAR (status bar).
    aids: snap::Aids,
    /// Points placed so far with the current tool.
    draft: Vec<Point>,
    drag: Option<Drag>,
    cmdline: command::CommandLine,
    /// The AutoCAD-style command line; hidden unless the user turns it on (View menu).
    show_cmdline: bool,
    measured: Option<(Point, Point)>,
    /// Where the mouse is on the material (snapped), for the command card's live measurements.
    pointer_mm: Option<Point>,
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
    agents: Arc<Mutex<Option<Vec<agents::Target>>>>,
    confirm_close: bool,
    allow_close: bool,
    svg_dialog: Option<svg_import::SvgDialog>,
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
            Ok(Box::new(App::new(shared, mcp)))
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
            new_fill: false,
            sides: 6,
            snap: true,
            aids: snap::Aids::default(),
            draft: Vec::new(),
            drag: None,
            cmdline: command::CommandLine::new(),
            show_cmdline: false,
            measured: None,
            pointer_mm: None,
            view: View { offset: Vec2::ZERO, zoom: 3.0, auto_fit: true, fit_size: Vec2::ZERO },
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
            agents: Arc::new(Mutex::new(None)),
            confirm_close: false,
            allow_close: false,
            svg_dialog: None,
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

    fn create(&mut self, name: &str, op: Op, fill: bool, shape: Shape) {
        let mut d = self.doc.lock().unwrap();
        d.checkpoint();
        let id = d.add(name, op, fill, shape);
        d.selection = vec![id];
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
    fn space_as_enter(&self, ctx: &egui::Context) -> bool {
        if self.takes_text() || !ctx.input(|i| i.key_pressed(Key::Space)) {
            return false;
        }
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { key: Key::Space, .. }) && !matches!(e, Event::Text(t) if t == " ")));
        true
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
        self.keys(&ctx, pointer);
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, pointer));
        if self.show_cmdline {
            egui::Panel::bottom("command").show(ui, |ui| self.command_line(ui));
        }
        egui::Panel::left("tools").exact_size(126.0).show(ui, |ui| self.toolbar(ui));
        egui::Panel::right("props").default_size(290.0).show(ui, |ui| self.properties(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
        self.command_card(&ctx);

        if self.show_parameters {
            self.parameters_window(&ctx);
        }
        if self.show_agents {
            self.agents_window(&ctx);
        }
        self.svg_import_window(&ctx);
        if self.confirm_close {
            self.close_dialog(&ctx);
        }
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
