//! Theme files: a flat TOML of `#rrggbb` colours, the same keys as Omarchy's `colors.toml` plus a
//! few of Scriber's own. Whatever a file leaves out is derived from the rest.

use super::Theme;
use eframe::egui::Color32;
use std::collections::HashMap;

/// Every key a theme file may set; any other key is ignored (Omarchy files carry more).
pub const KEYS: &[&str] = &[
    "background", "foreground", "accent", "selection", "muted", "lighter_background", "dark_background", "darker_background",
    "red", "yellow", "green", "canvas", "material", "grid", "cut", "score", "engrave", "no_output", "selected", "hover", "grips",
    "guide", "dimension",
];

#[derive(Debug)]
pub struct Palette {
    pub name: Option<String>,
    dark: Option<bool>,
    colours: HashMap<String, Color32>,
}

pub fn parse(src: &str) -> Result<Palette, String> {
    let doc: toml_edit::DocumentMut = src.parse().map_err(|e: toml_edit::TomlError| e.message().to_string())?;
    let mut p = Palette { name: None, dark: None, colours: HashMap::new() };
    for (key, item) in doc.iter() {
        let value = item.as_str();
        match key {
            "name" => p.name = value.map(str::to_string),
            "mode" => {
                p.dark = Some(match value {
                    Some("dark") => true,
                    Some("light") => false,
                    _ => return Err("mode must be \"dark\" or \"light\"".into()),
                })
            }
            k if KEYS.contains(&k) => {
                let c = value.and_then(hex).ok_or_else(|| format!("{k}: {} is not a colour like \"#1e1e2e\"", item.to_string().trim()))?;
                p.colours.insert(k.to_string(), c);
            }
            _ => {}
        }
    }
    if !p.colours.contains_key("background") {
        return Err("no background colour".into());
    }
    Ok(p)
}

fn hex(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 && h.len() != 8 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?))
}

fn to_hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// Relative luminance (WCAG).
fn lum(c: Color32) -> f32 {
    let ch = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

/// WCAG contrast ratio, 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (lum(a), lum(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `a` moved `t` (0 to 1) of the way to `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn dark_bg(bg: Color32) -> bool {
    contrast(Color32::WHITE, bg) > contrast(Color32::BLACK, bg)
}

/// `c`, lightened or darkened just enough to reach `ratio` against `bg`.
pub fn readable(c: Color32, bg: Color32, ratio: f32) -> Color32 {
    let target = if dark_bg(bg) { Color32::WHITE } else { Color32::BLACK };
    (0..=20).map(|i| mix(c, target, i as f32 / 20.0)).find(|m| contrast(*m, bg) >= ratio).unwrap_or(target)
}

impl Palette {
    pub fn theme(&self) -> Theme {
        let c = |k: &str| self.colours.get(k).copied();
        let rgb = Color32::from_rgb;
        let gray = Color32::from_gray;
        let (white, black) = (Color32::WHITE, Color32::BLACK);
        let bg = c("background").unwrap_or(gray(27));
        let dark = self.dark.unwrap_or_else(|| dark_bg(bg));
        let text = readable(c("foreground").unwrap_or(if dark { gray(220) } else { gray(30) }), bg, 4.5);
        let accent = c("accent").unwrap_or(if dark { rgb(60, 155, 215) } else { rgb(0, 110, 160) });
        let panel_alt = c("lighter_background").unwrap_or(mix(bg, text, 0.06));
        let material = c("material").unwrap_or(if dark { panel_alt } else { mix(bg, white, 0.8) });
        let engrave = c("engrave").unwrap_or_else(|| readable(if dark_bg(material) { gray(220) } else { black }, material, 4.5));
        let on_material = |k: &str, base: Color32, ratio: f32| c(k).unwrap_or_else(|| readable(base, material, ratio));
        Theme {
            dark,
            panel: bg,
            panel_alt,
            extreme: c("dark_background").unwrap_or(if dark { mix(bg, black, 0.35) } else { mix(bg, white, 0.7) }),
            text,
            text_weak: readable(c("muted").unwrap_or(mix(text, bg, 0.4)), bg, 3.0),
            border: mix(bg, text, 0.2),
            accent,
            on_accent: if contrast(white, accent) >= contrast(black, accent) { white } else { black },
            selection: c("selection").unwrap_or(mix(bg, accent, 0.35)),
            canvas: c("canvas").or(c("darker_background")).unwrap_or(mix(bg, black, if dark { 0.45 } else { 0.18 })),
            material,
            grid: c("grid").unwrap_or(mix(material, engrave, 0.13)),
            cursor: readable(gray(150), material, 2.5),
            cut: on_material("cut", rgb(255, 0, 0), 3.0),
            score: on_material("score", rgb(0, 0, 255), 3.0),
            engrave,
            no_output: on_material("no_output", gray(150), 2.0),
            selected: on_material("selected", rgb(255, 150, 0), 1.8),
            hover: on_material("hover", rgb(90, 170, 255), 1.8),
            grips: on_material("grips", rgb(40, 110, 255), 2.5),
            guide: on_material("guide", rgb(0, 170, 200), 2.0),
            dimension: on_material("dimension", rgb(40, 110, 220), 3.0),
            ok: readable(c("green").unwrap_or(rgb(60, 170, 80)), bg, 3.0),
            warn: readable(c("yellow").unwrap_or(rgb(220, 150, 30)), bg, 3.0),
            error: readable(c("red").unwrap_or(rgb(220, 70, 60)), bg, 3.0),
        }
    }
}

/// A theme file that gives exactly `t`, to start a theme of one's own from.
pub fn to_toml(name: &str, t: &Theme) -> String {
    let keys = [
        ("background", t.panel), ("foreground", t.text), ("muted", t.text_weak), ("accent", t.accent), ("selection", t.selection),
        ("lighter_background", t.panel_alt), ("dark_background", t.extreme), ("red", t.error), ("yellow", t.warn), ("green", t.ok),
        ("canvas", t.canvas), ("material", t.material), ("grid", t.grid), ("cut", t.cut), ("score", t.score), ("engrave", t.engrave),
        ("no_output", t.no_output), ("selected", t.selected), ("hover", t.hover), ("grips", t.grips), ("guide", t.guide),
        ("dimension", t.dimension),
    ];
    let mut s = format!("name = {:?}\nmode = \"{}\"\n\n", name, if t.dark { "dark" } else { "light" });
    for (k, c) in keys {
        s += &format!("{k} = \"{}\"\n", to_hex(c));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const OMARCHY: &str = r##"# A theme
mode = "dark"
accent = "#c9a66b"           # bronze
selection = "#3f3a2e"
muted = "#7d7462"
background = "#15130f"       # rubber
dark_background = "#0e0d0a"
darker_background = "#080706"
lighter_background = "#221f19"
foreground = "#d7d8dc"
red = "#c8553d"
yellow = "#d9b85c"
green = "#94a35a"
blue = "#c9a66b"
bright_red = "#e0735b"
"##;

    #[test]
    fn parses_omarchy_colors_with_comments() {
        let p = parse(OMARCHY).unwrap();
        let t = p.theme();
        assert!(t.dark);
        assert_eq!(t.panel, Color32::from_rgb(0x15, 0x13, 0x0f));
        assert_eq!(t.accent, Color32::from_rgb(0xc9, 0xa6, 0x6b));
        assert_eq!(t.canvas, Color32::from_rgb(0x08, 0x07, 0x06));
        assert_eq!(t.material, Color32::from_rgb(0x22, 0x1f, 0x19));
        assert!(contrast(t.text, t.panel) >= 4.5);
        // Engraving is light on the dark sheet, and the laser colours stay visible on it.
        assert!(contrast(t.engrave, t.material) >= 4.5 && t.engrave.r() > 150);
        assert!(contrast(t.score, t.material) >= 3.0 && t.score.b() > t.score.r());
        assert!(contrast(t.cut, t.material) >= 3.0 && t.cut.r() > t.cut.g());
    }

    #[test]
    fn missing_keys_are_derived() {
        let t = parse("background = \"#fafafa\"").unwrap().theme();
        assert!(!t.dark, "mode follows the background");
        assert!(contrast(t.text, t.panel) >= 4.5);
        assert!(contrast(t.engrave, t.material) >= 4.5);
        assert_eq!(t.cut, Color32::from_rgb(255, 0, 0), "pure red already reads on a light sheet");
    }

    #[test]
    fn broken_files_say_why() {
        assert!(parse("background = ").is_err());
        assert!(parse("foreground = \"#ffffff\"").unwrap_err().contains("background"));
        assert!(parse("background = \"#12345\"").unwrap_err().contains("background"));
        assert!(parse("background = \"#000000\"\nmode = \"dim\"").unwrap_err().contains("mode"));
        assert!(parse("").is_err());
    }

    #[test]
    fn a_saved_theme_reads_back_the_same() {
        let t = parse(OMARCHY).unwrap().theme();
        let back = parse(&to_toml("Mine", &t)).unwrap();
        assert_eq!(back.name.as_deref(), Some("Mine"));
        assert!(back.theme() == t);
    }
}
