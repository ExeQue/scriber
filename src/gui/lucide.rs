//! Lucide icons (https://lucide.dev, ISC licence, `assets/fonts/LUCIDE-LICENSE`), from the icon
//! font of `lucide-static` 0.460.0. The codepoints come from that version's `font/info.json`; when
//! the font is updated, look new ones up there (they change between versions).

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, RichText, Ui};
use std::sync::Arc;

pub const EYE: &str = "\u{e0be}";
pub const EYE_OFF: &str = "\u{e0bf}";
pub const LOCK: &str = "\u{e10f}";
pub const LOCK_OPEN: &str = "\u{e110}";
pub const PLUS: &str = "\u{e141}";
pub const CHEVRON_UP: &str = "\u{e074}";
pub const CHEVRON_DOWN: &str = "\u{e071}";
pub const X: &str = "\u{e1b2}";
pub const PENCIL: &str = "\u{e1f9}";
pub const COPY: &str = "\u{e0a2}";
pub const PRINTER: &str = "\u{e145}";
pub const ZAP: &str = "\u{e1b4}";
pub const LAYERS: &str = "\u{e104}";
pub const MAXIMIZE: &str = "\u{e116}";
pub const CIRCLE_CHECK: &str = "\u{e226}";
pub const CIRCLE_ALERT: &str = "\u{e07b}";

/// Add the icon font after the default fonts, so its private-use codepoints work in any text.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("lucide".into(), Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/lucide.ttf"))));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("lucide".into());
    }
    ctx.set_fonts(fonts);
}

/// A flat icon button; `on` draws it highlighted (a toggle).
pub fn button(ui: &mut Ui, icon: &str, on: bool, tip: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(icon).size(15.0)).frame(on).min_size(egui::vec2(22.0, 22.0))).on_hover_text(tip)
}

/// A red ✕ for deleting, as in the library dialog.
pub fn delete(ui: &mut Ui, enabled: bool, tip: &str) -> egui::Response {
    let red = egui::Color32::from_rgb(220, 70, 70);
    let text = RichText::new(X).size(15.0).color(if enabled { red } else { red.gamma_multiply(0.35) });
    ui.add_enabled(enabled, egui::Button::new(text).frame(false).min_size(egui::vec2(22.0, 22.0))).on_hover_text(tip).on_disabled_hover_text(tip)
}
