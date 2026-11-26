//! ZOOM as in AutoCAD: zoom window, zoom previous (a short view history), zoom to the selection,
//! and Space + left drag to pan. Wheel zoom, middle drag and wheel double-click live in canvas.rs.

use super::{App, View};
use crate::model::BBox;
use eframe::egui::{self, Key, PointerButton, vec2};

/// How many earlier views ZOOM Previous can go back.
const HISTORY: usize = 30;
/// Room left around a zoomed box, in pixels.
const MARGIN: f32 = 30.0;

impl View {
    /// Keep the current view for ZOOM Previous.
    pub(super) fn remember(&mut self) {
        let now = (self.offset, self.zoom);
        if self.history.last() != Some(&now) {
            self.history.push(now);
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
        }
    }
}

impl App {
    /// Show the box `b` (mm) as large as fits in the canvas.
    pub(super) fn zoom_to(&mut self, b: BBox) {
        let rect = self.canvas_rect;
        let size = rect.size() - vec2(2.0 * MARGIN, 2.0 * MARGIN);
        // A point or a line still gets a sensible zoom.
        let (w, h) = (b[2].max(1.0) as f32, b[3].max(1.0) as f32);
        self.view.remember();
        self.view.auto_fit = false;
        self.view.zoom = (size.x / w).min(size.y / h).clamp(0.05, 400.0);
        let centre = vec2((b[0] + b[2] / 2.0) as f32, (b[1] + b[3] / 2.0) as f32);
        self.view.offset = rect.size() / 2.0 - centre * self.view.zoom;
    }

    /// ZOOM Previous: back to the view before the last zoom or pan. False when there is none.
    pub(super) fn zoom_previous(&mut self) -> bool {
        let Some((offset, zoom)) = self.view.history.pop() else { return false };
        self.view.offset = offset;
        self.view.zoom = zoom;
        self.view.auto_fit = false;
        true
    }

    /// ZOOM Selection (or ZOOM Object): the selected items fill the view.
    pub(super) fn zoom_selection(&mut self) -> bool {
        let b = {
            let d = self.doc.lock().unwrap();
            d.bbox(&d.selection)
        };
        b.map(|b| self.zoom_to(b)).is_some()
    }

    /// Space held with a left drag pans (unless Space types into a text field). Returns true while
    /// Space is held, so the drag does not also select or draw.
    pub(super) fn space_pan(&mut self, ui: &egui::Ui, response: &egui::Response) -> bool {
        let typing = self.takes_text() || (ui.ctx().text_edit_focused() && ui.ctx().memory(|m| m.focused()) != Some(super::command::input_id()));
        let held = !typing && ui.input(|i| i.key_down(Key::Space));
        if held && response.drag_started_by(PointerButton::Primary) {
            self.view.remember();
        }
        if held && response.dragged_by(PointerButton::Primary) {
            self.view.offset += response.drag_delta();
            self.view.auto_fit = false;
            self.space_panned = true;
        }
        held
    }
}
