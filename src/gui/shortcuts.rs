//! One-key shortcuts for the tools: the one table the key handler, the tooltips and the Help >
//! Keyboard shortcuts window all read. Single keys only, no Shift/Ctrl/Alt. They work while no
//! text is typed and the command line is hidden (with it shown, letters go to the command line).
//! A key acts like the tool's button.

use super::App;
use super::command::Cmd;
use eframe::egui::{self, Key, Modifiers, RichText};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Action {
    Run(Cmd),
    Preview,
    ZoomAll,
}

/// The shortcuts. Avoid digits, `.`, `,`, `-`, Tab, Enter, Esc, Backspace (dynamic input), Space
/// and the F keys of the status bar.
// ponytail: provisional mapping; the user may still change letters.
const KEYS: &[(Key, Action)] = &[
    (Key::L, Action::Run(Cmd::Line)),
    (Key::P, Action::Run(Cmd::Pline)),
    (Key::R, Action::Run(Cmd::Rectang)),
    (Key::C, Action::Run(Cmd::Circle)),
    (Key::A, Action::Run(Cmd::Arc)),
    (Key::G, Action::Run(Cmd::Polygon)),
    (Key::T, Action::Run(Cmd::Text)),
    // ponytail: D goes to the smart DIM command once it lands on main.
    (Key::D, Action::Run(Cmd::DimLinear)),
    (Key::I, Action::Run(Cmd::Dist)),
    (Key::M, Action::Run(Cmd::Move)),
    (Key::K, Action::Run(Cmd::Copy)),
    (Key::Q, Action::Run(Cmd::Rotate)),
    (Key::S, Action::Run(Cmd::Scale)),
    (Key::N, Action::Run(Cmd::Mirror)),
    (Key::F, Action::Run(Cmd::Fillet)),
    (Key::H, Action::Run(Cmd::Chamfer)),
    (Key::O, Action::Run(Cmd::Offset)),
    (Key::X, Action::Run(Cmd::Trim)),
    (Key::Y, Action::Run(Cmd::Array)),
    (Key::J, Action::Run(Cmd::Join)),
    (Key::E, Action::Run(Cmd::Explode)),
    (Key::W, Action::Run(Cmd::Stretch)),
    (Key::B, Action::Run(Cmd::Break)),
    (Key::Delete, Action::Run(Cmd::Erase)),
    (Key::V, Action::Preview),
    (Key::Z, Action::ZoomAll),
];

/// Keys that are not tools, for the Help window.
const OTHER: &[(&str, &str)] = &[
    ("Esc", "Cancel the command (back to Select), or clear the selection"),
    ("Enter / Space", "Finish, or repeat the last command"),
    ("Space + drag", "Pan the view"),
    ("Shift+Space", "Cycle through overlapping shapes"),
    ("Arrow keys", "Nudge the selection 1 mm (with Shift 10 mm)"),
    ("Backspace", "Delete the selection"),
    ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
    ("Ctrl+S / Ctrl+Shift+S", "Save / Save as"),
    ("Ctrl+O", "Open"),
    ("Ctrl+D", "Duplicate"),
    ("Ctrl+A", "Select all"),
    ("Ctrl+G / Ctrl+Shift+G", "Combine / Break apart"),
    ("F3, F7–F11", "Snaps and drawing aids (status bar)"),
];

fn text(k: Key) -> &'static str {
    if k == Key::Delete { "Del" } else { k.name() }
}

/// The shortcut of an action, as shown to the user.
pub(super) fn of(action: Action) -> Option<&'static str> {
    KEYS.iter().find(|e| e.1 == action).map(|e| text(e.0))
}

/// The Preview key, for hints.
pub(super) fn preview_key() -> &'static str {
    of(Action::Preview).unwrap_or("")
}

fn title(a: Action) -> &'static str {
    match a {
        Action::Run(c) => c.title(),
        Action::Preview => "Preview what the laser makes",
        Action::ZoomAll => "Show the whole material",
    }
}

impl App {
    /// Run the action of a pressed shortcut. In preview mode only Preview and zoom work.
    pub(super) fn shortcut_keys(&mut self, ctx: &egui::Context, editing: bool) {
        let typing = ctx.text_edit_focused() || self.takes_text() || !self.cmdline.input.is_empty();
        if self.show_cmdline || self.confirm_close || self.library.is_some() || typing || !ctx.input(|i| i.modifiers.is_none()) {
            return;
        }
        let found = KEYS.iter().find(|e| (editing || !matches!(e.1, Action::Run(_))) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, e.0)));
        match found.map(|e| e.1) {
            Some(Action::Run(c)) => self.start(c),
            Some(Action::Preview) => self.toggle_preview(),
            Some(Action::ZoomAll) => self.view.zoom_extents(),
            None => {}
        }
    }

    /// Help > Keyboard shortcuts.
    pub(super) fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_shortcuts;
        egui::Window::new("Keyboard shortcuts").id(self.popouts.id("shortcuts")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new("Tools (when the command line is hidden)").strong());
            egui::Grid::new("shortcut_keys").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                for &(k, a) in KEYS {
                    ui.label(RichText::new(text(k)).monospace());
                    ui.label(title(a));
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Other keys").strong());
            egui::Grid::new("other_keys").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                for (k, what) in OTHER {
                    ui.label(RichText::new(*k).monospace());
                    ui.label(*what);
                    ui.end_row();
                }
            });
        });
        self.show_shortcuts = open;
    }
}

#[cfg(test)]
mod tests {
    use super::KEYS;

    #[test]
    fn no_key_or_action_is_used_twice() {
        for (n, a) in KEYS.iter().enumerate() {
            assert!(KEYS[n + 1..].iter().all(|b| a.0 != b.0 && a.1 != b.1), "{:?} or {:?} is used twice", a.0, a.1);
        }
    }
}
