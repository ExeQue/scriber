//! The "Machines & materials" window, and the machine/material pickers in the ribbon's Laser tab.

use super::App;
use super::lucide;
use super::ribbon::{self, Icon};
use crate::library::{self, Library, Operation};
use crate::model::{Document, State};
use eframe::egui::{self, Color32, RichText};
use material_list::{Filter, Picked};
use widgets::Confirm;

mod import_export;
mod machines;
mod material_list;
mod materials;
mod widgets;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Machines,
    Materials,
}

/// The window's working copy; nothing is saved until the user presses Save.
pub(super) struct Editor {
    lib: Library,
    tab: Tab,
    machine: usize,
    material: usize,
    error: String,
    /// The document's machine and laser: the material list's tags show the settings for them.
    doc_machine: Option<String>,
    doc_laser: Option<String>,
    filter: Filter,
    /// A delete waiting for "yes".
    confirm: Option<Confirm>,
    /// The machine and laser whose settings the material page shows.
    settings_view: materials::SettingsView,
}

impl Editor {
    /// Opens on the document's machine and material.
    pub(super) fn new(s: &State) -> Self {
        let lib = library::get().clone();
        let index = |list: Vec<&String>, id: &Option<String>| list.iter().position(|i| Some(*i) == id.as_ref()).unwrap_or(0);
        let machine = index(lib.machines.iter().map(|m| &m.id).collect(), &s.machine);
        let material = index(lib.materials.iter().map(|m| &m.id).collect(), &s.material);
        Self { lib, tab: Tab::Machines, machine, material, error: String::new(), doc_machine: s.machine.clone(), doc_laser: s.laser.clone(), filter: Filter::default(), confirm: None, settings_view: materials::SettingsView { key: s.machine.clone().zip(s.laser.clone()), search: String::new() } }
    }

    /// Opens on the Materials page instead.
    pub(super) fn on_materials(mut self) -> Self {
        self.tab = Tab::Materials;
        self
    }
}

impl App {
    pub(super) fn library_window(&mut self, ctx: &egui::Context) {
        let Some(ed) = &mut self.library else { return };
        if show(ctx, ed) {
            self.library = None;
        }
    }
}

/// The editor as a modal; returns true when it should close.
fn show(ctx: &egui::Context, ed: &mut Editor) -> bool {
    let mut close = false;
    // A fixed size that still fits small screens, so the layout does not jump between pages.
    let screen = ctx.content_rect().size();
    let size = egui::vec2((screen.x - 80.0).clamp(560.0, 980.0), (screen.y - 220.0).clamp(300.0, 560.0));
    // A centred modal: the drawing waits until the user saves or cancels.
    let modal = egui::Modal::new(egui::Id::unique("library-modal")).show(ctx, |ui| {
        ui.set_width(size.x);
        ui.horizontal(|ui| {
            ui.heading("Machines & materials");
            ui.add_space(24.0);
            ui.selectable_value(&mut ed.tab, Tab::Machines, RichText::new("Machines").size(15.0));
            ui.selectable_value(&mut ed.tab, Tab::Materials, RichText::new("Materials").size(15.0));
            ui.add_space(24.0);
            import_export::menus(ui, ed);
        });
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        // The body has exactly this size, laid out left to right: the sidebar, then the form.
        ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.set_min_size(size);
            match ed.tab {
                Tab::Machines => machines::page(ui, ed),
                Tab::Materials => materials::page(ui, ed),
            }
        });
        ui.add_space(4.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new(&ed.error).color(Color32::from_rgb(230, 90, 80)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save = egui::Button::new(RichText::new("Save").strong().color(Color32::WHITE)).fill(ui.visuals().selection.bg_fill).min_size(egui::vec2(90.0, 28.0));
                if ui.add(save).clicked() {
                    let lib = ed.lib.clone();
                    match library::update(|l| {
                        *l = lib;
                        Ok(())
                    }) {
                        Ok(()) => close = true,
                        Err(e) => ed.error = e,
                    }
                }
                if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 28.0))).clicked() {
                    close = true;
                }
            });
        });
    });
    // Shown after the window, so it sits on top of it.
    let answer = ed.confirm.as_ref().and_then(|c| widgets::confirm_modal(ctx, &c.question));
    match (answer, ed.confirm.take()) {
        (Some(true), Some(c)) => (c.apply)(&mut ed.lib),
        (None, c) => ed.confirm = c,
        _ => {}
    }
    modal.should_close() || close
}

/// A name cut to fit under a ribbon button.
fn short(name: &str) -> String {
    const MAX: usize = 18;
    if name.chars().count() <= MAX { name.to_string() } else { format!("{}…", name.chars().take(MAX - 1).collect::<String>()) }
}

/// The ribbon's Machine & material panel: large buttons for the document's machine, laser and
/// material, then whether the library has settings for them and "Use work area". Returns true
/// when the user asks to set up the material's settings (opens the editor on the Materials page).
pub(super) fn pickers(ui: &mut egui::Ui, d: &mut Document) -> bool {
    let lib = library::get();
    let s = &d.s;
    let machine = s.machine.as_deref().and_then(|id| lib.machine(id));
    let material = s.material.as_deref().and_then(|id| lib.material(id));
    let (mut new_machine, mut new_laser, mut new_material) = (s.machine.clone(), s.laser.clone(), s.material.clone());
    let (mut use_area, mut set_up) = (None, false);

    let r = ribbon::button(ui, true, false, false, Icon::Glyph(lucide::PRINTER), &format!("{} ▾", short(machine.map_or("No machine", |m| &m.name))));
    let r = r.on_hover_text(format!("Machine: {}\nClick to choose the laser cutter", machine.map_or("none", |m| &m.name)));
    egui::Popup::menu(&r).show(|ui| {
        ui.selectable_value(&mut new_machine, None, "None");
        let mut list: Vec<_> = lib.machines.iter().collect();
        list.sort_by(|a, b| widgets::natural_cmp(&a.name, &b.name));
        for m in list {
            ui.selectable_value(&mut new_machine, Some(m.id.clone()), &m.name);
        }
    });
    if let Some(m) = machine {
        let laser = s.laser.as_deref().and_then(|l| m.laser(l));
        let r = ribbon::button(ui, true, false, false, Icon::Glyph(lucide::ZAP), &format!("{} ▾", short(laser.map_or("Choose laser", |l| &l.name))));
        let r = r.on_hover_text(format!("Laser module: {}\nClick to choose another one", laser.map_or("none", |l| &l.name)));
        egui::Popup::menu(&r).show(|ui| {
            for l in &m.lasers {
                ui.selectable_value(&mut new_laser, Some(l.id.clone()), &l.name);
            }
        });
    }
    // The picker (search and kinds) is a modal whose state lives in egui's memory while it is open.
    let picker = egui::Id::unique("material-picker-state");
    let r = ribbon::button(ui, true, false, false, Icon::Glyph(lucide::LAYERS), &format!("{} ▾", short(material.map_or("No material", |m| &m.name))));
    if r.on_hover_text(format!("Material: {}\nClick to choose the material", material.map_or("none", |m| &m.name))).clicked() {
        ui.data_mut(|data| data.insert_temp(picker, Filter { only_ready: s.machine.is_some(), ..Filter::default() }));
    }
    if let Some(mut f) = ui.data(|data| data.get_temp::<Filter>(picker)) {
        match material_list::picker(ui.ctx(), &mut f, &lib, s.machine.as_deref(), s.laser.as_deref(), s.material.as_deref()) {
            Picked::Nothing => ui.data_mut(|data| {
                data.insert_temp(picker, f);
            }),
            Picked::Closed => ui.data_mut(|data| data.remove::<Filter>(picker)),
            Picked::Chose(id) => {
                new_material = id;
                ui.data_mut(|data| data.remove::<Filter>(picker));
            }
        }
    }

    // Status and work area: two rows of the ribbon's small-button height.
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let (icon, color, text, tip) = match (machine, material) {
            (None, _) => (lucide::CIRCLE_ALERT, ui.visuals().weak_text_color(), "Choose a machine".to_string(), "Pick your laser cutter to see the settings".to_string()),
            (_, None) => (lucide::CIRCLE_ALERT, ui.visuals().weak_text_color(), "Choose a material".to_string(), "Pick the material to see its settings".to_string()),
            (Some(m), Some(mat)) => {
                let hints: Vec<String> = Operation::ALL.iter().filter_map(|op| mat.setting(&m.id, s.laser.as_deref(), *op).map(|x| format!("{}: {}", op.label(), x.hint()))).collect();
                if hints.is_empty() {
                    (lucide::CIRCLE_ALERT, Color32::from_rgb(230, 160, 40), "No settings yet — set up…".to_string(), format!("The library has no settings for {} on this machine and laser.\nClick to add them.", mat.name))
                } else {
                    let ops: Vec<String> = Operation::ALL.iter().filter(|op| mat.setting(&m.id, s.laser.as_deref(), **op).is_some()).map(|op| op.label().to_lowercase()).collect();
                    (lucide::CIRCLE_CHECK, Color32::from_rgb(80, 190, 100), format!("Settings for {}", ops.join(", ")), hints.join("\n"))
                }
            }
        };
        let status = egui::Button::new(RichText::new(format!("{icon}  {text}")).color(color)).frame(false).min_size(egui::vec2(0.0, 22.0));
        let can_set_up = machine.is_some() && material.is_some() && icon == lucide::CIRCLE_ALERT;
        if ui.add(status).on_hover_text(tip).clicked() && can_set_up {
            set_up = true;
        }
        if let Some(m) = machine {
            let [w, h] = m.area(s.laser.as_deref());
            let differs = [w, h] != [s.width, s.height];
            let r = ui.add_enabled_ui(differs, |ui| ribbon::button(ui, false, false, false, Icon::Glyph(lucide::MAXIMIZE), "Use work area")).inner;
            if r.on_hover_text(format!("Make the material as large as the machine's work area, {w} × {h} mm")).clicked() {
                use_area = Some([w, h]);
            }
        }
    });

    if new_machine != s.machine {
        new_laser = new_machine.as_deref().and_then(|id| lib.machine(id)).and_then(|m| m.lasers.first()).map(|l| l.id.clone());
    }
    if (&new_machine, &new_laser, &new_material) == (&s.machine, &s.laser, &s.material) && use_area.is_none() {
        return set_up;
    }
    drop(lib);
    d.checkpoint();
    (d.s.machine, d.s.laser, d.s.material) = (new_machine, new_laser, new_material);
    if let Some([w, h]) = use_area {
        (d.s.width, d.s.height) = (w, h);
    }
    set_up
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Where each text was painted, and whether it is visible (inside its clip rect).
    fn texts(ctx: &egui::Context, ed: &mut Editor, size: egui::Vec2) -> Vec<(String, egui::Rect, bool)> {
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
        let mut out = None;
        // The first frames are sizing passes; the third is what the user sees.
        for _ in 0..3 {
            let mut o = ctx.run_pass(input(), |ctx| _ = show(ctx, ed));
            o.textures_delta.clear();
            out = Some(o);
        }
        let mut found = Vec::new();
        for clipped in out.unwrap().shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                let rect = t.visual_bounding_rect();
                found.push((t.galley.text().to_string(), rect, clipped.clip_rect.contains_rect(rect)));
            }
        }
        found
    }

    fn find<'a>(texts: &'a [(String, egui::Rect, bool)], text: &str) -> &'a (String, egui::Rect, bool) {
        texts.iter().find(|t| t.0 == text || t.0.ends_with(&format!(" {text}"))).unwrap_or_else(|| panic!("\"{text}\" not painted; painted: {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>()))
    }

    /// The sidebar on the left, the form to its right and the footer visible, on a large and a small
    /// screen, with and without materials; list names stay on one line and nothing runs past the modal.
    #[test]
    fn layout_keeps_sidebar_form_and_footer() {
        let mut material = library::Material { id: "ply-3".into(), name: "Basswood plywood 3 mm with a long name".into(), category: library::Category::Plywood, thickness: 3.0, notes: String::new(), source: String::new(), settings: Vec::new() };
        let mut materials = vec![material.clone()];
        (material.id, material.name) = ("ply-10".into(), "Basswood plywood 10 mm".into());
        materials.push(material);
        let full = Library { machines: library::builtin_machines(), materials, ..Default::default() };
        let empty = Library { machines: library::builtin_machines(), ..Default::default() };
        let ctx = egui::Context::default();
        crate::gui::lucide::install(&ctx);
        for size in [egui::vec2(1600.0, 900.0), egui::vec2(800.0, 600.0)] {
            for (lib, tab) in [(&full, Tab::Machines), (&full, Tab::Materials), (&empty, Tab::Materials)] {
                let mut ed = Editor::new(&State::default());
                (ed.lib, ed.tab) = (lib.clone(), tab);
                let t = texts(&ctx, &mut ed, size);
                let what = format!("{size:?}, {} page, {} materials", if tab == Tab::Machines { "machines" } else { "materials" }, lib.materials.len());
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let form_title = if lib.materials.is_empty() && tab == Tab::Materials { material_list::NO_MATERIALS } else { "General" };
                // The form's text is the right-most one of its kind (the empty list says the same).
                let form = t.iter().filter(|x| x.0 == form_title).max_by(|a, b| a.1.left().total_cmp(&b.1.left())).unwrap_or_else(|| panic!("{what}: no \"{form_title}\""));
                let (add, save, cancel) = (find(&t, "Add"), find(&t, "Save"), find(&t, "Cancel"));
                for x in [add, form, save, cancel] {
                    assert!(x.2 && screen.contains_rect(x.1), "{what}: \"{}\" is hidden at {:?}", x.0, x.1);
                }
                assert!(form.1.left() > add.1.left() + 200.0, "{what}: the form must be right of the sidebar: {:?} vs {:?}", form.1, add.1);
                assert!(save.1.left() > form.1.left() && save.1.top() > add.1.top(), "{what}: Save must be bottom right: {:?}", save.1);
                for x in t.iter().filter(|x| x.2) {
                    assert!(x.1.right() <= save.1.right() + 12.0, "{what}: \"{}\" runs past the modal: {:?}", x.0, x.1);
                }
                if tab == Tab::Materials && !lib.materials.is_empty() {
                    let row = t.iter().find(|x| x.0.starts_with("Basswood plywood 10")).expect("the list shows the material");
                    assert!(row.1.height() < 24.0 && row.1.width() > 100.0, "{what}: the list row must be one line: {:?}", row.1);
                    let rows: Vec<&str> = t.iter().filter(|x| x.0.starts_with("Basswood")).map(|x| x.0.as_str()).collect();
                    assert!(rows[0].starts_with("Basswood plywood 3"), "{what}: natural order: {rows:?}");
                }
            }
        }
    }
}
