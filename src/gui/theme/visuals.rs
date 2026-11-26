//! A theme as egui's look: panels, windows, buttons, text fields, selection and links.

use super::Theme;
use super::palette::{mix, readable};
use eframe::egui::{self, Color32, Visuals};

pub fn visuals(t: &Theme) -> Visuals {
    let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
    let strong = mix(t.text, if t.dark { Color32::WHITE } else { Color32::BLACK }, 0.5);
    v.weak_text_color = Some(t.text_weak);
    v.hyperlink_color = readable(t.accent, t.panel, 3.0);
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.window_stroke.color = t.border;
    v.faint_bg_color = t.panel_alt;
    v.code_bg_color = t.panel_alt;
    v.extreme_bg_color = t.extreme;
    v.text_edit_bg_color = Some(t.extreme);
    v.warn_fg_color = t.warn;
    v.error_fg_color = t.error;
    v.selection.bg_fill = t.selection;
    v.selection.stroke.color = readable(t.text, t.selection, 4.5);
    v.text_cursor.stroke.color = t.accent;
    let w = &mut v.widgets;
    for (wv, lift, fg) in [(&mut w.noninteractive, 0.0, t.text), (&mut w.inactive, 0.10, t.text), (&mut w.hovered, 0.18, strong), (&mut w.active, 0.26, strong), (&mut w.open, 0.14, strong)] {
        let fill = mix(t.panel, t.text, lift);
        (wv.bg_fill, wv.weak_bg_fill) = (fill, fill);
        wv.fg_stroke.color = readable(fg, fill, 4.5);
    }
    w.noninteractive.bg_fill = t.panel;
    w.noninteractive.bg_stroke.color = t.border;
    w.hovered.bg_stroke.color = mix(t.border, t.accent, 0.5);
    w.active.bg_stroke.color = t.accent;
    v
}

pub fn apply(ctx: &egui::Context, t: &Theme) {
    let theme = if t.dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, visuals(t));
}
