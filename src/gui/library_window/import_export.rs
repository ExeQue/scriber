//! The Machines & materials window's Import ▾ and Export ▾ menus: materials from xTool Studio (its
//! local cache or an `.xs` project) and Scriber library files (`.scrlib`) to share with other users.
//! Everything lands in the window's working copy; the user still presses Save.

use super::{Editor, Tab, material_list};
use crate::library::exchange::{self, Bundle, OnConflict};
use crate::library::xtool::{self, XtoolGroup, XtoolMaterial};
use crate::library::{LaserKind, Library, Operation};
use crate::gui::file_dialog::Pending;
use eframe::egui::{self, RichText};
use std::sync::Mutex;

const XTOOL_HELP: &str = "Scriber reads the materials xTool Studio keeps on this computer (a read-only copy; nothing is downloaded). \
Only materials you have opened in xTool Studio's material library for your machine are there, or ones you saved to it from atomm.com's Materials Lab with \"Save to Studio\". \
Missing one? Open it in xTool Studio first, then come back. Image (bitmap) engraving settings are not imported.";

/// The xTool picker: the cache's materials (one per material, all its xTool machines), which are
/// ticked, the search text, "replace earlier imports", and whether to show materials for other machines.
struct Picker {
    list: Vec<XtoolGroup>,
    ticked: Vec<bool>,
    search: String,
    overwrite: bool,
    others: bool,
}

#[derive(Default)]
struct State {
    picker: Option<Picker>,
    /// A library file waiting for the answer to "what about ids that are taken?".
    pending: Option<(Bundle, usize)>,
    status: String,
    /// The open file dialog (it runs on its own thread) and what it is for.
    dialog: Pending<Want>,
}

/// What the library's file dialog was opened for.
enum Want {
    XtoolProject,
    Library,
    Export(Bundle),
}

// One window at a time, so one state; it lives here so the window's own struct stays untouched.
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn filter(name: &str, ext: &[&str]) -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter(name, ext)
}

/// A file dialog's answer: import or export.
fn answer(st: &mut State, ed: &mut Editor) {
    let Some((want, path)) = st.dialog.poll() else { return };
    match want {
        Want::XtoolProject => {
            let origin = format!("project {}", path.file_name().unwrap_or_default().to_string_lossy());
            match xtool::read_project(&path) {
                Ok(list) => st.status = import_xtool(ed, &list, &origin, false),
                Err(e) => ed.error = e,
            }
        }
        Want::Library => match exchange::read(&path) {
            Ok(b) => match exchange::conflicts(&ed.lib, &b) {
                0 => st.status = exchange::import(&mut ed.lib, b, OnConflict::Skip).summary(),
                n => st.pending = Some((b, n)),
            },
            Err(e) => ed.error = e,
        },
        Want::Export(b) => match exchange::write(&b, &path) {
            Ok(()) => st.status = format!("Exported {} machine(s) and {} material(s)", b.machines.len(), b.materials.len()),
            Err(e) => ed.error = e,
        },
    }
}

/// The menus and their dialogs; call once per frame while the window is open.
pub(super) fn menus(ui: &mut egui::Ui, ed: &mut Editor) {
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(State::default);
    let before = ed.lib.materials.len();
    answer(st, ed);
    let ctx = ui.ctx().clone();
    ui.menu_button("Import ▾", |ui| {
        if ui.button("From xTool Studio…").on_hover_text(XTOOL_HELP).clicked() {
            match xtool::read_cache(None) {
                Ok(list) => {
                    let list = xtool::group(list);
                    st.picker = Some(Picker { ticked: vec![false; list.len()], list, search: String::new(), overwrite: false, others: false });
                }
                Err(e) => ed.error = e,
            }
        }
        if ui.button("xTool project (.xs)…").on_hover_text("The laser settings of an xTool Studio project, as a material named after the file").clicked() {
            st.dialog.pick(&ctx, Want::XtoolProject, filter("xTool Studio project", &["xs"]));
        }
        if ui.button("Scriber library file…").on_hover_text("Machines and materials another Scriber user exported").clicked() {
            st.dialog.pick(&ctx, Want::Library, filter("Scriber library", &[exchange::EXTENSION]));
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
            match exchange::bundle(&ed.lib, machines.as_deref(), materials.as_deref()) {
                Ok(b) => st.dialog.save(&ctx, Want::Export(b), filter("Scriber library", &[exchange::EXTENSION]).set_file_name(file)),
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

/// One xTool material's row: tick box, C/S/E tags for the target machine, the name, and under it
/// thickness, kind and laser modules (or why it cannot be imported). Hover shows the numbers.
fn picker_row(ui: &mut egui::Ui, g: &XtoolGroup, ticked: &mut bool, lib: &Library, doc_machine: Option<&str>) {
    let target = g.targets(lib, doc_machine).first().copied();
    let modules = target.map(|(m, x)| xtool::modules(lib, m, x)).unwrap_or_default();
    let mut detail = vec![format!("{} mm", g.thickness), g.category.label().to_string()];
    let hover = match target {
        Some((m, _)) => {
            let name = lib.machine(m).map_or(m, |x| x.name.as_str());
            let watts: Vec<String> = modules.iter().map(|(l, _)| if l.kind == LaserKind::Diode { format!("{} W", l.power_w) } else { format!("{} W {}", l.power_w, l.kind.label()) }).collect();
            detail.push(if Some(m) == doc_machine { watts.join(" · ") } else { format!("{name}: {}", watts.join(" · ")) });
            let per_module = modules.iter().map(|(l, list)| format!("{}\n{}", l.name, list.iter().map(|s| format!("   {}: {}", s.operation.label(), s.hint())).collect::<Vec<_>>().join("\n")));
            format!("For your {name}\n{}", per_module.collect::<Vec<_>>().join("\n"))
        }
        None => {
            detail.push(g.reason(lib));
            g.reason(lib)
        }
    };
    let row = ui.horizontal(|ui| {
        ui.add_enabled_ui(target.is_some(), |ui| {
            ui.checkbox(ticked, "");
            ui.spacing_mut().item_spacing.x = 2.0;
            for op in Operation::ALL {
                material_list::pill(ui, op, modules.iter().any(|(_, list)| list.iter().any(|s| s.operation == op)));
            }
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let name = ui.add(egui::Label::new(RichText::new(&g.name).strong()).truncate().sense(egui::Sense::click()));
                ui.add(egui::Label::new(RichText::new(detail.join("  ·  ")).small().weak()).truncate());
                if name.clicked() {
                    *ticked = !*ticked;
                }
            });
        });
    });
    row.response.on_hover_text(hover);
}

fn picker(ctx: &egui::Context, st: &mut State, ed: &mut Editor) {
    let Some(p) = &mut st.picker else { return };
    let mut close = false;
    let mut import = false;
    let doc_machine = ed.doc_machine.as_deref();
    let modal = egui::Modal::new(egui::Id::unique("xtool-import")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading("Import from xTool Studio");
        ui.label(RichText::new(XTOOL_HELP).small().weak());
        ui.add_space(4.0);
        ui.add(egui::TextEdit::singleline(&mut p.search).hint_text("🔍 Search").desired_width(f32::INFINITY));
        let words: Vec<String> = p.search.to_lowercase().split_whitespace().map(str::to_string).collect();
        let matches = |g: &XtoolGroup| words.iter().all(|w| g.name.to_lowercase().contains(w));
        let importable: Vec<bool> = p.list.iter().map(|g| !g.targets(&ed.lib, doc_machine).is_empty()).collect();
        let hidden = p.list.iter().zip(&importable).filter(|(g, ok)| !**ok && matches(g)).count();
        ui.checkbox(&mut p.others, format!("Show materials for other xTool machines ({hidden})")).on_hover_text("Materials xTool only has settings for machines that are not in your Machines list; they cannot be imported");
        // A solid scroll bar next to the rows, not over their text.
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        let width = ui.available_width();
        egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(ui.available_width().min(width));
            let mut shown = 0;
            for ((g, t), ok) in p.list.iter().zip(p.ticked.iter_mut()).zip(&importable) {
                if matches(g) && (*ok || p.others) {
                    picker_row(ui, g, t, &ed.lib, doc_machine);
                    shown += 1;
                }
            }
            if p.list.is_empty() {
                ui.label("The cache has no materials yet.");
            } else if shown == 0 {
                ui.label(RichText::new("No material matches.").weak());
            }
        });
        ui.add_space(4.0);
        ui.checkbox(&mut p.overwrite, "Replace materials imported from the same xTool entry before");
        ui.horizontal(|ui| {
            let n = p.ticked.iter().zip(&importable).filter(|(t, ok)| **t && **ok).count();
            if ui.add_enabled(n > 0, egui::Button::new(format!("Import {n}"))).clicked() {
                import = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if import {
        let chosen: Vec<XtoolMaterial> = p.list.iter().zip(&p.ticked).filter(|(_, t)| **t).filter_map(|(g, _)| g.targets(&ed.lib, doc_machine).first().map(|(_, x)| (*x).clone())).collect();
        let overwrite = p.overwrite;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Category;
    use crate::library::xtool::XtoolSetting;

    fn set(id: &str, name: &str, device: &str) -> XtoolMaterial {
        let s = |operation| XtoolSetting { operation, source: "blue".into(), watts: 10.0, power: 80.0, speed: 10.0, passes: 1, lines_per_cm: None };
        XtoolMaterial { key: format!("{id}@{device}"), name: name.into(), thickness: 3.0, category: Category::Plywood, device: device.into(), settings: vec![s(Operation::Cut), s(Operation::Score)] }
    }

    /// The painted texts of the picker after a few frames, each with whether it is inside its clip rect.
    fn paint(p: Picker) -> Vec<(String, bool, f32)> {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(&crate::model::State::default());
        ed.lib = crate::library::Library { machines: crate::library::builtin_machines(), ..Default::default() };
        let mut st = State { picker: Some(p), ..Default::default() };
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0))), ..Default::default() };
        let mut out = None;
        for _ in 0..3 {
            let mut o = ctx.run_pass(input(), |ctx| picker(ctx, &mut st, &mut ed));
            o.textures_delta.clear();
            out = Some(o);
        }
        out.unwrap()
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_string(), c.clip_rect.contains_rect(t.visual_bounding_rect()), t.visual_bounding_rect().width())),
                _ => None,
            })
            .collect()
    }

    /// One row per material, other machines' materials hidden until asked for, and long names cut
    /// off inside the list (not under the scroll bar).
    #[test]
    fn picker_lists_each_material_once() {
        let long = "Basswood plywood with a very long name that cannot fit in the dialog at all, not even close, 3 mm";
        let list = xtool::group(vec![set("7", long, "GS003"), set("7", long, "JS002"), set("8", "Felt", "ZZ999")]);
        let picker = |others| Picker { ticked: vec![false; list.len()], list: list.clone(), search: String::new(), overwrite: false, others };
        let texts = paint(picker(false));
        let rows: Vec<&(String, bool, f32)> = texts.iter().filter(|t| t.0.starts_with("Basswood")).collect();
        assert_eq!(rows.len(), 1, "one row for both devices: {texts:?}");
        assert!(rows[0].2 < 500.0 && rows[0].1, "the name is truncated inside the list: {rows:?}");
        assert!(texts.iter().any(|t| t.0 == "3 mm  ·  Plywood & MDF  ·  xTool M2: 10 W"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.0 == "Felt"), "other machines' materials are hidden");
        assert!(texts.iter().any(|t| t.0.ends_with("(1)")), "the toggle counts them");
        let texts = paint(picker(true));
        assert!(texts.iter().any(|t| t.0.ends_with("for xTool device ZZ999 — not in your machines")), "{texts:?}");
        assert!(texts.iter().all(|t| t.1 || t.0.is_empty()), "no text is clipped: {texts:?}");
    }
}
