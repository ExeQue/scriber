//! Copy, cut and paste through the system clipboard (so two Scriber windows can share shapes),
//! with the last copy kept in the app when the system clipboard gives nothing back. Copied
//! shapes go as a JSON payload (`model::clipboard`); SVG text on the clipboard is imported.

use super::App;
use super::tools::Tool;
use crate::export::svg_import::ImportOptions;
use crate::model::clipboard::Clip;
use crate::model::{Point, Transform};
use eframe::egui::{self, Event};

/// Paste without a pointer on the canvas: this far right and down of the originals, in mm.
const OFFSET: f64 = 10.0;

/// A paste asked for from a menu or Ctrl+Shift+V, waiting for the clipboard's text.
pub(super) struct PasteRequest {
    at: Option<Point>,
    in_place: bool,
    frame: u64,
}

fn is_svg(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<svg") || t.starts_with("<?xml")
}

impl App {
    pub(super) fn copy_selection(&mut self, ctx: &egui::Context) -> bool {
        let d = self.doc.lock();
        let Some(clip) = d.copy(&d.selection) else { return false };
        drop(d);
        let json = clip.to_json();
        ctx.copy_text(json.clone());
        self.clip = Some(json);
        self.message = format!("Copied {} item(s).", clip.items.len());
        true
    }

    /// Copy, then delete (one undo step).
    pub(super) fn cut_selection(&mut self, ctx: &egui::Context) {
        if self.copy_selection(ctx) {
            self.delete_selection();
        }
    }

    /// Paste centred at `at` (None: offset from the originals) or in place. The clipboard's text
    /// arrives as a paste event in a later frame (`clipboard_events`).
    pub(super) fn request_paste(&mut self, ctx: &egui::Context, at: Option<Point>, in_place: bool) {
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        ctx.request_repaint();
        self.paste_request = Some(PasteRequest { at, in_place, frame: ctx.cumulative_frame_nr() });
    }

    /// Ctrl+C, Ctrl+X and Ctrl+V (egui turns them into events), and requested pastes. Text that is
    /// neither shapes nor SVG is left for the command line.
    pub(super) fn clipboard_events(&mut self, ctx: &egui::Context, pointer: Option<Point>) {
        if !self.cmdline.input.is_empty() {
            return;
        }
        let (mut copy, mut cut, mut pasted, mut other) = (false, false, None, false);
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Copy => !std::mem::replace(&mut copy, true),
                Event::Cut => !std::mem::replace(&mut cut, true),
                Event::Paste(t) if pasted.is_none() && (is_svg(t) || Clip::from_json(t).is_some()) => {
                    pasted = Some(t.clone());
                    false
                }
                Event::Paste(_) => {
                    other = true;
                    true
                }
                _ => true,
            })
        });
        if copy {
            self.copy_selection(ctx);
        }
        if cut {
            self.cut_selection(ctx);
        }
        let req = self.paste_request.take();
        let (at, in_place) = req.as_ref().map_or((pointer, false), |r| (r.at, r.in_place));
        match (pasted, req) {
            (Some(text), _) => self.paste_text(&text, at, in_place),
            (None, Some(_)) if other => self.message = "The clipboard holds no shapes.".into(),
            // Nothing from the system clipboard yet: wait a frame or two, then use the app's own copy.
            (None, Some(r)) if ctx.cumulative_frame_nr() <= r.frame + 2 => {
                ctx.request_repaint();
                self.paste_request = Some(r);
            }
            (None, Some(_)) => match self.clip.clone() {
                Some(text) => self.paste_text(&text, at, in_place),
                None => self.message = "The clipboard holds no shapes.".into(),
            },
            (None, None) => {}
        }
    }

    /// Paste copied shapes or import SVG text, centred at `at` when given (one undo step).
    fn paste_text(&mut self, text: &str, at: Option<Point>, in_place: bool) {
        let at = at.filter(|_| !in_place);
        self.set_tool(Tool::Select);
        let mut d = self.doc.lock();
        let before = d.s.clone();
        let result = if is_svg(text) {
            d.import_svg(text.as_bytes(), &ImportOptions::default()).map(|ids| {
                if let (Some(p), Some([x, y, w, h])) = (at, d.bbox(&ids)) {
                    d.transform(&ids, &Transform::translate(p[0] - x - w / 2.0, p[1] - y - h / 2.0));
                }
                ids
            })
        } else {
            Clip::from_json(text).ok_or_else(|| "the clipboard holds no shapes".to_string()).map(|clip| {
                let [dx, dy] = match (at, clip.bbox()) {
                    _ if in_place => [0.0, 0.0],
                    (Some(p), Some([x, y, w, h])) => [p[0] - x - w / 2.0, p[1] - y - h / 2.0],
                    _ => [OFFSET, OFFSET],
                };
                d.paste(&clip, dx, dy)
            })
        };
        match result {
            Ok(ids) if !ids.is_empty() => {
                d.push_undo(before);
                d.selection = ids.clone();
                drop(d);
                self.message = format!("Pasted {} item(s).", ids.len());
            }
            Ok(_) => self.message = "Nothing to paste.".into(),
            Err(e) => {
                d.s = before;
                drop(d);
                self.message = format!("Could not paste: {e}");
            }
        }
    }
}
