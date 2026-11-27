//! Where themes come from: the presets built in, the user's theme folder and Omarchy's current
//! theme, plus a watcher that notices when any of them change.

use super::Theme;
use super::palette::{self, Palette};
use eframe::egui;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The built-in themes, in menu order; the same format as a user's theme file.
pub const PRESETS: &[(&str, &str)] = &[
    ("dark", include_str!("../../../themes/dark.toml")),
    ("light", include_str!("../../../themes/light.toml")),
    ("graphite", include_str!("../../../themes/graphite.toml")),
    ("paper", include_str!("../../../themes/paper.toml")),
    ("blueprint", include_str!("../../../themes/blueprint.toml")),
    ("high-contrast", include_str!("../../../themes/high-contrast.toml")),
];

pub fn preset(id: &str) -> Option<Palette> {
    PRESETS.iter().find(|(i, _)| *i == id).and_then(|(_, src)| palette::parse(src).ok())
}

/// `<config>/scriber/themes`.
pub fn user_dir() -> PathBuf {
    crate::library::sources::config_dir().join("themes")
}

/// A theme file: its name, or why it cannot be used.
pub fn load(path: &Path) -> Result<(String, Theme), String> {
    let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let p = palette::parse(&src)?;
    let name = p.name.clone().unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
    Ok((name, p.theme()))
}

pub struct Entry {
    pub stem: String,
    pub theme: Result<(String, Theme), String>,
}

/// The user's theme files, by file name.
pub fn user_themes() -> Vec<Entry> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(user_dir()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
    files.sort();
    files.iter().map(|f| Entry { stem: f.file_stem().unwrap_or_default().to_string_lossy().into_owned(), theme: load(f) }).collect()
}

fn omarchy_dir() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    dirs::home_dir().map(|h| h.join(".local/state/omarchy/current"))
}

/// Omarchy's current theme (None when this is not Omarchy): the theme's own `scriber.toml` when it
/// has one, else its `colors.toml`.
pub fn omarchy() -> Option<Result<(String, Theme), String>> {
    let dir = omarchy_dir()?;
    let theme = dir.join("theme");
    if !theme.is_dir() {
        return None;
    }
    let name = std::fs::read_to_string(dir.join("theme.name")).map(|s| s.trim().to_string()).unwrap_or_default();
    let file = ["scriber.toml", "colors.toml"].map(|f| theme.join(f)).into_iter().find(|f| f.is_file());
    let Some(file) = file else { return Some(Err("the theme has no colors.toml".into())) };
    Some(load(&file).map(|(n, t)| (if name.is_empty() { n } else { name }, t)).map_err(|e| format!("{}: {e}", file.display())))
}

/// Whether a broken `colors.toml` still says `mode = "light"`.
pub fn omarchy_says_light() -> Option<bool> {
    let src = std::fs::read_to_string(omarchy_dir()?.join("theme/colors.toml")).ok()?;
    let mode = src.lines().find(|l| l.trim_start().starts_with("mode"))?;
    Some(mode.contains("light"))
}

static STAMP: AtomicU64 = AtomicU64::new(0);

/// Goes up whenever a theme file changed.
pub fn stamp() -> u64 {
    STAMP.load(Ordering::Relaxed)
}

/// The files that make up the themes, and when each changed.
fn fingerprint() -> u64 {
    let mut files: Vec<PathBuf> = std::fs::read_dir(user_dir()).into_iter().flatten().flatten().map(|e| e.path()).collect();
    if let Some(d) = omarchy_dir() {
        files.extend(["theme.generation", "theme.name", "theme/colors.toml", "theme/scriber.toml"].map(|f| d.join(f)));
    }
    let mut h = DefaultHasher::new();
    for f in files {
        (&f, std::fs::metadata(&f).and_then(|m| m.modified()).ok()).hash(&mut h);
    }
    h.finish()
}

/// Look at the theme files every two seconds and repaint when one changed, so switching the
/// Omarchy theme or saving a theme file shows at once.
pub fn watch(ctx: egui::Context) {
    // Polls the files' mtimes every 2 s instead of using a file-system watcher.
    std::thread::spawn(move || {
        let mut last = fingerprint();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let now = fingerprint();
            if now != last {
                last = now;
                STAMP.fetch_add(1, Ordering::Relaxed);
                ctx.request_repaint();
            }
        }
    });
}

/// Write `t` as a new theme file named after `name`; returns its stem and path.
pub fn save(name: &str, t: &Theme) -> std::io::Result<(String, PathBuf)> {
    let dir = user_dir();
    std::fs::create_dir_all(&dir)?;
    let slug: String = name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let slug = slug.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    let stem = (1..).map(|n| if n == 1 { format!("{slug}-copy") } else { format!("{slug}-copy-{n}") }).find(|s| !dir.join(format!("{s}.toml")).exists()).unwrap();
    let path = dir.join(format!("{stem}.toml"));
    std::fs::write(&path, palette::to_toml(&format!("{name} (copy)"), t))?;
    STAMP.fetch_add(1, Ordering::Relaxed);
    Ok((stem, path))
}

/// Show the theme folder in the file manager.
pub fn open_folder() -> std::io::Result<()> {
    let dir = user_dir();
    std::fs::create_dir_all(&dir)?;
    let opener = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(opener).arg(dir).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_theme_files_load_with_derived_keys_and_errors() {
        let dir = user_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mine.toml"), "name = \"Mine\"\nbackground = \"#101820\"\naccent = \"#ff8800\"\n").unwrap();
        std::fs::write(dir.join("broken.toml"), "background = \"blue\"\n").unwrap();
        let themes = user_themes();
        let get = |stem: &str| &themes.iter().find(|e| e.stem == stem).unwrap().theme;
        let (name, t) = get("mine").as_ref().unwrap();
        assert_eq!(name, "Mine");
        assert!(t.dark && super::super::palette::contrast(t.text, t.panel) >= 4.5);
        assert!(get("broken").as_ref().unwrap_err().contains("background"));
        let (stem, path) = save("Mine", t).unwrap();
        assert_eq!(stem, "mine-copy");
        assert_eq!(load(&path).unwrap().1, *t);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
