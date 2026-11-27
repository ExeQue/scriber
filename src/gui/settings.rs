//! The editor's own settings that survive a restart: the status bar toggles (GRID, SNAP, ORTHO,
//! POLAR with its angles, OSNAP, OTRACK, INFER), a few View choices, the ribbon's tab and fold, the
//! ops picked for DXF layers and the Settings dialog's choices. Stored as JSON in the user's config
//! folder; a missing or broken file just means the defaults, and a missing field its default.

use super::App;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    grid: bool,
    snap: bool,
    ortho: bool,
    polar: bool,
    polar_step: f64,
    polar_extra: Vec<f64>,
    osnap: bool,
    otrack: bool,
    infer: bool,
    show_guides: bool,
    show_cmdline: bool,
    measure_hover: bool,
    ribbon_tab: super::ribbon::Tab,
    ribbon_collapsed: bool,
    theme: super::theme::Choice,
    /// File → Open recent, newest first; also what MCP's list_recent_projects shows.
    pub recent: Vec<PathBuf>,
    /// The files open at exit, reopened on start when `reopen_files` is on.
    pub open_files: Vec<PathBuf>,
    pub reopen_files: bool,
    /// Sync the library sources at start (each at most once a day).
    pub auto_sync: bool,
    /// Import DXF: the op last picked per DXF layer name (null = skipped).
    dxf_ops: std::collections::BTreeMap<String, Option<crate::model::Op>>,
}

impl Default for Settings {
    fn default() -> Self {
        let aids = super::snap::Aids::default();
        Self {
            grid: aids.grid,
            snap: true,
            ortho: aids.ortho,
            polar: aids.polar,
            polar_step: aids.polar_step,
            polar_extra: aids.polar_extra,
            osnap: aids.osnap,
            otrack: aids.otrack,
            infer: aids.infer,
            show_guides: true,
            show_cmdline: false,
            measure_hover: false,
            ribbon_tab: Default::default(),
            ribbon_collapsed: false,
            theme: Default::default(),
            recent: Vec::new(),
            open_files: Vec::new(),
            reopen_files: true,
            auto_sync: true,
            dxf_ops: Default::default(),
        }
    }
}

fn file() -> Option<PathBuf> {
    Some(crate::library::sources::config_dir().join("settings.json"))
}

/// The saved settings; tests always get the defaults so the user's file cannot change them.
pub fn load() -> Settings {
    if cfg!(test) {
        return Settings::default();
    }
    file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(s: &Settings) {
    let Some(f) = file() else { return };
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    // Write then rename, so a crash never leaves half a file.
    let tmp = f.with_extension("json.tmp");
    if serde_json::to_vec_pretty(s).is_ok_and(|b| std::fs::write(&tmp, b).is_ok()) {
        let _ = std::fs::rename(tmp, f);
    }
}

impl App {
    fn current_settings(&self) -> Settings {
        let a = &self.aids;
        Settings {
            grid: a.grid,
            snap: self.snap,
            ortho: a.ortho,
            polar: a.polar,
            polar_step: a.polar_step,
            polar_extra: a.polar_extra.clone(),
            osnap: a.osnap,
            otrack: a.otrack,
            infer: a.infer,
            show_guides: self.show_guides,
            show_cmdline: self.show_cmdline,
            measure_hover: self.measure_hover,
            ribbon_tab: self.ribbon.tab,
            ribbon_collapsed: self.ribbon.collapsed,
            theme: self.theme.choice.clone(),
            recent: self.ws.recent(),
            open_files: self.ws.docs().iter().filter_map(|(_, d)| d.lock().unwrap().path.clone()).collect(),
            reopen_files: self.reopen_files,
            auto_sync: self.auto_sync,
            dxf_ops: self.dxf_ops.clone(),
        }
    }

    pub(super) fn apply_settings(&mut self, s: Settings) {
        let a = &mut self.aids;
        (a.grid, a.ortho, a.polar, a.osnap, a.otrack, a.infer) = (s.grid, s.ortho, s.polar, s.osnap, s.otrack, s.infer);
        (a.polar_step, a.polar_extra) = (s.polar_step, s.polar_extra.clone());
        self.polar_extra_text = s.polar_extra.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
        (self.snap, self.show_guides, self.show_cmdline, self.measure_hover) = (s.snap, s.show_guides, s.show_cmdline, s.measure_hover);
        (self.ribbon.tab, self.ribbon.collapsed) = (s.ribbon_tab, s.ribbon_collapsed);
        self.theme.choice = s.theme.clone();
        self.ws.set_recent(s.recent.clone());
        self.dxf_ops = s.dxf_ops.clone();
        (self.reopen_files, self.auto_sync) = (s.reopen_files, s.auto_sync);
        self.saved_settings = s;
    }

    /// Save when anything changed since the last save (cheap to compare every frame).
    pub(super) fn save_settings_if_changed(&mut self) {
        let now = self.current_settings();
        if now != self.saved_settings {
            save(&now);
            self.saved_settings = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::theme::Choice;

    #[test]
    fn settings_survive_a_restart() {
        let s = Settings { theme: Choice::File("mine".into()), reopen_files: false, auto_sync: false, polar_step: 7.5, polar_extra: vec![63.0], ..Default::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!(back == s);
        // Settings saved before themes and the Settings dialog existed: the defaults for what is new.
        let old: Settings = serde_json::from_str("{\"grid\": false, \"recent\": [\"/a.scrbr\"]}").unwrap();
        assert!(old.theme == Choice::System && old.reopen_files && old.auto_sync);
        assert!(!old.grid && old.recent == [PathBuf::from("/a.scrbr")]);
    }

    #[test]
    fn the_app_keeps_what_it_is_given() {
        let mut app = App::new(crate::model::Workspace::new(crate::model::Document::default()), Err(String::new()));
        let s = Settings { reopen_files: false, auto_sync: false, show_cmdline: true, polar_extra: vec![7.5], ..Default::default() };
        app.apply_settings(s.clone());
        assert!(app.current_settings() == s && app.polar_extra_text == "7.5");
    }
}
