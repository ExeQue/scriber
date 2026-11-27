//! Edit → Settings… (Ctrl+,): a centred modal with the sections on the left and their settings on
//! the right. Every change applies at once and is saved with the other settings (see settings.rs);
//! Close (or Esc) only closes the dialog.

use super::App;
use super::library_window::{form_column, list_row};
use eframe::egui::{self, RichText};

mod sections;

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub(super) enum Section {
    #[default]
    Appearance,
    DrawingAids,
    Files,
    Library,
    Agents,
}

impl Section {
    const ALL: [Section; 5] = [Section::Appearance, Section::DrawingAids, Section::Files, Section::Library, Section::Agents];

    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::DrawingAids => "Drawing aids",
            Section::Files => "Files",
            Section::Library => "Library",
            Section::Agents => "AI agents",
        }
    }
}

const SIDEBAR_WIDTH: f32 = 180.0;

impl App {
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(mut section) = self.settings else { return };
        let mut close = false;
        // A fixed size that still fits small screens, so the layout does not jump between sections.
        let screen = ctx.content_rect().size();
        let size = egui::vec2((screen.x - 80.0).clamp(560.0, 820.0), (screen.y - 220.0).clamp(300.0, 520.0));
        let modal = egui::Modal::new(egui::Id::unique("settings-modal")).show(ctx, |ui| {
            ui.set_width(size.x);
            ui.heading("Settings");
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.set_min_size(size);
                ui.allocate_ui_with_layout(egui::vec2(SIDEBAR_WIDTH, size.y), egui::Layout::top_down(egui::Align::Min), |ui| {
                    egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(6).inner_margin(6).show(ui, |ui| {
                        ui.set_width(SIDEBAR_WIDTH - 12.0);
                        ui.set_height(size.y - 12.0);
                        for s in Section::ALL {
                            if list_row(ui, section == s, s.label()) {
                                section = s;
                            }
                        }
                    });
                });
                form_column(ui, "settings-form", |ui| match section {
                    Section::Appearance => self.appearance_section(ui),
                    Section::DrawingAids => self.aids_section(ui),
                    Section::Files => self.files_section(ui),
                    Section::Library => close |= self.library_section(ui),
                    Section::Agents => close |= self.agents_section(ui),
                });
            });
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Changes apply at once and are kept for next time.").weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close |= ui.add(egui::Button::new("Close").min_size(egui::vec2(90.0, 28.0))).clicked();
                });
            });
        });
        // A button that opened another window closed the dialog already.
        if self.settings.is_some() {
            self.settings = (!close && !modal.should_close()).then_some(section);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, Workspace};

    /// Every section on a large and a small screen: its title and Close are visible, and no text
    /// runs past the dialog's right edge.
    #[test]
    fn every_section_fits() {
        let ctx = egui::Context::default();
        crate::gui::lucide::install(&ctx);
        let mut app = App::new(Workspace::new(Document::default()), Ok(7341));
        app.ws.set_recent(vec!["/nowhere/a rather long project name for a box.scrbr".into(), "/nowhere/b.scrbr".into()]);
        for size in [egui::vec2(1600.0, 900.0), egui::vec2(800.0, 600.0)] {
            for (s, title) in Section::ALL.into_iter().zip(["Theme", "Snaps and tracking", "At start", "Library sources", "MCP server"]) {
                app.settings = Some(s);
                let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
                let mut out = None;
                // The first frames are sizing passes; the third is what the user sees.
                for _ in 0..3 {
                    let mut o = ctx.run_pass(input(), |ctx| app.settings_window(ctx));
                    o.textures_delta.clear();
                    out = Some(o);
                }
                let mut texts = Vec::new();
                for clipped in out.unwrap().shapes {
                    if let egui::Shape::Text(t) = &clipped.shape {
                        let rect = t.visual_bounding_rect();
                        texts.push((t.galley.text().to_string(), rect, clipped.clip_rect.contains_rect(rect)));
                    }
                }
                let what = format!("{size:?}, {s:?}");
                let find = |text: &str| texts.iter().find(|t| t.0 == text).unwrap_or_else(|| panic!("{what}: \"{text}\" not painted: {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>()));
                let (heading, close, side) = (find(title), find("Close"), find(s.label()));
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for x in [heading, close, side] {
                    assert!(x.2 && screen.contains_rect(x.1), "{what}: \"{}\" is hidden at {:?}", x.0, x.1);
                }
                assert!(heading.1.left() > side.1.left() + 150.0, "{what}: the form must be right of the sidebar");
                for x in texts.iter().filter(|x| x.2) {
                    assert!(x.1.right() <= close.1.right() + 12.0, "{what}: \"{}\" runs past the dialog: {:?}", x.0, x.1);
                }
                assert!(app.settings == Some(s), "{what}: the dialog stays open");
            }
        }
    }
}
