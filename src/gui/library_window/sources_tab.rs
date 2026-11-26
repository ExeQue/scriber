//! The Sources page: machine and material libraries other people publish on GitHub. Subscribing,
//! syncing and loading the follow picker's lists use the network, on their own thread, so the
//! window never waits; the result comes back over a channel polled each frame.

use super::Editor;
use super::widgets::{form_column, section};
use crate::gui::lucide;
use crate::library::sources::{self, Choices, Net, Source, Track};
use crate::library::{self, Origin, store};
use eframe::egui::{self, RichText};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

const HELP: &str = "Machine and material libraries other people publish on GitHub. Their machines and materials are read-only here; Duplicate makes your own copy. \
Scriber syncs them when it starts, at most once a day, and when you press Sync. Changes here apply at once (no need to press Save).";

/// The automatic sync at start is running.
static AUTO_SYNCING: AtomicBool = AtomicBool::new(false);

/// A follow list from GitHub: (value, label) or why it could not be loaded.
type Loaded = Result<Vec<(String, String)>, String>;

enum Done {
    Text(String),
    Choices(String, Choices, Loaded),
}

#[derive(Default)]
pub(in crate::gui) struct State {
    add: String,
    list: Vec<Source>,
    /// When `list` was read (egui time), so syncs elsewhere show up.
    read_at: Option<f64>,
    /// The network job running and what it is, e.g. "Syncing Acme presets…".
    job: Option<(String, Receiver<Result<Done, String>>)>,
    /// The last job's result; true = error.
    status: (String, bool),
    choices: HashMap<(String, Choices), Loaded>,
    /// A source waiting for "yes, remove it".
    remove: Option<String>,
}

/// Sync every enabled source not checked for a day, on its own thread (the editor at start). The
/// receiver gets one line for the status bar when something changed (else nothing).
pub(in crate::gui) fn auto_sync(ctx: &egui::Context) -> Receiver<String> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    AUTO_SYNCING.store(true, Ordering::SeqCst);
    let spawned = std::thread::Builder::new().name("library-sync".into()).spawn(move || {
        let dir = sources::config_dir();
        let names: HashMap<String, String> = sources::list(&dir).into_iter().map(|s| (s.id.clone(), s.display_name().to_string())).collect();
        let changed: Vec<String> = sources::auto_sync(&dir, &Net).into_iter().flatten().filter(|r| r.changed).map(|r| format!("{}: {}", names.get(&r.id).unwrap_or(&r.id), r.summary)).collect();
        library::reload_sources();
        AUTO_SYNCING.store(false, Ordering::SeqCst);
        if !changed.is_empty() {
            let _ = tx.send(format!("Library sources updated — {}", changed.join("; ")));
        }
        ctx.request_repaint();
    });
    if spawned.is_err() {
        AUTO_SYNCING.store(false, Ordering::SeqCst);
    }
    rx
}

impl Editor {
    /// The sources changed (a sync, subscribe, …): the working copy gets the new base, edits kept.
    pub(in crate::gui) fn sources_changed(&mut self) {
        self.lib = store::rebase(&self.lib, library::get().base.clone());
        self.sources.read_at = None;
    }
}

impl State {
    fn start(&mut self, ctx: &egui::Context, label: String, f: impl FnOnce() -> Result<Done, String> + Send + 'static) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("library-source".into()).spawn(move || {
            let r = f();
            library::reload_sources();
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.job = Some((label, rx)),
            Err(e) => self.status = (e.to_string(), true),
        }
    }

    fn sync(&mut self, ctx: &egui::Context, ids: Vec<String>, label: String) {
        self.start(ctx, label, move || {
            let dir = sources::config_dir();
            let mut lines = Vec::new();
            for id in &ids {
                lines.push(match sources::sync(&dir, &Net, id, true) {
                    Ok(r) if r.changed => format!("{id}: {}", r.summary),
                    Ok(_) => format!("{id}: up to date"),
                    Err(e) => return Err(format!("{id}: {e}")),
                });
            }
            Ok(Done::Text(lines.join("; ")))
        });
    }
}

fn ago(secs: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    match now.saturating_sub(secs) {
        _ if secs == 0 => "never".into(),
        d if d < 60 => "just now".into(),
        d if d < 3600 => format!("{} min ago", d / 60),
        d if d < 86400 => format!("{} h ago", d / 3600),
        d => format!("{} days ago", d / 86400),
    }
}

pub(super) fn page(ui: &mut egui::Ui, ed: &mut Editor) {
    let ctx = ui.ctx().clone();
    let st = &mut ed.sources;
    // A finished job: show its result and take the new base.
    let mut changed = false;
    if let Some((_, rx)) = &st.job
        && let Ok(r) = rx.try_recv()
    {
        st.job = None;
        match r {
            Ok(Done::Text(t)) => (st.status, changed) = ((t, false), true),
            Ok(Done::Choices(id, kind, list)) => _ = st.choices.insert((id, kind), list),
            Err(e) => (st.status, changed) = ((e, true), true),
        }
    }
    let now = ctx.input(|i| i.time);
    if st.read_at.is_none_or(|t| now - t > 1.0) {
        st.list = sources::list(&sources::config_dir());
        st.read_at = Some(now);
    }
    form_column(ui, "sources", |ui| {
        section(ui, "Sources", HELP, |ui| {
            ui.horizontal(|ui| {
                let busy = st.job.is_some();
                let field = ui.add(egui::TextEdit::singleline(&mut st.add).desired_width(320.0).hint_text("owner/repo, owner/repo@v1.2 or a GitHub link"));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let subscribe = ui.add_enabled(!busy && !st.add.trim().is_empty(), egui::Button::new(format!("{} Subscribe", lucide::PLUS))).on_hover_text("Download the repository's machines and materials and keep them in sync");
                if (subscribe.clicked() || enter) && !busy && !st.add.trim().is_empty() {
                    let url = st.add.trim().to_string();
                    st.start(&ctx, format!("Subscribing to {url}…"), move || {
                        sources::subscribe(&sources::config_dir(), &Net, &url, None).map(|(s, d)| Done::Text(format!("Subscribed to {} ({}): {}", s.display_name(), s.track.label(), d.summary())))
                    });
                    st.add.clear();
                }
                let ids: Vec<String> = st.list.iter().filter(|s| s.enabled).map(|s| s.id.clone()).collect();
                if ui.add_enabled(!busy && !ids.is_empty(), egui::Button::new(format!("{} Sync all", lucide::REFRESH_CW))).clicked() {
                    st.sync(&ctx, ids, "Syncing all sources…".into());
                }
            });
            ui.horizontal(|ui| match &st.job {
                Some((label, _)) => {
                    ui.spinner();
                    ui.label(label);
                }
                None if AUTO_SYNCING.load(Ordering::SeqCst) => {
                    ui.spinner();
                    ui.label("Syncing sources…");
                }
                None if !st.status.0.is_empty() => {
                    let color = if st.status.1 { crate::gui::theme::get().error } else { crate::gui::theme::get().ok };
                    ui.label(RichText::new(&st.status.0).color(color));
                }
                None => {}
            });
        });
        if st.list.is_empty() {
            ui.label(RichText::new("No sources yet. Paste a GitHub repository above to subscribe to someone's machines and materials.").weak());
        }
        for s in st.list.clone() {
            source_card(ui, &ctx, st, &ed.lib, &s, &mut changed);
            ui.add_space(8.0);
        }
        let problems = &ed.lib.base.problems;
        if !problems.is_empty() {
            ui.collapsing(format!("{} problem(s) while reading the sources", problems.len()), |ui| {
                for p in problems {
                    ui.label(RichText::new(p).small().color(crate::gui::theme::get().warn));
                }
            });
        }
    });
    if let Some(id) = st.remove.clone() {
        match super::widgets::confirm_modal(&ctx, &format!("Remove the source \"{id}\"?"), "Its machines and materials leave your library and its files are deleted. This happens at once.") {
            Some(true) => {
                st.remove = None;
                st.status = match sources::unsubscribe(&sources::config_dir(), &id) {
                    Ok(()) => (format!("Removed {id}"), false),
                    Err(e) => (e, true),
                };
                library::reload_sources();
                changed = true;
            }
            Some(false) => st.remove = None,
            None => {}
        }
    }
    if changed {
        ed.sources_changed();
    }
}

fn source_card(ui: &mut egui::Ui, ctx: &egui::Context, st: &mut State, lib: &library::Library, s: &Source, changed: &mut bool) {
    let busy = st.job.is_some();
    let weak = ui.visuals().weak_text_color();
    let manifest = sources::read_manifest(&s.folder(&sources::config_dir())).ok();
    let count = |list: Vec<&Origin>| list.into_iter().filter(|o| matches!(o, Origin::Source { id, .. } if *id == s.id)).count();
    let (machines, materials) = (count(lib.base.machines.iter().map(|(o, _)| o).collect()), count(lib.base.materials.iter().map(|(o, _)| o).collect()));
    egui::Frame::group(ui.style()).corner_radius(6).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let mut on = s.enabled;
            if ui.checkbox(&mut on, "").on_hover_text("Use this source's machines and materials").changed() {
                if let Err(e) = sources::set_enabled(&sources::config_dir(), &s.id, on) {
                    st.status = (e, true);
                }
                library::reload_sources();
                *changed = true;
            }
            ui.label(RichText::new(s.display_name()).strong().size(15.0));
            ui.hyperlink_to(RichText::new(s.url.trim_start_matches("https://github.com/")).small(), &s.url);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if lucide::delete(ui, !busy, "Remove this source (asks first)").clicked() {
                    st.remove = Some(s.id.clone());
                }
                if ui.add_enabled(!busy, egui::Button::new(format!("{} Sync now", lucide::REFRESH_CW))).on_hover_text("Check for a new version now").clicked() {
                    st.sync(ctx, vec![s.id.clone()], format!("Syncing {}…", s.display_name()));
                }
                follow_menu(ui, ctx, st, s, busy);
            });
        });
        let mut facts = vec![format!("{} {}", lucide::GIT_BRANCH, s.track.label())];
        if !s.release.is_empty() {
            facts.push(format!("release {}", s.release));
        }
        if !s.pinned_commit.is_empty() {
            facts.push(format!("commit {}", sources::short(&s.pinned_commit)));
        }
        facts.push(format!("{machines} machine(s), {materials} material(s)"));
        facts.push(format!("checked {}", ago(s.last_checked)));
        ui.label(RichText::new(facts.join("  ·  ")).color(weak));
        if let Some(m) = &manifest {
            if !m.description.is_empty() {
                ui.label(&m.description);
            }
            let by: Vec<String> = [(!m.author.is_empty()).then(|| format!("by {}", m.author)), (!m.license.is_empty()).then(|| format!("license {}", m.license)), m.library_version.as_ref().map(|v| format!("version {v}"))].into_iter().flatten().collect();
            if !by.is_empty() {
                ui.label(RichText::new(by.join("  ·  ")).small().color(weak));
            }
        }
        if !s.last_sync.is_empty() {
            ui.label(RichText::new(format!("Last sync: {}", s.last_sync)).small().color(weak));
        }
        if !s.last_error.is_empty() {
            ui.label(RichText::new(format!("{} {}", lucide::CIRCLE_ALERT, s.last_error)).small().color(crate::gui::theme::get().error));
        }
        let missing = sources::missing_machines(lib, &s.id);
        if !missing.is_empty() {
            ui.label(RichText::new(format!("{} Settings for machines you do not have (kept): {}", lucide::CIRCLE_ALERT, missing.join(", "))).small().color(crate::gui::theme::get().warn));
        }
    });
}

/// "Follow ▾": default branch, a branch, a tag, a release, the latest release or this commit.
/// The lists come from GitHub when their submenu opens; choosing one syncs.
fn follow_menu(ui: &mut egui::Ui, ctx: &egui::Context, st: &mut State, s: &Source, busy: bool) {
    let mut chosen = None;
    ui.add_enabled_ui(!busy, |ui| {
        ui.menu_button("Follow ▾", |ui| {
            if ui.button("Default branch").clicked() {
                chosen = Some(Track::Branch(String::new()));
            }
            for (kind, title) in [(Choices::Branches, "Branch"), (Choices::Tags, "Tag"), (Choices::Releases, "Release")] {
                ui.menu_button(title, |ui| match st.choices.get(&(s.id.clone(), kind)) {
                    Some(Err(e)) => _ = ui.label(RichText::new(e).color(crate::gui::theme::get().error)),
                    Some(Ok(list)) if list.is_empty() => _ = ui.label(RichText::new("None").weak()),
                    Some(Ok(list)) => {
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for (value, label) in list {
                                if ui.button(label).clicked() {
                                    chosen = Some(if kind == Choices::Branches { Track::Branch(value.clone()) } else { Track::Tag(value.clone()) });
                                }
                            }
                        });
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Loading…");
                        });
                        let (id, url) = (s.id.clone(), s.url.clone());
                        st.start(ctx, format!("Loading {}…", title.to_lowercase()), move || Ok(Done::Choices(id, kind, sources::choices(&Net, &url, kind))));
                    }
                });
            }
            if ui.button("Latest release").on_hover_text("Follow GitHub's latest release: a new release is an update").clicked() {
                chosen = Some(Track::Release("latest".into()));
            }
            let pinned = !s.pinned_commit.is_empty();
            if ui.add_enabled(pinned, egui::Button::new("This commit")).on_hover_text("Lock to the commit in use: it never changes").clicked() {
                chosen = Some(Track::Commit(s.pinned_commit.clone()));
            }
        });
    });
    if let Some(track) = chosen {
        let id = s.id.clone();
        let label = track.label();
        st.start(ctx, format!("Following {label}…"), move || sources::set_track(&sources::config_dir(), &Net, &id, track).map(|r| Done::Text(format!("{id} follows {label}: {}", r.summary))));
    }
}
