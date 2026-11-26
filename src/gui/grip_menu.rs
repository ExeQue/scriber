//! Multifunctional grips, as in AutoCAD: resting the pointer on a grip shows a small menu of what
//! it can do (a path vertex: stretch, add or remove a vertex; a segment's middle: stretch, add a
//! vertex, turn the segment into an arc or back into a line; a circle's quadrant: radius). Stretch
//! and the actions that make a new point leave the grip "hot": it follows the cursor until a click,
//! and dynamic input takes typed distances and angles (or the radius).

use super::App;
use super::grips::{self, Grip, bulge_through, segment_count, segment_mid};
use super::select::Drag;
use crate::model::geom::Vertex;
use crate::model::{Contour, Point, Shape};
use eframe::egui;
use std::time::{Duration, Instant};

/// How long the pointer rests on a grip before its menu opens.
const MENU_DELAY: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Stretch,
    AddVertex,
    RemoveVertex,
    ToArc,
    ToLine,
    Radius,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::Stretch => "Stretch",
            Action::AddVertex => "Add vertex",
            Action::RemoveVertex => "Remove vertex",
            Action::ToArc => "Convert to arc",
            Action::ToLine => "Convert to line",
            Action::Radius => "Radius",
        }
    }
}

fn contour(shape: &Shape, i: usize) -> Option<&Contour> {
    match shape {
        Shape::Path { contours } => contours.get(i),
        _ => None,
    }
}

/// A vertex can go while an open path keeps 2 and a closed one 3.
fn removable(c: &Contour) -> bool {
    c.vertices.len() > if c.closed { 3 } else { 2 }
}

/// What a grip's menu offers.
pub fn actions(shape: &Shape, grip: Grip) -> Vec<Action> {
    match grip {
        Grip::Vertex { contour: ci, .. } => {
            let mut out = vec![Action::Stretch, Action::AddVertex];
            if contour(shape, ci).is_some_and(removable) {
                out.push(Action::RemoveVertex);
            }
            out
        }
        Grip::Mid { contour: ci, segment, .. } => {
            let straight = contour(shape, ci).and_then(|c| c.vertices.get(segment)).is_some_and(|v| v.bulge.abs() < 1e-9);
            vec![Action::Stretch, Action::AddVertex, if straight { Action::ToArc } else { Action::ToLine }]
        }
        Grip::Radius { .. } => vec![Action::Radius],
        _ => Vec::new(),
    }
}

/// Split segment `seg` at its middle; an arc becomes two arcs. Past the end of an open path, a new
/// last vertex on top of the old one. Returns the new vertex's index.
pub fn add_vertex(c: &mut Contour, seg: usize) -> usize {
    if seg >= segment_count(c) {
        let last = *c.vertices.last().expect("a path has vertices");
        c.vertices.push(Vertex { bulge: 0.0, ..last });
        return c.vertices.len() - 1;
    }
    let m = segment_mid(c, seg);
    // Each half sweeps half the angle: tan(sweep / 8).
    let half = (c.vertices[seg].bulge.atan() / 2.0).tan();
    c.vertices[seg].bulge = half;
    c.vertices.insert(seg + 1, Vertex::new(m[0], m[1], half));
    seg + 1
}

/// Remove vertex `v`; the segment that now spans the gap is straight.
pub fn remove_vertex(c: &mut Contour, v: usize) {
    let n = c.vertices.len();
    if v < n && removable(c) {
        if v > 0 || c.closed {
            c.vertices[(v + n - 1) % n].bulge = 0.0;
        }
        c.vertices.remove(v);
    }
}

/// Do `action` on `shape` from `grip`: the new shape, and the grip that stays hot (follows the cursor).
pub fn apply(shape: &Shape, grip: Grip, action: Action) -> Option<(Shape, Option<Grip>)> {
    let mut s = shape.clone();
    let Shape::Path { contours } = &mut s else { return None };
    let (id, ci, at) = match grip {
        Grip::Vertex { id, contour, vertex } => (id, contour, vertex),
        Grip::Mid { id, contour, segment } => (id, contour, segment),
        Grip::Radius { .. } => return Some((shape.clone(), Some(grip))),
        _ => return None,
    };
    let c = contours.get_mut(ci)?;
    let hot = match (action, grip) {
        (Action::Stretch | Action::Radius, _) => Some(grip),
        (Action::AddVertex, _) => Some(Grip::Vertex { id, contour: ci, vertex: add_vertex(c, at) }),
        (Action::RemoveVertex, _) => {
            remove_vertex(c, at);
            None
        }
        (Action::ToArc, Grip::Mid { .. }) => {
            // A gentle arc to start from; the hot grip bends it further.
            let (a, b) = (c.vertices[at], c.vertices[(at + 1) % c.vertices.len()]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            c.vertices[at].bulge = bulge_through([a.x, a.y], [b.x, b.y], [(a.x + b.x) / 2.0 + dy / 4.0, (a.y + b.y) / 2.0 - dx / 4.0]);
            Some(grip)
        }
        (Action::ToLine, Grip::Mid { .. }) => {
            c.vertices[at].bulge = 0.0;
            None
        }
        _ => return None,
    };
    Some((s, hot))
}

impl App {
    /// The pointer rests on `grip` (or none): open its menu once it has rested long enough.
    pub(super) fn rest_on_grip(&mut self, ctx: &egui::Context, grip: Option<(Grip, Point)>) {
        if self.grip_menu.is_some() {
            return;
        }
        match (self.grip_rest, grip) {
            (Some((g, since)), Some((now, at))) if g == now => {
                let waited = since.elapsed();
                if waited >= MENU_DELAY {
                    self.grip_menu = Some((g, at));
                } else {
                    ctx.request_repaint_after(MENU_DELAY - waited);
                }
            }
            (_, g) => self.grip_rest = g.map(|(g, _)| (g, Instant::now())),
        }
    }

    /// The grip menu, while the pointer stays on the grip or the menu.
    pub(super) fn grip_menu_window(&mut self, ctx: &egui::Context) {
        let Some((grip, at)) = self.grip_menu else { return };
        let shape = grip.id().and_then(|id| self.doc.lock().unwrap().item(id).map(|i| i.shape.clone()));
        let list = shape.as_ref().map(|s| actions(s, grip)).unwrap_or_default();
        if list.is_empty() || self.drag.is_some() {
            self.grip_menu = None;
            return;
        }
        let spot = self.w2s(at);
        let mut chosen = None;
        let area = egui::Area::new(egui::Id::unique("grip-menu")).fixed_pos(spot + egui::vec2(10.0, 6.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                for a in &list {
                    if ui.button(a.label()).clicked() {
                        chosen = Some(*a);
                    }
                }
            });
        });
        let pointer = ctx.pointer_hover_pos();
        let near = pointer.is_some_and(|p| area.response.rect.expand(8.0).contains(p) || p.distance(spot) < 12.0);
        if let Some(a) = chosen {
            self.grip_menu = None;
            self.grip_action(grip, at, a);
        } else if !near {
            self.grip_menu = None;
            self.grip_rest = None;
        }
    }

    fn grip_action(&mut self, grip: Grip, at: Point, action: Action) {
        let Some(id) = grip.id() else { return };
        let mut d = self.doc.lock().unwrap();
        let Some((shape, hot)) = d.item(id).and_then(|i| apply(&i.shape, grip, action)) else { return };
        let changed = d.item(id).is_some_and(|i| i.shape != shape);
        if changed {
            d.checkpoint();
            if let Some(item) = d.item_mut(id) {
                item.shape = shape;
            }
            d.touch();
        }
        drop(d);
        if let Some(g) = hot {
            self.start_hot_grip(g, at, !changed);
        }
    }

    /// Make `grip` hot: it follows the cursor until a click (Esc undoes). `near` picks among a
    /// circle's four radius grips. Without `checkpoint` the undo step taken before is reused.
    pub(super) fn start_hot_grip(&mut self, grip: Grip, near: Point, checkpoint: bool) {
        let mut d = self.doc.lock().unwrap();
        let Some(item) = grip.id().and_then(|id| d.item(id)) else { return };
        let Some(at) = grips::item_grips(item)
            .into_iter()
            .filter(|(g, _)| *g == grip)
            .map(|(_, p)| p)
            .min_by(|a, b| crate::model::geom::distance(*a, near).total_cmp(&crate::model::geom::distance(*b, near)))
        else {
            return;
        };
        if checkpoint {
            d.checkpoint();
        }
        // Item grips only: the selection box frame is not used.
        self.drag = Some(Drag::Grip { grip, at, base: d.s.items.clone(), frame: Default::default(), hot: true });
        drop(d);
        self.dyn_reset();
        self.message = "Move the cursor and click to place, or type a distance (Tab for the angle) and press Enter. Esc puts it back.".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::geom::distance;

    fn pts(s: &Shape) -> Vec<Point> {
        s.contours()[0].vertices.iter().map(|v| [v.x, v.y]).collect()
    }

    #[test]
    fn menus_per_grip() {
        let tri = Shape::path(Contour::polyline(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], true));
        let v = Grip::Vertex { id: 1, contour: 0, vertex: 0 };
        assert_eq!(actions(&tri, v), vec![Action::Stretch, Action::AddVertex]);
        let m = Grip::Mid { id: 1, contour: 0, segment: 1 };
        assert_eq!(actions(&tri, m), vec![Action::Stretch, Action::AddVertex, Action::ToArc]);
        assert_eq!(actions(&tri, Grip::Move { id: 1 }), vec![]);
    }

    #[test]
    fn add_and_remove_vertices() {
        let line = Shape::path(Contour::polyline(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false));
        let (s, hot) = apply(&line, Grip::Mid { id: 1, contour: 0, segment: 0 }, Action::AddVertex).unwrap();
        assert_eq!(pts(&s), vec![[0.0, 0.0], [5.0, 0.0], [10.0, 0.0], [10.0, 10.0]]);
        assert_eq!(hot, Some(Grip::Vertex { id: 1, contour: 0, vertex: 1 }));
        // On the last vertex of an open path: a new end on top of it, to drag out.
        let (s, hot) = apply(&line, Grip::Vertex { id: 1, contour: 0, vertex: 2 }, Action::AddVertex).unwrap();
        assert_eq!(pts(&s).len(), 4);
        assert_eq!(hot, Some(Grip::Vertex { id: 1, contour: 0, vertex: 3 }));
        let (s, hot) = apply(&line, Grip::Vertex { id: 1, contour: 0, vertex: 1 }, Action::RemoveVertex).unwrap();
        assert_eq!((pts(&s), hot), (vec![[0.0, 0.0], [10.0, 10.0]], None));
        // Two vertices left: nothing more to remove.
        let (s2, _) = apply(&s, Grip::Vertex { id: 1, contour: 0, vertex: 0 }, Action::RemoveVertex).unwrap();
        assert_eq!(pts(&s2).len(), 2);
    }

    #[test]
    fn arcs_split_and_convert() {
        let line = Shape::path(Contour::line([0.0, 0.0], [10.0, 0.0]));
        let mid = Grip::Mid { id: 1, contour: 0, segment: 0 };
        let (arc, hot) = apply(&line, mid, Action::ToArc).unwrap();
        assert_eq!(hot, Some(mid));
        assert!(arc.contours()[0].vertices[0].bulge.abs() > 0.1);
        assert_eq!(actions(&arc, mid)[2], Action::ToLine);
        // Splitting the arc keeps its shape: the new vertex is on it and both halves bulge the same way.
        let before = segment_mid(&arc.contours()[0], 0);
        let (split, _) = apply(&arc, mid, Action::AddVertex).unwrap();
        let c = &split.contours()[0];
        assert!(distance([c.vertices[1].x, c.vertices[1].y], before) < 1e-9);
        assert!((c.length() - arc.contours()[0].length()).abs() < 1e-6);
        let (back, hot) = apply(&arc, mid, Action::ToLine).unwrap();
        assert_eq!((back, hot), (line, None));
    }
}
