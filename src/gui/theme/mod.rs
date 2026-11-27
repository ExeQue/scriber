//! Colour themes: the built-in presets, the user's theme files and, on Omarchy, the desktop's
//! current theme, followed live. The active theme is global so painting code anywhere can ask
//! for its colours with `theme::get()`.

pub mod palette;
mod picker;
mod sources;
mod visuals;

use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use parking_lot::RwLock;
use std::sync::LazyLock;

pub use sources::watch;

/// Every colour Scriber paints with; see `palette` for how a theme file becomes one.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Theme {
    pub dark: bool,
    pub panel: Color32,
    /// Cards and stripes on panels.
    pub panel_alt: Color32,
    /// Text fields.
    pub extreme: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub border: Color32,
    pub accent: Color32,
    /// Text and icons on the accent colour.
    pub on_accent: Color32,
    /// Selected text and buttons.
    pub selection: Color32,
    /// The canvas around the material sheet.
    pub canvas: Color32,
    pub material: Color32,
    /// The 10 mm grid; the 1 mm grid is fainter.
    pub grid: Color32,
    pub cursor: Color32,
    pub cut: Color32,
    pub score: Color32,
    pub engrave: Color32,
    pub no_output: Color32,
    /// Selection glow, hover and pick glow, grips, guide lines and dimensions on the canvas.
    pub selected: Color32,
    pub hover: Color32,
    pub grips: Color32,
    pub guide: Color32,
    pub dimension: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub error: Color32,
}

impl Theme {
    pub fn op(&self, op: crate::model::Op) -> Color32 {
        use crate::model::Op;
        match op {
            Op::Cut => self.cut,
            Op::Score => self.score,
            Op::Engrave => self.engrave,
            Op::NoOutput => self.no_output,
        }
    }

    pub fn grid_minor(&self) -> Color32 {
        palette::mix(self.material, self.grid, 0.45)
    }

    pub fn engrave_fill(&self) -> Color32 {
        palette::mix(self.material, self.engrave, 0.75)
    }
}

// One global theme, as Scriber has one window.
static CURRENT: LazyLock<RwLock<Theme>> = LazyLock::new(|| RwLock::new(sources::preset("dark").unwrap().theme()));

/// The active theme.
pub fn get() -> Theme {
    *CURRENT.read()
}

/// The theme setting.
#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// Omarchy's theme on Omarchy, else the system's light or dark.
    #[default]
    System,
    Omarchy,
    /// A built-in theme (`themes/<id>.toml`).
    Preset(String),
    /// `<config>/scriber/themes/<stem>.toml`.
    File(String),
}

#[derive(Default)]
pub struct State {
    pub choice: Choice,
    /// What was applied last: the choice, the file watcher's count and the system's dark mode.
    applied: Option<(Choice, u64, bool)>,
    /// The active theme's name, and why the chosen one could not be used.
    name: String,
    error: Option<String>,
    /// A message from the picker's last action (where a theme was saved).
    note: Option<String>,
    entries: Vec<sources::Entry>,
    omarchy: Option<Result<(String, Theme), String>>,
}

impl State {
    /// Apply the chosen theme when it, its files or the system's dark mode changed.
    pub fn update(&mut self, ctx: &egui::Context) {
        let sys_dark = ctx.system_theme() != Some(egui::Theme::Light);
        let key = (self.choice.clone(), sources::stamp(), sys_dark);
        if self.applied.as_ref() == Some(&key) {
            return;
        }
        self.entries = sources::user_themes();
        self.omarchy = sources::omarchy();
        let (name, theme, error) = self.resolve(sys_dark);
        *CURRENT.write() = theme;
        visuals::apply(ctx, &theme);
        (self.name, self.error, self.applied) = (name, error, Some(key));
    }

    fn resolve(&self, sys_dark: bool) -> (String, Theme, Option<String>) {
        let plain = |dark: bool| {
            let p = sources::preset(if dark { "dark" } else { "light" }).unwrap();
            (p.name.clone().unwrap_or_default(), p.theme())
        };
        let fallback = |dark: bool, error: String| {
            let (name, t) = plain(dark);
            (name, t, Some(error))
        };
        match &self.choice {
            Choice::System if self.omarchy.is_none() => {
                let (name, t) = plain(sys_dark);
                (format!("System ({name})"), t, None)
            }
            Choice::System | Choice::Omarchy => match &self.omarchy {
                Some(Ok((name, t))) => (format!("Omarchy ({name})"), *t, None),
                Some(Err(e)) => fallback(sources::omarchy_says_light().map_or(sys_dark, |light| !light), format!("Omarchy's theme: {e}")),
                None => fallback(sys_dark, "Omarchy was not found".into()),
            },
            Choice::Preset(id) => match sources::preset(id) {
                Some(p) => (p.name.clone().unwrap_or(id.clone()), p.theme(), None),
                None => fallback(sys_dark, format!("No built-in theme \"{id}\"")),
            },
            Choice::File(stem) => match self.entries.iter().find(|e| &e.stem == stem).map(|e| &e.theme) {
                Some(Ok((name, t))) => (name.clone(), *t, None),
                Some(Err(e)) => fallback(sys_dark, format!("{stem}.toml: {e}")),
                None => fallback(sys_dark, format!("{stem}.toml was not found in {}", sources::user_dir().display())),
            },
        }
    }

    /// Read every theme file again.
    pub fn reload(&mut self) {
        self.applied = None;
    }
}

#[cfg(test)]
mod tests {
    use super::palette::contrast;
    use super::*;

    #[test]
    fn presets_parse_and_read_well() {
        for (id, _) in sources::PRESETS {
            let t = sources::preset(id).unwrap_or_else(|| panic!("{id} does not parse")).theme();
            let v = visuals::visuals(&t);
            assert!(contrast(v.text_color(), v.panel_fill) >= 4.5, "{id}: text");
            assert!(contrast(v.widgets.inactive.fg_stroke.color, v.widgets.inactive.bg_fill) >= 4.5, "{id}: buttons");
            assert!(contrast(t.engrave, t.material) >= 4.5, "{id}: engraving");
            assert!(contrast(t.cut, t.material) >= 3.0 && contrast(t.score, t.material) >= 3.0, "{id}: cut and score");
            assert!(contrast(t.on_accent, t.accent) >= 3.0, "{id}: accent");
        }
    }

    #[test]
    fn choice_round_trips_through_json() {
        for c in [Choice::System, Choice::Omarchy, Choice::Preset("paper".into()), Choice::File("mine".into())] {
            assert_eq!(serde_json::from_str::<Choice>(&serde_json::to_string(&c).unwrap()).unwrap(), c);
        }
    }
}
