//! Constraint bars (AutoCAD's CONSTRAINTBAR): a small badge per geometric constraint next to the
//! geometry it binds; hovering a badge highlights that geometry. Also the per-frame solve.

use super::App;
use super::glyphs;
use crate::model::constraints::pick::{anchor, outline};
use crate::model::{Ref, State};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, vec2};

const BADGE: f32 = 16.0;
const BAR_BG: Color32 = Color32::from_rgb(52, 58, 70);
const BAR_FG: Color32 = Color32::from_rgb(200, 220, 255);
const HIGHLIGHT: Color32 = Color32::from_rgb(0, 170, 255);

impl App {
    /// Solve the constraints after this frame's changes (see `Document::settle`). While dragging, a
    /// failed solve silently keeps the last good state; otherwise the refusal is reported.
    pub(super) fn settle(&mut self) {
        let explain = self.drag.is_none();
        let result = self.doc.lock().unwrap().settle(explain);
        if let Err(e) = result
            && explain
        {
            self.message = format!("Refused: {e}.");
            self.cmdline.log(self.message.clone());
        }
    }

    /// Draw the badges, grouped per piece of geometry, and highlight what the hovered one binds.
    pub(super) fn paint_bars(&mut self, painter: &egui::Painter, s: &State) {
        self.bar_badges.clear();
        if let Some(c) = self.hot_constraint.and_then(|id| s.constraints.iter().find(|c| c.id == id)) {
            self.highlight(painter, s, &c.refs);
        }
        if !self.bars {
            return;
        }
        // (geometry, its badges) in drawing order.
        let mut bars: Vec<(Ref, Pos2, Vec<usize>)> = Vec::new();
        for (k, c) in s.constraints.iter().enumerate().filter(|(_, c)| !c.kind.is_dimension()) {
            for r in &c.refs {
                if self.hidden_bars.contains(&r.item()) {
                    continue;
                }
                match bars.iter_mut().find(|b| b.0 == *r) {
                    Some(b) => b.2.push(k),
                    None => {
                        if let Some(a) = anchor(s, r) {
                            bars.push((*r, self.w2s(a), vec![k]));
                        }
                    }
                }
            }
        }
        let hover = painter.ctx().pointer_hover_pos();
        let mut hot = None;
        for (_, at, list) in &bars {
            let start = *at + vec2(8.0, 8.0);
            for (n, k) in list.iter().enumerate() {
                let r = Rect::from_min_size(start + vec2(n as f32 * (BADGE + 1.0), 0.0), vec2(BADGE, BADGE));
                let over = hover.is_some_and(|h| r.contains(h));
                painter.rect_filled(r, 3.0, if over { HIGHLIGHT } else { BAR_BG });
                glyphs::paint(painter, r.shrink(3.0), BAR_FG, s.constraints[*k].kind);
                if over {
                    hot = Some((*k, r));
                }
                self.bar_badges.push((s.constraints[*k].id, r));
            }
        }
        let Some((k, r)) = hot else { return };
        let c = &s.constraints[k];
        self.highlight(painter, s, &c.refs);
        let galley = painter.layout_no_wrap(c.label(), FontId::proportional(12.0), Color32::WHITE);
        let pos = r.right_bottom() + vec2(6.0, 2.0);
        painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(3.0), 3.0, Color32::from_black_alpha(200));
        painter.galley(pos, galley, Color32::WHITE);
    }

    /// Show the geometry a constraint binds.
    fn highlight(&self, painter: &egui::Painter, s: &State, refs: &[Ref]) {
        for g in refs {
            let pts: Vec<Pos2> = outline(s, g).iter().map(|p| self.w2s(*p)).collect();
            match pts.as_slice() {
                [p] => _ = painter.circle_stroke(*p, 6.0, Stroke::new(2.5, HIGHLIGHT)),
                _ => _ = painter.add(egui::Shape::line(pts, Stroke::new(4.0, HIGHLIGHT.gamma_multiply(0.7)))),
            }
        }
    }
}
