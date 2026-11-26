//! The Machines & materials window's Import ▾ and Export ▾ menus: materials from xTool Studio (its
//! local cache or an `.xs` project) and Scriber library files (`.scrlib`) to share with other users.
//! Everything lands in the window's working copy; the user still presses Save.

use super::{Editor, Tab};
use crate::library::exchange::{self, Bundle, OnConflict};
use crate::library::xtool::{self, XtoolMaterial};
use eframe::egui::{self, RichText};
use std::sync::Mutex;

const XTOOL_HELP: &str = "Scriber reads the materials xTool Studio keeps on this computer (a read-only copy; nothing is downloaded). \
Only materials you have opened in xTool Studio's material library for your machine are there, or ones you saved to it from atomm.com's Materials Lab with \"Save to Studio\". \
Missing one? Open it in xTool Studio first, then come back. Image (bitmap) engraving settings are not imported.";

#[derive(Default)]
struct State {
    /// The xTool picker: the cache's materials, which are ticked, the search text.
    picker: Option<(Vec<XtoolMaterial>, Vec<bool>, String, bool)>,
    /// A library file waiting for the answer to "what about ids that are taken?".
    pending: Option<(Bundle, usize)>,
    status: String,
}

// One window at a time, so one state; it lives here so the window's own struct stays untouched.
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn pick(name: &str, ext: &[&str]) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new().add_filter(name, ext).pick_file()
}

/// The menus and their dialogs; call once per frame while the window is open.
pub(super) fn menus(ui: &mut egui::Ui, ed: &mut Editor) {
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(State::default);
    let before = ed.lib.materials.len();
    ui.menu_button("Import ▾", |ui| {
        if ui.button("From xTool Studio…").on_hover_text(XTOOL_HELP).clicked() {
            match xtool::read_cache(None) {
                Ok(list) => st.picker = Some((list.clone(), vec![false; list.len()], String::new(), false)),
                Err(e) => ed.error = e,
            }
        }
        if ui.button("xTool project (.xs)…").on_hover_text("The laser settings of an xTool Studio project, as a material named after the file").clicked()
            && let Some(path) = pick("xTool Studio project", &["xs"])
        {
            let origin = format!("project {}", path.file_name().unwrap_or_default().to_string_lossy());
            match xtool::read_project(&path) {
                Ok(list) => st.status = import_xtool(ed, &list, &origin, false),
                Err(e) => ed.error = e,
            }
        }
        if ui.button("Scriber library file…").on_hover_text("Machines and materials another Scriber user exported").clicked()
            && let Some(path) = pick("Scriber library", &[exchange::EXTENSION])
        {
            match exchange::read(&path) {
                Ok(b) => match exchange::conflicts(&ed.lib, &b) {
                    0 => st.status = exchange::import(&mut ed.lib, b, OnConflict::Skip).summary(),
                    n => st.pending = Some((b, n)),
                },
                Err(e) => ed.error = e,
            }
        }
    });
    ui.menu_button("Export ▾", |ui| {
        let material = (ed.tab == Tab::Materials).then(|| ed.lib.materials.get(ed.material)).flatten().map(|m| (m.id.clone(), m.name.clone()));
        let machine = (ed.tab == Tab::Machines).then(|| ed.lib.machines.get(ed.machine)).flatten().map(|m| (m.id.clone(), m.name.clone()));
        let mut choice = None;
        if let Some((id, name)) = &material
            && ui.button(format!("Material \"{name}\"…")).on_hover_text("With the machines its settings use").clicked()
        {
            choice = Some((None, Some(vec![id.clone()]), name.clone()));
        }
        if let Some((id, name)) = &machine
            && ui.button(format!("Machine \"{name}\"…")).clicked()
        {
            choice = Some((Some(vec![id.clone()]), None, name.clone()));
        }
        if ui.button("Whole library…").clicked() {
            choice = Some((None, None, "library".into()));
        }
        if let Some((machines, materials, name)) = choice {
            let file = format!("{}.{}", ed.lib.new_id(&name), exchange::EXTENSION);
            let result = exchange::bundle(&ed.lib, machines.as_deref(), materials.as_deref()).and_then(|b| {
                let Some(path) = rfd::FileDialog::new().add_filter("Scriber library", &[exchange::EXTENSION]).set_file_name(file).save_file() else { return Ok(String::new()) };
                exchange::write(&b, &path)?;
                Ok(format!("Exported {} machine(s) and {} material(s)", b.machines.len(), b.materials.len()))
            });
            match result {
                Ok(s) => st.status = s,
                Err(e) => ed.error = e,
            }
        }
    });
    if !st.status.is_empty() {
        ui.label(RichText::new(&st.status).weak().small());
    }
    picker(ui.ctx(), st, ed);
    conflict_question(ui.ctx(), st, ed);
    // Show what was just imported.
    if ed.lib.materials.len() > before {
        ed.tab = Tab::Materials;
        ed.material = ed.lib.materials.len() - 1;
    }
}

fn import_xtool(ed: &mut Editor, list: &[XtoolMaterial], origin: &str, overwrite: bool) -> String {
    let (mut done, mut errors) = (0, Vec::new());
    for x in list {
        match xtool::import(&mut ed.lib, x, origin, overwrite) {
            Ok(_) => done += 1,
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        ed.error = errors.join("; ");
    }
    format!("Imported {done} material(s) from xTool Studio")
}

fn picker(ctx: &egui::Context, st: &mut State, ed: &mut Editor) {
    let Some((list, ticked, search, overwrite)) = &mut st.picker else { return };
    let mut close = false;
    let mut import = false;
    let modal = egui::Modal::new(egui::Id::unique("xtool-import")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.heading("Import from xTool Studio");
        ui.label(RichText::new(XTOOL_HELP).small().weak());
        ui.add_space(4.0);
        ui.add(egui::TextEdit::singleline(search).hint_text("🔍 Search").desired_width(f32::INFINITY));
        let words: Vec<String> = search.to_lowercase().split_whitespace().map(str::to_string).collect();
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (x, t) in list.iter().zip(ticked.iter_mut()) {
                if !words.iter().all(|w| x.name.to_lowercase().contains(w)) {
                    continue;
                }
                let machine = xtool::machine_for(&x.device);
                let ops: Vec<String> = xtool::ops_by_module(&ed.lib, x).iter().map(|(m, ops)| format!("{m}: {}", ops.as_array().unwrap().iter().filter_map(|o| o.as_str()).collect::<Vec<_>>().join("/"))).collect();
                ui.add_enabled_ui(machine.is_some(), |ui| {
                    ui.checkbox(t, format!("{}  ·  {} mm", x.name, x.thickness)).on_hover_text(ops.join("\n")).on_disabled_hover_text(format!("No Scriber machine profile for xTool device {} yet", x.device));
                });
            }
            if list.is_empty() {
                ui.label("The cache has no materials yet.");
            }
        });
        ui.checkbox(overwrite, "Replace materials imported from the same xTool entry before");
        ui.horizontal(|ui| {
            let n = ticked.iter().filter(|t| **t).count();
            if ui.add_enabled(n > 0, egui::Button::new(format!("Import {n}"))).clicked() {
                import = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if import {
        let chosen: Vec<XtoolMaterial> = list.iter().zip(ticked.iter()).filter(|(_, t)| **t).map(|(x, _)| x.clone()).collect();
        let overwrite = *overwrite;
        st.status = import_xtool(ed, &chosen, "cache", overwrite);
        close = true;
    }
    if close || modal.should_close() {
        st.picker = None;
    }
}

fn conflict_question(ctx: &egui::Context, st: &mut State, ed: &mut Editor) {
    let Some((_, n)) = &st.pending else { return };
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::unique("library-import-conflict")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(format!("{n} machine(s) or material(s) in the file have the same id as different ones in your library."));
        ui.horizontal(|ui| {
            if ui.button("Replace").on_hover_text("Use the file's versions").clicked() {
                answer = Some(Some(OnConflict::Replace));
            }
            if ui.button("Keep both").on_hover_text("Import them under new ids").clicked() {
                answer = Some(Some(OnConflict::KeepBoth));
            }
            if ui.button("Skip").on_hover_text("Keep yours; import only the new ones").clicked() {
                answer = Some(Some(OnConflict::Skip));
            }
            if ui.button("Cancel").clicked() {
                answer = Some(None);
            }
        });
    });
    if modal.should_close() {
        answer = Some(None);
    }
    if let Some(a) = answer
        && let Some((b, _)) = st.pending.take()
        && let Some(on) = a
    {
        st.status = exchange::import(&mut ed.lib, b, on).summary();
    }
}
