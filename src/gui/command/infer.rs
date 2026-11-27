//! INFER, as in AutoCAD: what is drawn on a snap stays bound to what it snapped to (an end, a
//! centre, a point on a line), LINE segments stay chained, rectangles and ORTHO lines stay level and
//! plumb, and a segment drawn along a tracking path keeps to where the path came from (square to,
//! parallel to or in line with a segment; level or plumb along POLAR). Added in the drawing's undo
//! step; dropped when they would not hold together.

use super::App;
use crate::gui::grips::Grip;
use crate::gui::snap::{self, Snap};
use crate::gui::tools::Tool;
use crate::gui::tracking::{Path, Source, Track};
use crate::model::constraints::attach::{Along, Link, along_constraints, axis_constraints, bind, link_constraints, rect_constraints};
use crate::model::{Kind, Point, Ref};

/// What a segment drawn along tracking path `t` through `p` keeps.
fn along(t: &Track, p: Point) -> Option<Along> {
    let kind = match (t.source, t.path) {
        (Source::Polar | Source::Ortho, Path::Line { ang, .. }) => {
            let a = ang.rem_euclid(180.0);
            return [(0.0, Kind::Horizontal), (90.0, Kind::Vertical), (180.0, Kind::Horizontal)]
                .into_iter()
                .find(|(x, _)| (a - x).abs() < 1e-9)
                .map(|(_, kind)| Along { p, kind, target: None });
        }
        (Source::Perpendicular, _) => Kind::Perpendicular,
        (Source::Parallel, _) => Kind::Parallel,
        (Source::Extension, Path::Line { .. }) => Kind::Collinear,
        (Source::Extension, Path::Circle { .. }) => Kind::Concentric,
        _ => return None,
    };
    Some(Along { p, kind, target: Some(t.from?) })
}

impl App {
    /// Remember the snap point or tracking path(s) `p` was placed with.
    pub(in crate::gui) fn note_snap(&mut self, p: Point) {
        if let Some(hit) = self.aids.hit
            && hit.p == p
            && let Some((kind, target)) = hit.infer()
        {
            self.draft_links.push(Link { p, kind, target });
        }
        if let Some(l) = self.aids.tracking.as_ref().filter(|l| l.p == p) {
            self.draft_along.extend(l.tracks.iter().filter_map(|t| along(t, p)));
            // On an arc's circle (Extension): the point stays on it, or a new circle touches it.
            for t in &l.tracks {
                if let (Path::Circle { .. }, Some(target)) = (t.path, t.from) {
                    self.draft_links.push(Link { p, kind: Kind::Coincident, target });
                }
            }
        }
    }

    /// Constraints for item `id`, just drawn from the points `pts`.
    pub(super) fn infer_drawn(&mut self, id: u64, pts: &[Point]) {
        let links: Vec<Link> = std::mem::take(&mut self.draft_links).into_iter().filter(|l| pts.contains(&l.p)).collect();
        let along: Vec<Along> = std::mem::take(&mut self.draft_along).into_iter().filter(|l| pts.contains(&l.p)).collect();
        let typed = std::mem::take(&mut self.draft_typed);
        let mut d = self.doc.lock();
        let own = self.own_constraints(&d.s, id, &typed);
        if !self.aids.infer {
            d.constrain_new(own, Vec::new());
            return;
        }
        let mut found = link_constraints(&d.s, id, &links);
        if self.tool == Tool::Rect {
            found.extend(rect_constraints(&d.s, id));
        } else if self.aids.ortho && matches!(self.tool, Tool::Line | Tool::Polyline) {
            found.extend(axis_constraints(&d.s, id));
        }
        for c in along_constraints(&d.s, id, &along) {
            if !found.contains(&c) {
                found.push(c);
            }
        }
        d.constrain_new(own, found);
    }

    /// LINE goes on from the end of segment `id`: the next segment starts joined to it.
    pub(super) fn chain_from(&mut self, id: u64, end: Point) {
        self.draft = vec![end];
        self.draft_links = vec![Link { p: end, kind: Kind::Coincident, target: Ref::Point { item: id, contour: 0, vertex: 1 } }];
        self.draft_along.clear();
    }

    /// A vertex or circle centre grip let go on an end or centre of another item: joined to it.
    pub(in crate::gui) fn infer_grip(&mut self, grip: Grip, hit: Option<Snap>) {
        let new = match grip {
            Grip::Vertex { id, contour, vertex } => Ref::Point { item: id, contour, vertex },
            Grip::Centre { id, contour } => Ref::Centre { item: id, contour, segment: 0 },
            _ => return,
        };
        let Some(Snap { kind: snap::Kind::Endpoint | snap::Kind::Center, target: Some(target), .. }) = hit else { return };
        if self.aids.infer && target.item() != new.item() {
            let (kind, refs) = bind(new, target);
            self.doc.lock().infer_new(vec![(kind, refs)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gui::App;
    use crate::gui::command::Cmd;
    use crate::model::geom::distance;
    use crate::model::{Contour, Document, Kind, Op, Point, Ref, Shape, Transform};
    use parking_lot::Mutex;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// Draws a LINE with the pointer: rests on `acquire` (OTRACK, Extension, Parallel), clicks
    /// `start`, then clicks near `end` (where a tracking path should lock). Returns the document,
    /// the source line (0,0)-(40,30), the new line and where the end landed.
    fn draw_along(acquire: Point, start: Point, end: Point) -> (Arc<Mutex<Document>>, u64, u64, Point) {
        let doc = Arc::new(Mutex::new(Document::new(200.0, 200.0)));
        let a = doc.lock().add("", Op::Cut, Shape::path(Contour::line([0.0, 0.0], [40.0, 30.0])));
        let mut app = App::new(crate::model::Workspace::from_shared(doc.clone()), Err(String::new()));
        (app.aids.osnap, app.aids.otrack, app.aids.infer, app.aids.polar, app.aids.ortho) = (true, true, true, false, false);
        let items = doc.lock().s.items.clone();
        app.start(Cmd::Line);
        let t0 = Instant::now();
        let aim = |app: &mut App, p: Point, from: Option<Point>, ms: u64| {
            app.aids.hit = None;
            app.aids.tracking = None;
            app.aim_at_time(&items, p, from, t0 + Duration::from_millis(ms))
        };
        aim(&mut app, acquire, None, 0);
        aim(&mut app, acquire, None, 600);
        let p = aim(&mut app, start, None, 700);
        app.place_point(p);
        let q = aim(&mut app, end, Some(p), 800);
        app.place_point(q);
        let b = doc.lock().s.items.last().unwrap().id;
        (doc, a, b, q)
    }

    fn seg(item: u64) -> Ref {
        Ref::Segment { item, contour: 0, segment: 0 }
    }

    fn has(doc: &Document, kind: Kind, refs: Vec<Ref>) -> bool {
        doc.s.constraints.iter().any(|c| c.kind == kind && c.refs == refs)
    }

    fn direction(doc: &Document, id: u64) -> Point {
        let v = &doc.item(id).unwrap().shape.contours()[0].vertices;
        let (dx, dy) = (v[1].x - v[0].x, v[1].y - v[0].y);
        [dx / dx.hypot(dy), dy / dx.hypot(dy)]
    }

    #[test]
    fn a_line_along_a_perpendicular_track_stays_square() {
        // Square to (0,0)-(40,30) at its end runs along (-0.6, 0.8).
        let (doc, a, b, q) = draw_along([40.2, 30.1], [40.1, 30.1], [25.24, 50.18]);
        assert!(distance(q, [25.0, 50.0]) < 1e-6, "{q:?}");
        let mut d = doc.lock();
        assert!(has(&d, Kind::Perpendicular, vec![seg(a), seg(b)]), "{:?}", d.s.constraints);
        d.transform(&[a], &Transform { rotate: 25.0, pivot: Some([0.0, 0.0]), ..Transform::default() });
        d.settle(true).unwrap();
        let (u, v) = (direction(&d, a), direction(&d, b));
        assert!((u[0] * v[0] + u[1] * v[1]).abs() < 1e-6, "{u:?} {v:?}");
    }

    #[test]
    fn a_line_along_a_parallel_track_stays_parallel() {
        let (doc, a, b, q) = draw_along([10.1, 7.6], [60.0, 0.0], [79.82, 15.24]);
        assert!(distance(q, [80.0, 15.0]) < 1e-6, "{q:?}");
        let d = doc.lock();
        assert!(has(&d, Kind::Parallel, vec![seg(a), seg(b)]), "{:?}", d.s.constraints);
    }

    #[test]
    fn a_line_along_an_extension_is_collinear() {
        let (doc, a, b, q) = draw_along([40.2, 30.1], [40.1, 30.1], [79.82, 60.24]);
        assert!(distance(q, [80.0, 60.0]) < 1e-6, "{q:?}");
        let d = doc.lock();
        assert!(has(&d, Kind::Collinear, vec![seg(a), seg(b)]), "{:?}", d.s.constraints);
        assert!(!has(&d, Kind::Perpendicular, vec![seg(a), seg(b)]));
    }
}
