//! Review comments on the canvas (see `model::annotations`): the Comment tool pins one on a spot
//! or a shape, pins show as numbered speech bubbles (open in the accent colour, resolved grey),
//! hovering one shows its text and clicking it opens the thread (replies, Resolve/Reopen, Delete).
//! The right panel lists them; agents read and answer them over MCP.

use super::App;
use super::canvas::tooltip;
use super::icons::ACCENT;
use super::lucide;
use crate::model::Point;
use crate::model::annotations::{Annotation, Author, Status};
use eframe::egui::{self, Align2, Color32, FontId, Key, PointerButton, Rect, RichText, Stroke, vec2};

#[derive(Default)]
pub(super) struct Comments {
    /// The Comment tool is on: a click pins a new comment.
    pub placing: bool,
    /// The pins are hidden (View).
    pub hidden: bool,
    draft: Option<Draft>,
    /// The comment whose thread is open.
    open: Option<u64>,
    reply: String,
    /// Where each pin was drawn last frame, and the one under the pointer.
    pins: Vec<(u64, Rect)>,
    hot: Option<u64>,
}

/// A comment being written.
struct Draft {
    at: Point,
    items: Vec<u64>,
    text: String,
    focus: bool,
}

const PIN: egui::Vec2 = vec2(22.0, 17.0);
const RESOLVED: Color32 = Color32::from_gray(140);

fn who(a: &Author) -> &str {
    match a {
        Author::Person => "You",
        Author::Agent(n) => n,
    }
}

impl App {
    /// Comment tool on/off (ribbon). Cancels a running command.
    pub(super) fn toggle_comment_tool(&mut self) {
        let c = &mut self.comments;
        c.placing = !c.placing;
        if c.placing {
            c.hidden = false;
            self.escape();
            self.message = "Click a spot or a shape to comment on it (Esc stops).".into();
        }
    }

    /// Canvas input for comments: placing a new one, or a click on a pin. Returns true when it
    /// took the pointer, so selecting and drawing leave it alone.
    pub(super) fn comment_input(&mut self, ui: &egui::Ui, response: &egui::Response, raw: Option<Point>) -> bool {
        if self.active_cmd().is_some() {
            self.comments.placing = false;
            self.comments.hot = None;
            return false;
        }
        let pos = response.hover_pos();
        self.comments.hot = if self.comments.hidden { None } else { pos.and_then(|p| self.comments.pins.iter().rev().find(|(_, r)| r.contains(p)).map(|(id, _)| *id)) };
        if let Some(id) = self.comments.hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if response.clicked_by(PointerButton::Primary) {
                self.comments.open = Some(id);
                self.comments.reply.clear();
            }
            return true;
        }
        if !self.comments.placing {
            return false;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.comments.placing = false;
            return true;
        }
        if response.clicked_by(PointerButton::Primary)
            && let Some(at) = raw
        {
            let items = {
                let d = self.doc.lock().unwrap();
                self.hit(&d, at).map(|id| d.expand_groups(&[id])).unwrap_or_default()
            };
            self.start_draft(at, items);
        }
        true
    }

    fn start_draft(&mut self, at: Point, items: Vec<u64>) {
        let text = self.comments.draft.take().map(|d| d.text).unwrap_or_default();
        self.comments.draft = Some(Draft { at, items, text, focus: true });
        self.comments.hidden = false;
    }

    /// "Comment on this…" in the right-click menu, for the selection.
    pub(super) fn comment_menu(&mut self, ui: &mut egui::Ui) {
        let (sel, centre) = {
            let d = self.doc.lock().unwrap();
            (d.selection.clone(), d.bbox(&d.selection).map(|[x, y, w, h]| [x + w / 2.0, y + h / 2.0]))
        };
        if let Some(at) = centre
            && ui.button(format!("{} Comment on this…", lucide::MESSAGE_SQUARE)).clicked()
        {
            self.start_draft(at, sel);
        }
    }

    /// The pins, numbered, on top of the drawing.
    pub(super) fn paint_comments(&mut self, painter: &egui::Painter) {
        self.comments.pins.clear();
        if self.comments.hidden {
            return;
        }
        let list = self.doc.lock().unwrap().annotations.list.clone();
        let font = FontId::proportional(11.0);
        let draft = self.comments.draft.as_ref().map(|d| (0, d.at, Status::Open));
        for (id, at, status) in list.iter().map(|a| (a.id, a.at, a.status)).chain(draft) {
            let tip = self.w2s(at);
            let body = Rect::from_min_size(tip + vec2(-4.0, -PIN.y - 6.0), PIN);
            let fill = if status == Status::Open { ACCENT } else { RESOLVED };
            let hot = self.comments.hot == Some(id) || self.comments.open == Some(id);
            let stroke = Stroke::new(if hot { 2.0 } else { 1.0 }, Color32::WHITE);
            painter.add(egui::Shape::convex_polygon(vec![tip, body.left_bottom(), body.left_bottom() + vec2(9.0, 0.0)], fill, Stroke::NONE));
            painter.rect(body, 4.0, fill, stroke, egui::StrokeKind::Outside);
            painter.text(body.center(), Align2::CENTER_CENTER, if id == 0 { "+".into() } else { id.to_string() }, font.clone(), Color32::WHITE);
            if id != 0 {
                self.comments.pins.push((id, body.union(Rect::from_two_pos(tip, tip))));
            }
        }
        if let Some(a) = self.comments.hot.and_then(|id| list.iter().find(|a| a.id == id))
            && self.comments.open != Some(a.id)
        {
            let pos = self.comments.pins.iter().find(|(id, _)| *id == a.id).map(|(_, r)| r.right_top()).unwrap_or_default();
            let mut text = format!("{}: {}", who(&a.author), a.text);
            if let Some(r) = a.replies.last() {
                text += &format!("\n{}: {}", who(&r.author), r.text);
            }
            tooltip(painter, pos, &text);
        }
    }

    /// The new-comment box and the open thread.
    pub(super) fn comment_windows(&mut self, ctx: &egui::Context) {
        self.draft_window(ctx);
        self.thread_window(ctx);
    }

    fn draft_window(&mut self, ctx: &egui::Context) {
        let Some(at) = self.comments.draft.as_ref().map(|d| d.at) else { return };
        let (mut post, mut cancel) = (false, false);
        let mut open = true;
        egui::Window::new("New comment").open(&mut open).collapsible(false).resizable(false).default_pos(self.w2s(at) + vec2(24.0, -10.0)).show(ctx, |ui| {
            let d = self.comments.draft.as_mut().unwrap();
            if !d.items.is_empty() {
                ui.label(RichText::new(format!("On {} shape{}", d.items.len(), if d.items.len() == 1 { "" } else { "s" })).small());
            }
            let edit = ui.add(egui::TextEdit::multiline(&mut d.text).desired_rows(3).desired_width(260.0).hint_text("What should change?"));
            if std::mem::take(&mut d.focus) {
                edit.request_focus();
            }
            post = edit.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
            ui.horizontal(|ui| {
                post |= ui.add_enabled(!d.text.trim().is_empty(), egui::Button::new("Comment")).on_hover_text("Ctrl+Enter").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if post {
            let d = self.comments.draft.take().unwrap();
            let r = self.doc.lock().unwrap().annotate(Author::Person, &d.text, Some(d.at), d.items);
            match r {
                Ok(id) => self.message = format!("Comment {id} added"),
                Err(e) => self.message = e,
            }
            self.comments.placing = false;
        } else if cancel || !open {
            self.comments.draft = None;
        }
    }

    fn thread_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.comments.open else { return };
        let Some(a) = self.doc.lock().unwrap().annotations.get(id).cloned() else {
            self.comments.open = None;
            return;
        };
        let mut open = true;
        let mut action = None;
        let pos = self.w2s(a.at) + vec2(24.0, -10.0);
        egui::Window::new(format!("Comment {id}")).open(&mut open).collapsible(false).resizable(false).default_pos(pos).show(ctx, |ui| {
            ui.set_max_width(300.0);
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                message(ui, &a.author, &a.text);
                for r in &a.replies {
                    ui.separator();
                    message(ui, &r.author, &r.text);
                }
            });
            if a.status == Status::Resolved {
                ui.label(RichText::new(format!("{} Resolved", lucide::CIRCLE_CHECK)).color(RESOLVED));
            }
            ui.separator();
            let reply = &mut self.comments.reply;
            let edit = ui.add(egui::TextEdit::multiline(reply).desired_rows(2).desired_width(280.0).hint_text("Reply"));
            let enter = edit.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
            ui.horizontal(|ui| {
                if (ui.add_enabled(!reply.trim().is_empty(), egui::Button::new("Reply")).on_hover_text("Ctrl+Enter").clicked() || enter) && !reply.trim().is_empty() {
                    action = Some(Act::Reply);
                }
                let (label, status) = if a.status == Status::Open { ("Resolve", Status::Resolved) } else { ("Reopen", Status::Open) };
                if ui.button(label).clicked() {
                    action = Some(Act::Status(status));
                }
                if lucide::delete(ui, true, "Delete the comment").clicked() {
                    action = Some(Act::Delete);
                }
            });
        });
        let mut d = self.doc.lock().unwrap();
        let r = match action {
            Some(Act::Reply) => d.reply_annotation(id, Author::Person, &std::mem::take(&mut self.comments.reply), None),
            Some(Act::Status(s)) => d.set_annotation_status(id, Author::Person, s),
            Some(Act::Delete) => {
                open = false;
                d.delete_annotation(id)
            }
            None => Ok(()),
        };
        if let Err(e) = r {
            self.message = e;
        }
        if !open {
            self.comments.open = None;
        }
    }

    /// The Comments list in the right panel (only when there are any): click one to zoom to it.
    pub(super) fn comments_panel(&mut self, ui: &mut egui::Ui) {
        let (list, open_count) = {
            let d = self.doc.lock().unwrap();
            (d.annotations.list.clone(), d.annotations.open_count())
        };
        if list.is_empty() {
            return;
        }
        let mut go = None;
        egui::CollapsingHeader::new(format!("{} Comments ({open_count} open)", lucide::MESSAGE_SQUARE)).id_salt("comments").default_open(true).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("comments-list").max_height(140.0).show(ui, |ui| {
                for a in &list {
                    let colour = if a.status == Status::Open { ui.visuals().text_color() } else { RESOLVED };
                    let first = a.text.lines().next().unwrap_or_default();
                    let text = RichText::new(format!("{}  {first}", a.id)).color(colour);
                    if ui.add(egui::Label::new(text).truncate().sense(egui::Sense::click())).on_hover_text(&a.text).clicked() {
                        go = Some(a.clone());
                    }
                }
            });
        });
        ui.separator();
        if let Some(a) = go {
            self.zoom_to_comment(&a);
        }
    }

    fn zoom_to_comment(&mut self, a: &Annotation) {
        let around = {
            let d = self.doc.lock().unwrap();
            d.bbox(&a.items)
        };
        let [x, y] = a.at;
        let b = match around {
            Some([bx, by, bw, bh]) => {
                let (x0, y0, x1, y1) = (bx.min(x), by.min(y), (bx + bw).max(x), (by + bh).max(y));
                [x0 - 10.0, y0 - 10.0, x1 - x0 + 20.0, y1 - y0 + 20.0]
            }
            None => [x - 30.0, y - 30.0, 60.0, 60.0],
        };
        self.zoom_to(b);
        self.comments.hidden = false;
        self.comments.open = Some(a.id);
        self.comments.reply.clear();
    }
}

enum Act {
    Reply,
    Status(Status),
    Delete,
}

fn message(ui: &mut egui::Ui, author: &Author, text: &str) {
    ui.label(RichText::new(who(author)).strong().small());
    ui.label(text);
}

