//! The select tool, AutoCAD style: a click adds to the selection and Shift-click removes; a window
//! (drag or two clicks, left to right) takes items fully inside it, a crossing (right to left) anything
//! it touches; Esc clears. Dragging a grip edits one item or transforms the whole selection.

use super::App;
use super::grips::{self, Grip};
use super::snap::line_line;
use super::tools::Tool;
use crate::model::geom::union;
use crate::model::{BBox, Item, Point, Transform};
use eframe::egui::{self, Color32, PointerButton, Pos2, Rect, Stroke, vec2};

const SELECTED: Color32 = Color32::from_rgb(255, 150, 0);
const GRIP: Color32 = Color32::from_rgb(40, 110, 255);
const GRIP_HOT: Color32 = Color32::from_rgb(240, 60, 60);
/// Half a grip square, in pixels.
const GRIP_HALF: f32 = 4.0;
/// Gap between the selection's bounding box and its free-transform grips, in pixels.
const BOX_PAD: f32 = 12.0;
/// Height of the rotate grip above the box, in pixels.
const ROTATE_LIFT: f32 = 28.0;

/// A drag in progress with the select tool.
pub(super) enum Drag {
    Move { origin: Point, applied: Point },
    /// A selection window from `start`, by dragging or by two clicks.
    Box { start: Point },
    /// A grip dragged from `at`; `base` is every item as it was when the drag started, `bbox` the selection box then.
    Grip { grip: Grip, at: Point, base: Vec<Item>, bbox: BBox },
}

fn selection_box(items: &[Item], selection: &[u64]) -> Option<BBox> {
    union(&items.iter().filter(|i| selection.contains(&i.id)).map(|i| i.shape.metrics().bbox).collect::<Vec<_>>())
}

fn contains_box(outer: BBox, inner: BBox) -> bool {
    inner[0] >= outer[0] && inner[1] >= outer[1] && inner[0] + inner[2] <= outer[0] + outer[2] && inner[1] + inner[3] <= outer[1] + outer[3]
}

/// Whether any outline point lies in `area` or any outline segment crosses its edges.
fn touches(area: BBox, outline: &[Vec<Point>]) -> bool {
    let [x0, y0, x1, y1] = [area[0], area[1], area[0] + area[2], area[1] + area[3]];
    let edges = [([x0, y0], [x1, y0]), ([x1, y0], [x1, y1]), ([x1, y1], [x0, y1]), ([x0, y1], [x0, y0])];
    outline.iter().any(|line| {
        line.iter().any(|p| (x0..=x1).contains(&p[0]) && (y0..=y1).contains(&p[1]))
            || line.windows(2).any(|s| edges.iter().any(|(a, b)| line_line(s[0], s[1], *a, *b).is_some()))
    })
}

impl App {
    /// Grips of the selection (item grips plus the free-transform box) while the select tool is active.
    fn grips(&self, items: &[Item], selection: &[u64]) -> Vec<(Grip, Point)> {
        if self.tool != Tool::Select {
            return Vec::new();
        }
        let mut out: Vec<(Grip, Point)> = items.iter().filter(|i| selection.contains(&i.id)).flat_map(grips::item_grips).collect();
        if let Some(b) = selection_box(items, selection) {
            out.extend(grips::box_grips(b, self.mm(BOX_PAD), self.mm(ROTATE_LIFT)));
        }
        out
    }

    fn mm(&self, px: f32) -> f64 {
        (px / self.view.zoom) as f64
    }

    /// The grip under the screen position `at`.
    fn grip_at(&self, grips: &[(Grip, Point)], at: Pos2) -> Option<(Grip, Point)> {
        grips
            .iter()
            .map(|(g, p)| (*g, *p, self.w2s(*p).distance(at)))
            .filter(|(_, _, d)| *d <= GRIP_HALF + 3.0)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(g, p, _)| (g, p))
    }

    /// Mouse input for the select tool. Returns where the crosshair goes (snapped while dragging a grip).
    pub(super) fn select_input(&mut self, ui: &egui::Ui, response: &egui::Response, raw: Option<Point>) -> Option<Point> {
        let shift = ui.input(|i| i.modifiers.shift);
        let doc = self.doc.clone();
        if response.drag_started_by(PointerButton::Primary)
            && !matches!(self.drag, Some(Drag::Box { .. }))
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let origin = self.s2w(o);
            let mut d = doc.lock().unwrap();
            let grip = self.grip_at(&self.grips(&d.s.items, &d.selection), o);
            self.drag = Some(match (grip, self.hit(&d, origin)) {
                (Some((grip, at)), _) => {
                    d.checkpoint();
                    let bbox = d.bbox(&d.selection).unwrap_or_default();
                    Drag::Grip { grip, at, base: d.s.items.clone(), bbox }
                }
                (None, Some(id)) => {
                    if !d.selection.contains(&id) {
                        d.selection = vec![id];
                    }
                    d.checkpoint();
                    Drag::Move { origin, applied: [0.0, 0.0] }
                }
                (None, None) => Drag::Box { start: origin },
            });
        }

        let mut cursor = raw;
        if response.dragged_by(PointerButton::Primary)
            && let Some(r) = raw
            && matches!(self.drag, Some(Drag::Grip { .. }))
            && let Some(Drag::Grip { grip, at, base, bbox }) = self.drag.take()
        {
            let p = self.aim(&base, r, Some(at));
            cursor = Some(p);
            self.apply_grip(grip, at, &base, bbox, p, shift);
            self.drag = Some(Drag::Grip { grip, at, base, bbox });
        }
        if let (Some(Drag::Move { origin, applied }), Some(p)) = (&mut self.drag, raw) {
            let mut want = [p[0] - origin[0], p[1] - origin[1]];
            if self.snap {
                want = [want[0].round(), want[1].round()];
            }
            let (dx, dy) = (want[0] - applied[0], want[1] - applied[1]);
            if dx != 0.0 || dy != 0.0 {
                *applied = want;
                let mut d = doc.lock().unwrap();
                let ids = d.selection.clone();
                d.transform(&ids, &Transform::translate(dx, dy));
            }
        }
        if response.drag_stopped_by(PointerButton::Primary) {
            if let (Some(Drag::Box { start }), Some(p)) = (&self.drag, raw) {
                self.finish_box(*start, p, shift);
            }
            self.drag = None;
        }

        if response.clicked_by(PointerButton::Primary)
            && let Some(p) = raw
        {
            let mut d = doc.lock().unwrap();
            match self.hit(&d, p) {
                // Shift+click adds or removes; a plain click selects only that shape.
                Some(id) if shift && d.selection.contains(&id) => d.selection.retain(|s| *s != id),
                Some(id) if shift => d.selection.push(id),
                Some(id) => d.selection = vec![id],
                // A click on empty space clears the selection; window and crossing are made by dragging.
                None if !shift => d.selection.clear(),
                None => {}
            }
        }
        cursor
    }

    /// Redo the grip edit from the state at drag start, so every frame applies one clean change.
    fn apply_grip(&mut self, grip: Grip, at: Point, base: &[Item], bbox: BBox, p: Point, shift: bool) {
        let mut d = self.doc.lock().unwrap();
        let ids = d.selection.clone();
        if let Some(id) = grip.id() {
            // Path grips set a point, so they apply to the shape as the solver left it last frame:
            // only the dragged point counts as the user's edit.
            let from_now = matches!(grip, Grip::Vertex { .. } | Grip::Radius { .. } | Grip::Centre { .. });
            if let (Some(b), Some(item)) = (base.iter().find(|i| i.id == id), d.item_mut(id)) {
                item.shape = grips::drag_item(if from_now { &item.shape } else { &b.shape }, grip, at, p);
            }
            d.touch();
        } else if let Some(t) = grips::box_transform(bbox, grip, at, p, shift) {
            // ponytail: linear lookup per selected item; index `base` by id if huge selections get slow.
            for item in d.s.items.iter_mut().filter(|i| ids.contains(&i.id)) {
                if let Some(b) = base.iter().find(|b| b.id == item.id) {
                    item.shape = b.shape.clone();
                }
            }
            d.transform(&ids, &t);
        }
    }

    /// Window (left to right): items fully inside. Crossing (right to left): also items it touches.
    /// Without Shift the box replaces the selection; with Shift it adds to it.
    fn finish_box(&mut self, start: Point, end: Point, add: bool) {
        let area = [start[0].min(end[0]), start[1].min(end[1]), (end[0] - start[0]).abs(), (end[1] - start[1]).abs()];
        let crossing = end[0] < start[0];
        let mut d = self.doc.lock().unwrap();
        let picked: Vec<u64> = d
            .s
            .items
            .iter()
            .filter(|i| contains_box(area, i.shape.metrics().bbox) || (crossing && touches(area, &i.shape.outline(0.1))))
            .map(|i| i.id)
            .collect();
        // Without Shift the box replaces the selection; with Shift it adds to it.
        if !add {
            d.selection.clear();
        }
        {
            for id in picked {
                if !d.selection.contains(&id) {
                    d.selection.push(id);
                }
            }
        }
    }

    /// Esc: drop a pending window, or put back what a grip or move drag changed.
    pub(super) fn cancel_drag(&mut self) {
        if let Some(Drag::Grip { .. } | Drag::Move { .. }) = self.drag.take() {
            self.doc.lock().unwrap().undo();
        }
    }

    /// The selection box with its grips, and the selection window being dragged.
    pub(super) fn paint_selection(&self, painter: &egui::Painter, items: &[Item], selection: &[u64], pointer: Option<Point>) {
        if let Some(b) = selection_box(items, selection) {
            let pb = grips::padded(b, self.mm(BOX_PAD));
            let r = Rect::from_min_max(self.w2s([pb[0], pb[1]]), self.w2s([pb[0] + pb[2], pb[1] + pb[3]]));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, SELECTED.gamma_multiply(0.7)), egui::StrokeKind::Middle);
            if self.tool == Tool::Select {
                painter.line_segment([r.center_top(), r.center_top() - vec2(0.0, ROTATE_LIFT)], Stroke::new(1.0, SELECTED.gamma_multiply(0.7)));
            }
        }
        let grips = self.grips(items, selection);
        let active = match &self.drag {
            Some(Drag::Grip { grip, .. }) => Some(*grip),
            _ => painter.ctx().pointer_hover_pos().and_then(|h| self.grip_at(&grips, h)).map(|(g, _)| g),
        };
        for (g, p) in &grips {
            let color = if Some(*g) == active { GRIP_HOT } else { GRIP };
            let at = self.w2s(*p);
            let square = Rect::from_center_size(at, vec2(GRIP_HALF * 2.0, GRIP_HALF * 2.0));
            match g {
                Grip::Rotate => painter.circle(at, GRIP_HALF + 1.0, color, Stroke::new(1.0, Color32::WHITE)),
                Grip::Scale { .. } => painter.rect(square, 0.0, Color32::WHITE, Stroke::new(1.5, color), egui::StrokeKind::Middle),
                _ => painter.rect(square, 0.0, color, Stroke::new(1.0, Color32::from_gray(20)), egui::StrokeKind::Middle),
            };
        }
        if let (Some(Drag::Box { start }), Some(p)) = (&self.drag, pointer) {
            let r = Rect::from_two_pos(self.w2s(*start), self.w2s(p));
            if p[0] >= start[0] {
                let blue = Color32::from_rgb(80, 140, 255);
                painter.rect(r, 0.0, blue.gamma_multiply(0.15), Stroke::new(1.0, blue), egui::StrokeKind::Inside);
            } else {
                let green = Color32::from_rgb(70, 200, 90);
                painter.rect_filled(r, 0.0, green.gamma_multiply(0.15));
                let outline = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&outline, Stroke::new(1.0, green), 6.0, 4.0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grip_drags_keep_constraints() {
        use crate::model::{Contour, Document, Kind, Op, Ref, Shape};
        let doc = std::sync::Arc::new(std::sync::Mutex::new(Document::new(200.0, 200.0)));
        let mut app = App::new(doc.clone(), Err(String::new()));
        let id = {
            let mut d = doc.lock().unwrap();
            let id = d.add("", Op::Cut, false, Shape::path(Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0)));
            for (k, kind) in [(0, Kind::Horizontal), (1, Kind::Vertical), (2, Kind::Horizontal), (3, Kind::Vertical)] {
                d.add_constraint(kind, vec![Ref::Segment { item: id, contour: 0, segment: k }]).unwrap();
            }
            d.settle(true).unwrap();
            d.selection = vec![id];
            id
        };
        let base = doc.lock().unwrap().s.items.clone();
        let grip = Grip::Vertex { id, contour: 0, vertex: 2 };
        app.drag = Some(Drag::Box { start: [0.0, 0.0] });
        // Two frames of the drag, each settled as the canvas does.
        for p in [[55.0, 35.0], [70.0, 50.0]] {
            app.apply_grip(grip, [50.0, 30.0], &base, [10.0, 10.0, 40.0, 20.0], p, false);
            app.settle();
        }
        let pts: Vec<Point> = doc.lock().unwrap().item(id).unwrap().shape.contours()[0].vertices.iter().map(|v| [v.x, v.y]).collect();
        assert_eq!(pts, vec![[10.0, 10.0], [70.0, 10.0], [70.0, 50.0], [10.0, 50.0]]);
    }

    #[test]
    fn crossing_touches() {
        let square = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]];
        assert!(touches([5.0, -1.0, 1.0, 2.0], &square), "box over the top edge");
        assert!(!touches([2.0, 2.0, 1.0, 1.0], &square), "box inside, touching no line");
        assert!(touches([-1.0, -1.0, 2.0, 2.0], &square), "corner inside the box");
        assert!(contains_box([-1.0, -1.0, 12.0, 12.0], [0.0, 0.0, 10.0, 10.0]));
    }
}
