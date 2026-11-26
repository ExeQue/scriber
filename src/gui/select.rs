//! The select tool, AutoCAD style: a click adds to the selection and Shift-click removes; a window
//! (drag or two clicks, left to right) takes items fully inside it, a crossing (right to left) anything
//! it touches; Esc clears. Dragging a grip edits one item or transforms the whole selection.

use super::App;
use super::grips::{self, Grip};
use super::snap::line_line;
use super::tools::Tool;
use crate::model::turn::{self, turned_box};
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
    /// A grip dragged from `at`; `base` is every item as it was when the drag started, `frame` the
    /// selection box then (box and angle, see `turn::turned_box`). A `hot` grip follows the cursor without a button held, until a click.
    Grip { grip: Grip, at: Point, base: Vec<Item>, frame: (BBox, f64), hot: bool },
    /// A dimension's label, grabbed `grab` mm from its anchor.
    Label { id: u64, grab: Point },
}

/// The selection box, turned with the items when they share an angle.
fn selection_box(items: &[Item], selection: &[u64]) -> Option<(BBox, f64)> {
    turned_box(items.iter().filter(|i| selection.contains(&i.id)))
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
        if let Some((b, angle)) = selection_box(items, selection) {
            out.extend(grips::box_grips(b, angle, self.mm(BOX_PAD), self.mm(ROTATE_LIFT)));
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
        let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
        let doc = self.doc.clone();
        let hot = matches!(self.drag, Some(Drag::Grip { hot: true, .. }));
        if response.drag_started_by(PointerButton::Primary)
            && !hot
            && !matches!(self.drag, Some(Drag::Box { .. }))
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let origin = self.s2w(o);
            let mut d = doc.lock().unwrap();
            let grip = self.grip_at(&self.grips(&d.s.items, &d.selection), o);
            let label = self.dim_labels.iter().find(|(_, r)| r.contains(o)).and_then(|(id, _)| self.grab_label(&d, *id, origin));
            self.drag = Some(match (grip, label, self.hit(&d, origin)) {
                (None, Some(drag), _) => {
                    d.checkpoint();
                    drag
                }
                (Some((grip, at)), ..) => {
                    d.checkpoint();
                    let frame = selection_box(&d.s.items, &d.selection).unwrap_or_default();
                    Drag::Grip { grip, at, base: d.s.items.clone(), frame, hot: false }
                }
                // A grouped shape moves with its group.
                (None, None, Some(id)) => {
                    if !d.selection.contains(&id) {
                        d.selection = d.expand_groups(&[id]);
                    }
                    d.checkpoint();
                    Drag::Move { origin, applied: [0.0, 0.0] }
                }
                (None, None, None) => Drag::Box { start: origin },
            });
            drop(d);
            self.grip_menu = None;
            self.dyn_reset();
        }

        let mut cursor = raw;
        if (response.dragged_by(PointerButton::Primary) || hot)
            && let Some(r) = raw
            && let Some(Drag::Grip { grip, at, base, frame, hot }) = self.drag.take_if(|d| matches!(d, Drag::Grip { .. }))
        {
            // The rotate handle turns; POLAR snaps its angle instead of tracking lines from the handle.
            let p = if grip == Grip::Rotate { r } else { self.aim(&base, r, tracks_from(grip, &base).then_some(at)) };
            self.drag = Some(Drag::Grip { grip, at, base, frame, hot });
            // Locked values typed on dynamic input win over the mouse.
            let p = self.dyn_point(p).unwrap_or(p);
            cursor = Some(p);
            self.move_grip(p, shift, alt);
        }
        if let (Some(Drag::Label { id, grab }), Some(p)) = (&self.drag, raw) {
            self.drag_label(*id, [p[0] - grab[0], p[1] - grab[1]]);
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
            match (self.drag.take(), raw) {
                (Some(Drag::Box { start }), Some(p)) => self.finish_box(start, p, shift),
                (Some(g @ Drag::Grip { .. }), r) => {
                    self.drag = Some(g);
                    self.finish_grip(r);
                }
                _ => {}
            }
        }

        if response.clicked_by(PointerButton::Primary)
            && let Some(p) = raw
        {
            let grip = response.interact_pointer_pos().and_then(|at| {
                let d = doc.lock().unwrap();
                self.grip_at(&self.grips(&d.s.items, &d.selection), at)
            });
            match grip {
                _ if hot => self.finish_grip(Some(p)),
                // A click on a grip makes it hot, as in AutoCAD: it follows the cursor until the next click.
                Some((grip, at)) => {
                    self.grip_menu = None;
                    self.start_hot_grip(grip, at, true);
                }
                None => self.click_select(p, shift),
            }
        }

        // Before a click: what it would pick glows, overlapping shapes can be cycled, grips offer their menu.
        if self.drag.is_none() {
            let (under, grip) = match raw {
                Some(p) => {
                    let d = doc.lock().unwrap();
                    // ponytail: outlines of every visible item per frame; cache them if big drawings get slow.
                    (self.hits(&d, p), response.hover_pos().and_then(|h| self.grip_at(&self.grips(&d.s.items, &d.selection), h)))
                }
                None => (Vec::new(), None),
            };
            self.cycle.update(under);
            self.update_hover(if grip.is_some() { None } else { self.cycle.current() });
            self.rest_on_grip(ui.ctx(), grip);
        } else {
            self.update_hover(None);
        }
        cursor
    }

    /// A click on a shape selects it with its group: Shift adds them, or takes them out when selected.
    fn click_select(&mut self, p: Point, shift: bool) {
        let mut d = self.doc.lock().unwrap();
        match self.hit(&d, p) {
            Some(id) => {
                let group = d.expand_groups(&[id]);
                if shift && d.selection.contains(&id) {
                    d.selection.retain(|s| !group.contains(s));
                } else if shift {
                    for g in group {
                        if !d.selection.contains(&g) {
                            d.selection.push(g);
                        }
                    }
                } else {
                    d.selection = group;
                }
            }
            // A click on empty space clears the selection; window and crossing are made by dragging.
            None if !shift => d.selection.clear(),
            None => {}
        }
    }

    /// Move the dragged grip to `p`.
    pub(super) fn move_grip(&mut self, p: Point, shift: bool, alt: bool) {
        let Some(Drag::Grip { grip, at, base, frame, hot }) = self.drag.take() else { return };
        self.apply_grip(grip, at, &base, frame, p, shift, alt);
        self.drag = Some(Drag::Grip { grip, at, base, frame, hot });
    }

    /// Let go of the grip (at `raw`, if over the canvas): INFER joins its point to a snap it was let go on.
    pub(super) fn finish_grip(&mut self, raw: Option<Point>) {
        let Some(Drag::Grip { grip, at, base, .. }) = self.drag.take() else { return };
        if let Some(r) = raw {
            self.aim(&base, r, Some(at));
            self.infer_grip(grip, self.aids.hit);
        }
        self.dyn_reset();
        self.once = None;
        self.tracker.clear();
        self.message.clear();
    }

    /// Redo the grip edit from the state at drag start, so every frame applies one clean change.
    fn apply_grip(&mut self, grip: Grip, at: Point, base: &[Item], frame: (BBox, f64), p: Point, shift: bool, alt: bool) {
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
        } else if let Some(mut t) = grips::box_transform(frame.0, frame.1, grip, at, p, shift, alt) {
            // POLAR snaps the angle the items end up at, not the turn.
            if grip == Grip::Rotate && !shift && self.aids.polar {
                t.rotate = snap_angle(frame.1 + t.rotate, &self.aids.polar_angles(), 4.0) - frame.1;
            }
            // ponytail: linear lookup per selected item; index `base` by id if huge selections get slow.
            for item in d.s.items.iter_mut().filter(|i| ids.contains(&i.id)) {
                if let Some(b) = base.iter().find(|b| b.id == item.id) {
                    item.shape = b.shape.clone();
                    item.rotation = b.rotation;
                }
            }
            d.transform_turned(&ids, &t, frame.1);
        }
    }

    /// Window (left to right): items fully inside. Crossing (right to left): also items it touches.
    /// Without Shift the box replaces the selection; with Shift it adds to it.
    fn finish_box(&mut self, start: Point, end: Point, add: bool) {
        let area = [start[0].min(end[0]), start[1].min(end[1]), (end[0] - start[0]).abs(), (end[1] - start[1]).abs()];
        let crossing = end[0] < start[0];
        let mut d = self.doc.lock().unwrap();
        // Hidden and isolated-away items are not picked; a picked item brings its group.
        let picked: Vec<u64> = d
            .s
            .items
            .iter()
            .filter(|i| self.visible(&d.s, i) && d.s.editable(i))
            .filter(|i| contains_box(area, i.shape.metrics().bbox) || (crossing && touches(area, &i.shape.outline(0.1))))
            .map(|i| i.id)
            .collect();
        let picked = d.expand_groups(&picked);
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
            self.tracker.clear();
            self.message.clear();
        }
    }

    /// The selection box with its grips, and the selection window being dragged.
    pub(super) fn paint_selection(&self, painter: &egui::Painter, items: &[Item], selection: &[u64], pointer: Option<Point>) {
        if let Some((b, angle)) = selection_box(items, selection) {
            let pb = grips::padded(b, self.mm(BOX_PAD));
            let stroke = Stroke::new(1.0, SELECTED.gamma_multiply(0.7));
            let corner = |fx: f64, fy: f64| self.w2s(turn::to_world([pb[0] + fx * pb[2], pb[1] + fy * pb[3]], angle));
            let corners = vec![corner(0.0, 0.0), corner(1.0, 0.0), corner(1.0, 1.0), corner(0.0, 1.0)];
            painter.add(egui::Shape::closed_line(corners, stroke));
            if self.tool == Tool::Select {
                let lift = turn::to_world([pb[0] + pb[2] / 2.0, pb[1] - self.mm(ROTATE_LIFT)], angle);
                painter.line_segment([corner(0.5, 0.0), self.w2s(lift)], stroke);
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
                Grip::Mid { .. } => painter.rect(square.shrink(1.5), 0.0, color.gamma_multiply(0.8), Stroke::new(1.0, Color32::from_gray(20)), egui::StrokeKind::Middle),
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
        let mut app = App::new(crate::model::Workspace::from_shared(doc.clone()), Err(String::new()));
        let id = {
            let mut d = doc.lock().unwrap();
            let id = d.add("", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0)));
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
            app.apply_grip(grip, [50.0, 30.0], &base, ([10.0, 10.0, 40.0, 20.0], 0.0), p, false, false);
            app.settle();
        }
        let pts: Vec<Point> = doc.lock().unwrap().item(id).unwrap().shape.contours()[0].vertices.iter().map(|v| [v.x, v.y]).collect();
        assert_eq!(pts, vec![[10.0, 10.0], [70.0, 10.0], [70.0, 50.0], [10.0, 50.0]]);
    }

    #[test]
    fn box_turns_with_the_items_and_resizes_along_their_axes() {
        use crate::model::{Contour, Document, Op, Shape};
        let doc = std::sync::Arc::new(std::sync::Mutex::new(Document::new(200.0, 200.0)));
        let mut app = App::new(crate::model::Workspace::from_shared(doc.clone()), Err(String::new()));
        app.aids.polar = false;
        let id = doc.lock().unwrap().add("", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0)));
        doc.lock().unwrap().selection = vec![id];
        let frame = |doc: &crate::model::Shared| {
            let d = doc.lock().unwrap();
            selection_box(&d.s.items, &d.selection).unwrap()
        };
        let grip_at = |f: (BBox, f64), g: Grip| grips::box_grips(f.0, f.1, 0.0, 10.0).into_iter().find(|(x, _)| *x == g).unwrap().1;
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;

        // Rotate grip dragged a quarter turn about the centre (30, 20): the angle is stored, the box keeps its size.
        let f = frame(&doc);
        let base = doc.lock().unwrap().s.items.clone();
        app.apply_grip(Grip::Rotate, grip_at(f, Grip::Rotate), &base, f, [60.0, 20.0], false, false);
        let f = frame(&doc);
        assert!(close(f.1, 90.0) && close(f.0[2], 40.0) && close(f.0[3], 20.0), "{f:?}");
        assert_eq!(doc.lock().unwrap().item(id).unwrap().rotation, 90.0);

        // The right edge of the turned box now points down: dragging it 10 mm down makes it 50 wide.
        let base = doc.lock().unwrap().s.items.clone();
        let at = grip_at(f, Grip::Scale { fx: 1.0, fy: 0.5 });
        app.apply_grip(Grip::Scale { fx: 1.0, fy: 0.5 }, at, &base, f, [at[0], at[1] + 10.0], false, false);
        let f2 = frame(&doc);
        assert!(close(f2.1, 90.0) && close(f2.0[2], 50.0) && close(f2.0[3], 20.0), "{f2:?}");

        // At 30° a stretch along the turned axes keeps the angle too.
        doc.lock().unwrap().set_rotation(&[id], 30.0).unwrap();
        let (f, base) = (frame(&doc), doc.lock().unwrap().s.items.clone());
        let at = grip_at(f, Grip::Scale { fx: 1.0, fy: 0.5 });
        let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
        app.apply_grip(Grip::Scale { fx: 1.0, fy: 0.5 }, at, &base, f, [at[0] + 10.0 * c, at[1] + 10.0 * s], false, false);
        let f2 = frame(&doc);
        assert!(close(f2.1, 30.0) && close(f2.0[2], 60.0) && close(f2.0[3], 20.0), "{f2:?}");

        // Shift snaps the resulting angle (not the turn) to 15°: from 30° a small turn ends at 45°.
        let (f, base) = (frame(&doc), doc.lock().unwrap().s.items.clone());
        let centre = crate::model::turn::to_world([f.0[0] + f.0[2] / 2.0, f.0[1] + f.0[3] / 2.0], f.1);
        let from = grip_at(f, Grip::Rotate);
        let to = crate::model::geom::rotate_point(from, centre, 12.0);
        app.apply_grip(Grip::Rotate, from, &base, f, to, true, false);
        assert!(close(doc.lock().unwrap().item(id).unwrap().rotation, 45.0));
    }

    #[test]
    fn window_takes_whole_groups_and_skips_hidden_items() {
        use crate::model::{Contour, Document, Op, Shape};
        let doc = std::sync::Arc::new(std::sync::Mutex::new(Document::new(200.0, 200.0)));
        let mut app = App::new(crate::model::Workspace::from_shared(doc.clone()), Err(String::new()));
        let [a, b, c] = {
            let mut d = doc.lock().unwrap();
            let ids = [0.0, 50.0, 100.0].map(|x| d.add("", Op::Cut, Shape::path(Contour::rect(x, 0.0, 10.0, 10.0, 0.0))));
            d.s.groups.push(vec![ids[0], ids[1]]);
            ids
        };
        app.finish_box([-1.0, -1.0], [20.0, 20.0], false);
        assert_eq!(doc.lock().unwrap().selection, vec![a, b], "the window caught a; b is in its group");
        app.isolated = Some(vec![a, b]);
        app.finish_box([-1.0, -1.0], [199.0, 20.0], false);
        assert!(!doc.lock().unwrap().selection.contains(&c), "isolated away");
        // A click on b selects its group too.
        app.click_select([50.0, 5.0], false);
        assert_eq!(doc.lock().unwrap().selection, vec![b, a]);
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

/// Whether POLAR and tracking measure from the grip's start: they do for grips that move a point or
/// a straight edge, not for bending an arc (its middle grip) or a circle's radius.
fn tracks_from(grip: Grip, items: &[Item]) -> bool {
    match grip {
        Grip::Radius { .. } => false,
        Grip::Mid { id, contour, segment } => !items
            .iter()
            .find(|i| i.id == id)
            .and_then(|i| match &i.shape {
                crate::model::Shape::Path { contours } => contours.get(contour).and_then(|c| c.vertices.get(segment)),
                _ => None,
            })
            .is_some_and(|v| v.bulge != 0.0),
        _ => true,
    }
}

/// `a` (degrees) moved onto the nearest of `angles` when within `tol` degrees.
fn snap_angle(a: f64, angles: &[f64], tol: f64) -> f64 {
    let diff = |x: f64| ((a - x + 180.0).rem_euclid(360.0) - 180.0).abs();
    angles.iter().copied().filter(|x| diff(*x) <= tol).min_by(|x, y| diff(*x).total_cmp(&diff(*y))).map_or(a, |x| a - ((a - x + 180.0).rem_euclid(360.0) - 180.0))
}

#[test]
fn rotation_snaps_to_polar_angles() {
    let angles = [0.0, 15.0, 30.0, 345.0];
    assert!((snap_angle(16.38, &angles, 4.0) - 15.0).abs() < 1e-9);
    assert!((snap_angle(-14.0, &angles, 4.0) + 15.0).abs() < 1e-9);
    assert_eq!(snap_angle(22.0, &angles, 4.0), 22.0);
}
