//! Document tabs above the canvas (see `model::workspace`): one per open document, with its file
//! name, • when unsaved and ✕ to close (middle-click closes too). Ctrl+Tab / Ctrl+Shift+Tab switch,
//! Ctrl+N opens a new tab, Ctrl+W closes one (asking Save / Don't save / Cancel when unsaved). Each
//! tab keeps its own view (zoom and pan). An agent may switch or close tabs too, so the app follows
//! the workspace's active document at the start of every frame.

use super::{App, View};
use crate::model::Document;
use eframe::egui::{self, Color32, Key, Modifiers, RichText, Sense, Stroke};
use std::path::PathBuf;

/// The tab's label: file name, or Untitled.
fn title(d: &Document) -> String {
    d.path.as_ref().and_then(|p| p.file_name()).map_or_else(|| "Untitled".into(), |f| f.to_string_lossy().into_owned())
}

impl App {
    /// Show the workspace's active document when it changed (a tab click, a close, an agent).
    pub(super) fn follow_active(&mut self) {
        let (id, doc) = self.ws.active();
        if id == self.doc_id {
            return;
        }
        self.cancel();
        let view = std::mem::replace(&mut self.view, self.views.remove(&id).unwrap_or_default());
        self.views.insert(self.doc_id, view);
        let open: Vec<u64> = self.ws.docs().iter().map(|(i, _)| *i).collect();
        self.views.retain(|i, _| open.contains(i));
        (self.doc_id, self.doc) = (id, doc);
        // What belonged to the other document: image textures (ids repeat across documents),
        // the preview and check caches (keyed by revision), comment threads, isolation, hover.
        self.textures.clear();
        if self.preview.is_some() {
            self.preview = Some(Default::default());
        }
        self.check.forget();
        self.comments.close_thread();
        (self.isolated, self.drag, self.hover, self.measured, self.grip_menu, self.grip_rest) = (None, None, None, None, None, None);
        self.hidden_bars.clear();
    }

    /// Open a new empty tab with the current material size.
    pub(super) fn new_tab(&mut self) {
        let (w, h) = {
            let d = self.doc.lock().unwrap();
            (d.s.width, d.s.height)
        };
        self.ws.add(Document::new(w, h), true);
    }

    /// Close a tab; unsaved changes ask first.
    pub(super) fn close_tab(&mut self, id: u64) {
        let dirty = self.ws.get(id).is_some_and(|d| d.lock().unwrap().dirty);
        if dirty {
            self.ws.activate(id);
            self.closing = Some(id);
        } else {
            self.ws.close(id);
        }
    }

    /// Open a file in a new tab, or show the tab that has it.
    pub(super) fn open_path(&mut self, path: &std::path::Path) {
        match self.ws.open(path, true) {
            Ok((_, true)) => self.message = format!("{} is already open", path.display()),
            Ok(_) => self.message = format!("Opened {}", path.display()),
            Err(e) => self.message = format!("Could not open the file: {e}"),
        }
    }

    /// Open an xTool Studio project as a new, unsaved document in a new tab.
    pub(super) fn open_xs(&mut self, path: &std::path::Path) {
        let lib = crate::library::get().clone();
        match crate::export::xs_import::read_file(path, &lib) {
            Ok(r) => {
                self.ws.add(r.doc, true);
                let items: usize = r.layers.iter().map(|l| l.items).sum();
                let mut msg = format!("Opened {}: {items} items on {} layers.", path.display(), r.layers.len());
                if !r.skipped.is_empty() {
                    msg += &format!(" Left out: {}.", r.skipped.join("; "));
                }
                for n in &r.notes {
                    msg += &format!(" {n}");
                }
                self.message = msg;
            }
            Err(e) => self.message = format!("Could not open the xTool project: {e}"),
        }
    }

    /// Reopen the files that were open at exit (missing ones are skipped).
    pub(super) fn restore_session(&mut self, files: &[PathBuf]) {
        for f in files.iter().filter(|f| f.is_file()) {
            let _ = self.ws.open(f, true);
        }
        if let Some(first) = files.iter().find_map(|f| self.ws.find(f)) {
            self.ws.activate(first);
        }
    }

    /// Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+N, Ctrl+W (also while previewing).
    pub(super) fn tab_keys(&mut self, ctx: &egui::Context) {
        if self.closing.is_some() || self.confirm_close {
            return;
        }
        let consume = |mods: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(mods, k));
        let (ctrl, ctrl_shift) = (Modifiers::COMMAND, Modifiers::COMMAND | Modifiers::SHIFT);
        if consume(ctrl_shift, Key::Tab) {
            self.ws.activate(self.ws.neighbour(-1));
        } else if consume(ctrl, Key::Tab) {
            self.ws.activate(self.ws.neighbour(1));
        } else if consume(ctrl, Key::N) {
            self.new_tab();
        } else if consume(ctrl, Key::W) {
            self.close_tab(self.doc_id);
        }
    }

    /// The strip of tabs above the canvas.
    pub(super) fn tab_strip(&mut self, ui: &mut egui::Ui) {
        let tabs: Vec<(u64, String, bool, Option<PathBuf>)> = self
            .ws
            .docs()
            .iter()
            .map(|(id, d)| {
                let d = d.lock().unwrap();
                (*id, title(&d), d.dirty, d.path.clone())
            })
            .collect();
        let (mut show, mut close) = (None, None);
        egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (id, name, dirty, path) in tabs {
                    let active = id == self.doc_id;
                    let frame = egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(8, 3))
                        .corner_radius(egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 })
                        .fill(if active { ui.visuals().extreme_bg_color } else { ui.visuals().faint_bg_color })
                        .stroke(if active { Stroke::new(1.0, ui.visuals().widgets.active.bg_stroke.color) } else { Stroke::NONE });
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let label = if dirty { format!("{name} •") } else { name.clone() };
                            let text = if active { RichText::new(label).strong() } else { RichText::new(label).color(ui.visuals().weak_text_color()) };
                            let r = ui.add(egui::Label::new(text).selectable(false).sense(Sense::click()));
                            if r.middle_clicked() {
                                close = Some(id);
                            } else if r.clicked() {
                                show = Some(id);
                            }
                            if let Some(path) = &path {
                                r.on_hover_text(path.display().to_string());
                            }
                            let x = ui.add(egui::Button::new(RichText::new("✕").size(11.0)).frame(false).fill(Color32::TRANSPARENT));
                            if x.middle_clicked() || x.on_hover_text("Close (Ctrl+W)").clicked() {
                                close = Some(id);
                            }
                        });
                    });
                }
                if ui.add(egui::Button::new("+").frame(false)).on_hover_text("New drawing (Ctrl+N)").clicked() {
                    self.new_tab();
                }
            });
        });
        if let Some(id) = close {
            self.close_tab(id);
        } else if let Some(id) = show {
            self.ws.activate(id);
        }
    }

    /// Save / Don't save / Cancel for a tab closed with unsaved changes.
    pub(super) fn close_tab_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.closing else { return };
        if self.ws.get(id).is_none() {
            self.closing = None;
            return;
        }
        let name = self.ws.get(id).map(|d| title(&d.lock().unwrap())).unwrap_or_default();
        egui::Modal::new(egui::Id::unique("close-tab")).show(ctx, |ui| {
            ui.heading("Save changes?");
            ui.label(format!("{name} has changes that are not saved."));
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.closing = None;
                    self.save_then(ctx, id, false, super::AfterSave::CloseTab);
                }
                if ui.button("Don't save").clicked() {
                    self.closing = None;
                    self.ws.close(id);
                }
                if ui.button("Cancel").clicked() {
                    self.closing = None;
                }
            });
        });
    }
}

impl Default for View {
    fn default() -> Self {
        Self { offset: egui::Vec2::ZERO, zoom: 3.0, auto_fit: true, fit_size: egui::Vec2::ZERO, history: Vec::new(), wheeling: false }
    }
}
