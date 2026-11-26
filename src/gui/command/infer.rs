//! INFER, as in AutoCAD: what is drawn on a snap stays bound to what it snapped to (an end, a
//! centre, a point on a line), LINE segments stay chained, rectangles and ORTHO lines stay level and
//! plumb. Added in the drawing's undo step; dropped when they would not hold together.

use super::App;
use crate::gui::grips::Grip;
use crate::gui::snap::{self, Snap};
use crate::gui::tools::Tool;
use crate::model::constraints::attach::{Link, axis_constraints, bind, link_constraints};
use crate::model::{Kind, Point, Ref};

impl App {
    /// Remember the snap point `p` was placed with, if it came from one.
    pub(in crate::gui) fn note_snap(&mut self, p: Point) {
        if let Some(hit) = self.aids.hit
            && hit.p == p
            && let Some((kind, target)) = hit.infer()
        {
            self.draft_links.push(Link { p, kind, target });
        }
    }

    /// Constraints for item `id`, just drawn from the points `pts`.
    pub(super) fn infer_drawn(&mut self, id: u64, pts: &[Point]) {
        let links: Vec<Link> = std::mem::take(&mut self.draft_links).into_iter().filter(|l| pts.contains(&l.p)).collect();
        if !self.aids.infer {
            return;
        }
        let mut d = self.doc.lock().unwrap();
        let mut found = link_constraints(&d.s, id, &links);
        if self.tool == Tool::Rect || (self.aids.ortho && matches!(self.tool, Tool::Line | Tool::Polyline)) {
            found.extend(axis_constraints(&d.s, id));
        }
        d.infer_new(found);
    }

    /// LINE goes on from the end of segment `id`: the next segment starts joined to it.
    pub(super) fn chain_from(&mut self, id: u64, end: Point) {
        self.draft = vec![end];
        self.draft_links = vec![Link { p: end, kind: Kind::Coincident, target: Ref::Point { item: id, contour: 0, vertex: 1 } }];
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
            self.doc.lock().unwrap().infer_new(vec![(kind, refs)]);
        }
    }
}
