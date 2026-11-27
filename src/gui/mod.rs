//! The desktop window. This file holds the app state, the layout and the file/edit actions;
//! the canvas, side panels and agent window live in their own files.

mod agents_window;
mod canvas;
mod check_panel;
mod clipboard;
mod command;
mod comments;
mod constraint_bars;
mod constraint_list;
mod cycle;
mod dimensions;
mod fonts;
mod file_dialog;
mod geometry_cache;
mod glyphs;
mod grip_menu;
mod grips;
mod guides;
mod hover;
mod icons;
mod icons_ux;
mod labels;
mod layers;
mod library_window;
mod lucide;
mod overrides;
mod panels;
mod parameters;
mod picked;
mod popouts;
mod preview;
mod ribbon;
mod select;
mod settings;
mod settings_window;
mod shortcuts;
mod snap;
mod svg_import;
mod dxf_import;
mod tabs;
mod theme;
mod tools;
mod tracking;
mod variant_icons;
mod zoom;

use crate::model::{Document, Op, Point, Shape, Shared, Transform, Workspace};
use crate::{agents, export, mcp};
use eframe::egui::{self, Event, Key, Modifiers, Pos2, Rect, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use select::Drag;
use tools::Tool;

/// What a file dialog was opened for; the document ids are the tab it was opened from.
enum Want {
    Open,
    Save(u64, AfterSave),
    ImportImage(u64),
    ImportSvg,
    ImportDxf,
    ExportSvg(u64),
    ExportDxf(u64),
    ExportPng(u64),
    ExportXs(u64),
}

/// What to do once a save is done.
#[derive(Clone, Copy)]
enum AfterSave {
    Nothing,
    CloseTab,
    Quit,
}

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
    /// The open documents (tabs); `doc` is the active one's, `doc_id` its id.
    ws: Workspace,
    doc: Shared,
    doc_id: u64,
    /// The other tabs' views (zoom and pan), by document id.
    views: HashMap<u64, View>,
    /// A tab with unsaved changes waiting for Save / Don't save / Cancel.
    closing: Option<u64>,
    /// Quit once the save that was asked for is done.
    quit_after_save: bool,
    /// The file dialog that is open (it runs on its own thread) and what it is for.
    dialog: file_dialog::Pending<Want>,
    mcp: Result<u16, String>,
    tool: Tool,
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
    /// Values typed into dynamic input for the draft's points: they become dimensions.
    draft_typed: Vec<command::Typed>,
    drag: Option<Drag>,
    cmdline: command::CommandLine,
    /// The AutoCAD-style command line; hidden unless the user turns it on (ribbon View tab).
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
    /// The same for the layers panel.
    layers_editing: bool,
    /// The Edit layers dialog is open.
    show_layers: bool,
    /// A layer button pressed in the dialog, applied after it is drawn.
    layer_action: Option<layers::Action>,
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
    /// Where the canvas was right-clicked (Paste there).
    menu_at: Option<Point>,
    /// The last copy, for when the system clipboard gives nothing back.
    clip: Option<String>,
    paste_request: Option<clipboard::PasteRequest>,
    /// The constraint pointed at in the properties list, highlighted on the canvas.
    hot_constraint: Option<u64>,
    agents: Arc<Mutex<Option<Vec<agents::Target>>>>,
    confirm_close: bool,
    allow_close: bool,
    svg_dialog: Option<svg_import::SvgDialog>,
    dxf_dialog: Option<dxf_import::DxfDialog>,
    /// The op last picked per DXF layer name (None = skipped), for the next DXF import.
    dxf_ops: std::collections::BTreeMap<String, Option<Op>>,
    /// The open "Machines & materials" window.
    library: Option<library_window::Editor>,
    /// The automatic library sources sync started with the editor: one line when something changed.
    library_sync: Option<std::sync::mpsc::Receiver<String>>,
    /// The ribbon's tab, fold and layout.
    ribbon: ribbon::Ribbon,
    /// Help > Keyboard shortcuts is open.
    show_shortcuts: bool,
    /// Show guide lines (ribbon Annotate and View tabs).
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
    popouts: popouts::Popouts,
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
    /// Measure on hover (ribbon View tab) (else hold Ctrl), and the last two snap points pointed at.
    measure_hover: bool,
    /// The settings as last saved (status bar toggles, view choices).
    saved_settings: settings::Settings,
    measure_pts: (Option<Point>, Option<Point>),
    /// The grip the pointer rests on (since when), and the grip whose menu is open.
    grip_rest: Option<(grips::Grip, std::time::Instant)>,
    grip_menu: Option<(grips::Grip, Point)>,
    /// POLAR's additional angles as typed in its menu.
    polar_extra_text: String,
    /// Review comments: the Comment tool, pins and their windows.
    comments: comments::Comments,
    /// The colour theme and what it follows.
    theme: theme::State,
    /// Reopen the files open at exit, and sync the library sources, at start (Settings).
    reopen_files: bool,
    auto_sync: bool,
    /// The open Settings dialog and its page.
    settings: Option<settings_window::Section>,
}

pub fn run(file: Option<PathBuf>) -> anyhow::Result<()> {
    #[cfg(windows)]
    detach_console();
    let saved = settings::load();
    let ws = Workspace::new(Document::new(300.0, 200.0));
    if let Some(f) = &file {
        ws.open(f, true)?;
    }
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
            lucide::install(&ctx);
            theme::watch(ctx.clone());
            let mcp = mcp::listen(ws.clone(), Arc::new(move || ctx.request_repaint())).map_err(|e| e.to_string());
            let mut app = App::new(ws, mcp);
            if saved.auto_sync {
                app.library_sync = Some(library_window::auto_sync(&cc.egui_ctx));
            }
            if file.is_none() && saved.reopen_files {
                app.restore_session(&saved.open_files);
            }
            app.apply_settings(saved);
            app.follow_active();
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// The recently used projects as the editor keeps them (for `scriber mcp --headless`).
pub fn recent_files() -> Vec<PathBuf> {
    settings::load().recent
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
    fn new(ws: Workspace, mcp: Result<u16, String>) -> Self {
        let (doc_id, doc) = ws.active();
        Self {
            ws,
            doc,
            doc_id,
            views: HashMap::new(),
            closing: None,
            quit_after_save: false,
            dialog: file_dialog::Pending::default(),
            mcp,
            tool: Tool::Select,
            sides: 6,
            snap: true,
            aids: snap::Aids::default(),
            draft: Vec::new(),
            draft_links: Vec::new(),
            draft_along: Vec::new(),
            draft_typed: Vec::new(),
            drag: None,
            cmdline: command::CommandLine::new(),
            show_cmdline: false,
            measured: None,
            pointer_mm: None,
            dyn_tabs: 0,
            picked: Vec::new(),
            view: View::default(),
            canvas_rect: Rect::NOTHING,
            textures: HashMap::new(),
            geometry: geometry_cache::GeometryCache::default(),
            panel_editing: false,
            layers_editing: false,
            show_layers: false,
            layer_action: None,
            message: String::new(),
            show_agents: false,
            bars: true,
            hidden_bars: Vec::new(),
            show_parameters: false,
            param_edit: None,
            dim_labels: Vec::new(),
            bar_badges: Vec::new(),
            menu_constraint: None,
            menu_at: None,
            clip: None,
            paste_request: None,
            hot_constraint: None,
            agents: Arc::new(Mutex::new(None)),
            confirm_close: false,
            allow_close: false,
            svg_dialog: None,
            dxf_dialog: None,
            dxf_ops: Default::default(),
            library: None,
            library_sync: None,
            ribbon: ribbon::Ribbon::default(),
            show_shortcuts: false,
            show_guides: true,
            isolated: None,
            space_panned: false,
            preview: None,
            check: check_panel::Check::default(),
            shift: false,
            popouts: popouts::Popouts::default(),
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
            comments: comments::Comments::default(),
            theme: theme::State::default(),
            reopen_files: true,
            auto_sync: true,
            settings: None,
        }
    }

    // ---------- files ----------

    fn open_dialog(&mut self, ctx: &egui::Context) {
        self.dialog.pick(ctx, Want::Open, rfd::FileDialog::new().add_filter("Scriber", &["scrbr"]));
    }

    /// A file dialog's answer: carry on with what it was opened for.
    fn dialog_answer(&mut self) {
        let Some((want, path)) = self.dialog.poll() else { return };
        match want {
            Want::Open => self.open_path(&path),
            Want::Save(id, after) => self.save_to(id, path.with_extension("scrbr"), after),
            Want::ImportImage(id) => self.import_image_into(id, &path, [10.0, 10.0]),
            Want::ImportSvg => self.svg_file_chosen(path),
            Want::ImportDxf => self.dxf_file_chosen(path, None),
            Want::ExportDxf(id) => self.export_dxf_to(id, &path),
            Want::ExportSvg(id) => self.export_svg_to(id, &path),
            Want::ExportPng(id) => self.export_png_to(id, &path),
            Want::ExportXs(id) => self.export_xs_to(id, &path),
        }
    }

    /// Save the active tab (asking for a file name when needed).
    fn save(&mut self, ctx: &egui::Context, save_as: bool) {
        self.save_then(ctx, self.doc_id, save_as, AfterSave::Nothing);
    }

    /// Save a tab, asking for a file name when it has none (or `save_as`), then do `after`.
    fn save_then(&mut self, ctx: &egui::Context, id: u64, save_as: bool, after: AfterSave) {
        let Some(doc) = self.ws.get(id) else { return };
        let current = doc.lock().unwrap().path.clone();
        match current {
            Some(p) if !save_as => self.save_to(id, p, after),
            _ => self.dialog.save(ctx, Want::Save(id, after), rfd::FileDialog::new().add_filter("Scriber", &["scrbr"]).set_file_name("design.scrbr")),
        }
    }

    fn save_to(&mut self, id: u64, path: PathBuf, after: AfterSave) {
        let Some(doc) = self.ws.get(id) else { return };
        let result = doc.lock().unwrap().save(&path);
        match result {
            Ok(()) => {
                self.ws.remember(&path);
                self.message = format!("Saved {}", path.display());
                match after {
                    AfterSave::Nothing => {}
                    AfterSave::CloseTab => _ = self.ws.close(id),
                    AfterSave::Quit => self.quit_after_save = true,
                }
            }
            Err(e) => self.message = format!("Could not save: {e}"),
        }
    }

    /// Quit with unsaved tabs: save those with a file, then ask for a name for the first without
    /// one (showing it); the quit comes back here until nothing is left unsaved.
    fn save_all_and_quit(&mut self, ctx: &egui::Context) {
        for (id, doc) in self.ws.docs() {
            let (dirty, path) = {
                let d = doc.lock().unwrap();
                (d.dirty, d.path.clone())
            };
            match (dirty, path) {
                (false, _) => {}
                (true, Some(p)) => self.save_to(id, p, AfterSave::Nothing),
                (true, None) => {
                    self.ws.activate(id);
                    return self.save_then(ctx, id, false, AfterSave::Quit);
                }
            }
        }
        self.quit_after_save = true;
    }

    fn import_image_dialog(&mut self, ctx: &egui::Context) {
        let exts = ["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"];
        self.dialog.pick(ctx, Want::ImportImage(self.doc_id), rfd::FileDialog::new().add_filter("Images", &exts));
    }

    /// Import an image into a tab (it becomes the tab shown).
    fn import_image_into(&mut self, id: u64, path: &Path, at: Point) {
        if self.ws.activate(id) {
            self.follow_active();
            self.import_image(path, at);
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

    fn export_svg(&mut self, ctx: &egui::Context) {
        self.dialog.save(ctx, Want::ExportSvg(self.doc_id), rfd::FileDialog::new().add_filter("SVG", &["svg"]).set_file_name("design.svg"));
    }

    fn export_svg_to(&mut self, id: u64, path: &Path) {
        let Some(doc) = self.ws.get(id) else { return };
        let svg = export::svg(&doc.lock().unwrap().s, false);
        self.message = match std::fs::write(path, svg) {
            Ok(()) => format!("{} {}", "Exported", path.display()),
            Err(e) => format!("{}: {e}", "Export failed"),
        };
    }

    fn export_dxf(&mut self, ctx: &egui::Context) {
        self.dialog.save(ctx, Want::ExportDxf(self.doc_id), rfd::FileDialog::new().add_filter("DXF", &["dxf"]).set_file_name("design.dxf"));
    }

    fn export_dxf_to(&mut self, id: u64, path: &Path) {
        let Some(doc) = self.ws.get(id) else { return };
        let out = export::dxf::dxf(&doc.lock().unwrap().s, false);
        self.message = match out.and_then(|o| std::fs::write(path, &o.bytes).map(|()| o).map_err(|e| e.to_string())) {
            Ok(o) if o.skipped == 0 => format!("Exported {} ({} entities on {} layers)", path.display(), o.entities, o.layers),
            Ok(o) => format!("Exported {} ({} entities on {} layers); {} images left out", path.display(), o.entities, o.layers, o.skipped),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    fn export_png(&mut self, ctx: &egui::Context) {
        self.dialog.save(ctx, Want::ExportPng(self.doc_id), rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("design.png"));
    }

    fn export_png_to(&mut self, id: u64, path: &Path) {
        let Some(doc) = self.ws.get(id) else { return };
        let state = doc.lock().unwrap().s.clone();
        self.message = match export::png(&state, 254.0, false).and_then(|png| Ok(std::fs::write(path, png)?)) {
            Ok(()) => format!("{} {} (254 DPI)", "Exported", path.display()),
            Err(e) => format!("{}: {e}", "Export failed"),
        };
    }

    fn export_xs(&mut self, ctx: &egui::Context) {
        self.dialog.save(ctx, Want::ExportXs(self.doc_id), rfd::FileDialog::new().add_filter("xTool Studio", &["xs"]).set_file_name("design.xs"));
    }

    fn export_xs_to(&mut self, id: u64, path: &Path) {
        let Some(doc) = self.ws.get(id) else { return };
        let state = doc.lock().unwrap().s.clone();
        let path = path.with_extension("xs");
        let name = path.file_stem().map_or("Scriber".into(), |s| s.to_string_lossy().into_owned());
        let lib = crate::library::get().clone(); // not locked while xs renders the cover (it locks the library)
        self.message = match export::xs(&state, &lib, &name).and_then(|x| Ok((std::fs::write(&path, &x.bytes)?, x))) {
            Ok(((), x)) if x.skipped.is_empty() => format!("Exported {} ({} elements)", path.display(), x.elements),
            Ok(((), x)) => format!("Exported {} ({} elements); left out: {}", path.display(), x.elements, x.skipped.join("; ")),
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
        d.selection = d.s.items.iter().filter(|i| d.s.editable(i)).map(|i| i.id).collect();
    }

    /// Add a drawn shape on the current layer (or, with an `op`, a layer with that op; see `new_item_layer`).
    fn create(&mut self, name: &str, op: Option<Op>, shape: Shape) -> u64 {
        let mut d = self.doc.lock().unwrap();
        d.checkpoint();
        let layer = d.new_item_layer(op);
        let id = d.add_to(layer, name, shape);
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
        if self.confirm_close || self.settings.is_some() || (focused.is_some_and(|f| f != input) && ctx.text_edit_focused()) {
            return;
        }
        if self.show_cmdline && focused != Some(input) {
            ctx.memory_mut(|m| m.request_focus(input));
        }
        let consume = |mods: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(mods, k));
        let (ctrl, ctrl_shift) = (Modifiers::COMMAND, Modifiers::COMMAND | Modifiers::SHIFT);
        self.clipboard_events(ctx, pointer);
        // Most specific first: Ctrl+Shift+Z must not also count as Ctrl+Z.
        match () {
            _ if consume(ctrl_shift, Key::Z) => _ = self.doc.lock().unwrap().redo(),
            _ if consume(ctrl, Key::Z) => _ = self.doc.lock().unwrap().undo(),
            _ if consume(ctrl, Key::Y) => _ = self.doc.lock().unwrap().redo(),
            _ if consume(ctrl_shift, Key::S) => self.save(ctx, true),
            _ if consume(ctrl, Key::S) => self.save(ctx, false),
            _ if consume(ctrl, Key::O) => self.open_dialog(ctx),
            _ if consume(ctrl, Key::Comma) => self.settings = Some(Default::default()),
            _ if consume(ctrl_shift, Key::V) => self.request_paste(ctx, None, true),
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
        let unsaved = self.ws.docs().iter().filter(|(_, d)| d.lock().unwrap().dirty).count();
        egui::Modal::new(egui::Id::unique("close")).show(ctx, |ui| {
            ui.heading("Save changes?");
            ui.label(if unsaved > 1 { format!("{unsaved} drawings have changes that are not saved.") } else { "The drawing has changes that are not saved.".into() });
            ui.horizontal(|ui| {
                if ui.button(if unsaved > 1 { "Save all" } else { "Save" }).clicked() {
                    self.confirm_close = false;
                    self.save_all_and_quit(ctx);
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
        self.theme.update(&ctx);
        self.dialog_answer();
        self.follow_active();
        self.library_synced();
        let title = {
            let d = self.doc.lock().unwrap();
            let name = d.path.as_ref().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().into_owned());
            let name = name.unwrap_or_else(|| "Untitled".into());
            format!("{name}{} — Scriber", if d.dirty { " •" } else { "" })
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        if std::mem::take(&mut self.quit_after_save) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let dirty = self.ws.docs().iter().any(|(_, d)| d.lock().unwrap().dirty);
        if ctx.input(|i| i.viewport().close_requested()) && dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }

        let hover = ctx.pointer_hover_pos().filter(|p| self.canvas_rect.contains(*p));
        let pointer = hover.map(|p| self.snap(self.s2w(p)));

        // No editing in preview mode.
        let editing = self.preview.is_none();
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::top("ribbon").show(ui, |ui| self.ribbon(ui, editing));
        self.aid_keys(&ctx);
        self.dyn_keys(&ctx);
        self.shortcut_keys(&ctx, editing);
        self.tab_keys(&ctx);
        // The ribbon or a shortcut may have just toggled the preview.
        let editing = self.preview.is_none();
        if editing {
            self.keys(&ctx, pointer);
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, pointer));
        if self.show_cmdline {
            egui::Panel::bottom("command").show(ui, |ui| self.command_line(ui));
        }
        egui::Panel::right("props").default_size(290.0).show(ui, |ui| {
            ui.add_enabled_ui(editing, |ui| {
                self.layers_panel(ui);
                ui.separator();
                self.comments_panel(ui);
                self.properties(ui);
            })
        });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            egui::Panel::top("tabs").show(ui, |ui| self.tab_strip(ui));
            self.canvas(ui)
        });
        self.command_card(&ctx);
        self.dyn_cursor(&ctx);
        self.grip_menu_window(&ctx);
        self.override_window(&ctx);


        let (svg, dxf) = (self.svg_dialog.is_some(), self.dxf_dialog.is_some());
        self.popouts.track(&[("parameters", self.show_parameters), ("agents", self.show_agents), ("check", self.check.open), ("shortcuts", self.show_shortcuts), ("svg", svg), ("dxf", dxf)]);
        if self.show_layers {
            self.layers_dialog(&ctx);
        }
        if self.show_parameters {
            self.parameters_window(&ctx);
        }
        if self.show_agents {
            self.agents_window(&ctx);
        }
        self.check_window(&ctx);
        if self.show_shortcuts {
            self.shortcuts_window(&ctx);
        }
        self.svg_import_window(&ctx);
        self.dxf_import_window(&ctx);
        self.library_window(&ctx);
        self.settings_window(&ctx);
        if self.confirm_close {
            self.close_dialog(&ctx);
        }
        self.close_tab_dialog(&ctx);
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
