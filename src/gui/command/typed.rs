//! Sizes typed into dynamic input stay: like AutoCAD's dynamic dimensional constraints, a length,
//! radius, width or height typed while drawing becomes a driving dimension of the new shape (an
//! angle of 0° or 90° becomes horizontal or vertical). Values that followed the mouse do not. A
//! polygon also keeps its equal sides and corners.

use super::dynamic::Kind as Field;
use super::{App, Cmd};
use crate::gui::tools::Tool;
use crate::model::{Kind, Ref, Shape, State};

/// What was typed for the draft's point `at`.
#[derive(Clone, Copy, Debug)]
pub struct Typed {
    field: Field,
    values: [f64; 2],
    locked: [bool; 2],
    diameter: bool,
    at: usize,
}

/// Horizontal or vertical for an angle on an axis.
fn axis(angle: f64) -> Option<Kind> {
    let a = angle.rem_euclid(180.0);
    if a < 1e-9 || 180.0 - a < 1e-9 {
        Some(Kind::Horizontal)
    } else if (a - 90.0).abs() < 1e-9 {
        Some(Kind::Vertical)
    } else {
        None
    }
}

impl App {
    /// Remember the locked dynamic-input values of the point about to be placed.
    pub(super) fn note_typed(&mut self) {
        let Some(field) = self.dyn_kind() else { return };
        let d = &self.cmdline.dynamic;
        if !d.locked.contains(&true) {
            return;
        }
        let at = self.draft.len();
        let t = Typed { field, values: d.values, locked: d.locked, diameter: d.diameter, at };
        // A point taken back (PLINE Undo) and placed again replaces what was typed for it.
        self.draft_typed.retain(|t| t.at < at);
        self.draft_typed.push(t);
    }

    /// The constraints new item `id` is made with: a dimension for each typed size, and a
    /// polygon's regularity.
    pub(super) fn own_constraints(&self, s: &State, id: u64, typed: &[Typed]) -> Vec<(Kind, Vec<Ref>)> {
        let Some(Shape::Path { contours }) = s.items.iter().find(|i| i.id == id).map(|i| &i.shape) else { return Vec::new() };
        let n = contours[0].vertices.len();
        let seg = |segment| Ref::Segment { item: id, contour: 0, segment };
        let pt = |vertex| Ref::Point { item: id, contour: 0, vertex };
        let mut out = Vec::new();
        if self.tool == Tool::Polygon {
            out.push((Kind::Regular, vec![Ref::Item { item: id }]));
        }
        for t in typed {
            let [first, second] = t.locked;
            let mut add = |on: bool, kind: Option<Kind>, refs: Vec<Ref>| {
                if let Some(kind) = kind.filter(|_| on) {
                    out.push((kind, refs));
                }
            };
            match (self.active_cmd(), t.field) {
                (Some(Cmd::Line | Cmd::Pline), Field::LengthAngle) if t.at > 0 => {
                    add(first, Some(Kind::DimAligned), vec![seg(t.at - 1)]);
                    add(second, axis(t.values[1]), vec![seg(t.at - 1)]);
                }
                // Rounded corners leave no straight side as long as the size: no dimension.
                (Some(Cmd::Rectang), _) if n != 4 => {}
                (Some(Cmd::Rectang), Field::Size) => {
                    add(first, Some(Kind::DimHorizontal), vec![seg(0)]);
                    add(second, Some(Kind::DimVertical), vec![seg(1)]);
                }
                (Some(Cmd::Rectang), Field::LengthAngle) => {
                    add(first, Some(Kind::DimAligned), vec![seg(0)]);
                    add(second, axis(t.values[1]), vec![seg(0)]);
                }
                (Some(Cmd::Rectang), Field::Height) => add(first, Some(Kind::DimAligned), vec![seg(1)]),
                (Some(Cmd::Circle | Cmd::Arc), Field::Radius { .. }) => add(first, Some(if t.diameter { Kind::DimDiameter } else { Kind::DimRadial }), vec![seg(0)]),
                // The typed size is across corners or flats; with an odd number of sides there is
                // no opposite corner or side, so the side length is locked instead.
                (Some(Cmd::Polygon), Field::Polygon) if n % 2 == 1 => add(first, Some(Kind::DimAligned), vec![seg(0)]),
                (Some(Cmd::Polygon), Field::Polygon) if self.cmdline.polygon_flat => add(first, Some(Kind::DimAligned), vec![pt(0), seg(n / 2)]),
                (Some(Cmd::Polygon), Field::Polygon) => add(first, Some(Kind::DimAligned), vec![pt(0), pt(n / 2)]),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::gui::App;
    use crate::gui::command::Cmd;
    use crate::model::geom::distance;
    use crate::model::{Document, Kind, Point};
    use std::sync::{Arc, Mutex};

    fn app() -> (Arc<Mutex<Document>>, App) {
        let doc = Arc::new(Mutex::new(Document::new(200.0, 200.0)));
        let mut app = App::new(doc.clone(), Err(String::new()));
        (app.aids.osnap, app.aids.otrack, app.aids.infer, app.aids.polar, app.aids.ortho) = (false, false, true, false, false);
        (doc, app)
    }

    /// Type `values` into the fields marked in `locked`, then click `p`.
    fn typed(app: &mut App, values: [f64; 2], locked: [bool; 2], p: Point) {
        let d = &mut app.cmdline.dynamic;
        (d.values, d.locked) = (values, locked);
        app.place_point(p);
    }

    fn vertices(d: &Document) -> Vec<Point> {
        d.s.items.last().unwrap().shape.contours()[0].vertices.iter().map(|v| [v.x, v.y]).collect()
    }

    /// The new item's constraints as (kind, value), after checking that settling moves nothing.
    fn constraints(doc: &Arc<Mutex<Document>>) -> Vec<(Kind, String)> {
        let mut d = doc.lock().unwrap();
        let before = vertices(&d);
        d.settle(true).unwrap();
        assert_eq!(vertices(&d), before);
        d.s.constraints.iter().map(|c| (c.kind, c.expr.clone())).collect()
    }

    #[test]
    fn a_typed_line_length_and_level_angle_stay() {
        let (doc, mut app) = app();
        app.start(Cmd::Line);
        app.place_point([10.0, 10.0]);
        typed(&mut app, [40.0, 0.0], [true, true], [50.0, 30.0]);
        assert_eq!(constraints(&doc), vec![(Kind::DimAligned, "40".into()), (Kind::Horizontal, String::new())]);
        let mut d = doc.lock().unwrap();
        d.set_parameter("d1", "55").unwrap();
        d.settle(true).unwrap();
        let v = vertices(&d);
        assert!((distance(v[0], v[1]) - 55.0).abs() < 1e-6 && (v[0][1] - v[1][1]).abs() < 1e-9, "{v:?}");
    }

    #[test]
    fn mouse_values_and_other_angles_get_no_dimension() {
        let (doc, mut app) = app();
        app.start(Cmd::Line);
        app.place_point([10.0, 10.0]);
        typed(&mut app, [0.0, 30.0], [false, true], [50.0, 30.0]);
        assert_eq!(constraints(&doc), vec![]);
    }

    #[test]
    fn a_typed_rectangle_width_stays_when_a_corner_is_dragged() {
        let (doc, mut app) = app();
        app.start(Cmd::Rectang);
        app.place_point([0.0, 0.0]);
        typed(&mut app, [30.0, 0.0], [true, false], [50.0, 20.0]);
        let kinds: Vec<Kind> = constraints(&doc).into_iter().map(|c| c.0).collect();
        assert_eq!(kinds.iter().filter(|k| **k == Kind::DimHorizontal).count(), 1, "{kinds:?}");
        assert!(!kinds.contains(&Kind::DimVertical));
        let mut d = doc.lock().unwrap();
        let id = d.s.items.last().unwrap().id;
        let crate::model::Shape::Path { contours } = &mut d.item_mut(id).unwrap().shape else { panic!() };
        (contours[0].vertices[2].x, contours[0].vertices[2].y) = (45.0, 25.0);
        d.touch();
        d.settle(true).unwrap();
        // A rectangle may turn (only its corners are square), but the typed width holds.
        let v = &d.item(id).unwrap().shape.contours()[0].vertices;
        let side = |a: usize, b: usize| (v[a].x - v[b].x).hypot(v[a].y - v[b].y);
        assert!((side(0, 1) - 30.0).abs() < 1e-6 || (v[1].x - v[0].x - 30.0).abs() < 1e-6, "{v:?}");
        assert!((side(0, 1) * side(1, 2) - side(2, 3) * side(3, 0)).abs() < 1e-6, "still a rectangle: {v:?}");
    }

    #[test]
    fn a_typed_circle_diameter_becomes_a_diameter() {
        let (doc, mut app) = app();
        app.start(Cmd::Circle);
        app.place_point([50.0, 50.0]);
        app.cmdline.dynamic.diameter = true;
        typed(&mut app, [10.0, 0.0], [true, false], [70.0, 50.0]);
        assert_eq!(constraints(&doc), vec![(Kind::DimDiameter, "20".into())]);
    }

    #[test]
    fn a_polygon_stays_regular_and_keeps_its_typed_size() {
        let (doc, mut app) = app();
        app.sides = 6;
        app.start(Cmd::Polygon);
        app.place_point([50.0, 50.0]);
        typed(&mut app, [40.0, 0.0], [true, false], [90.0, 50.0]);
        assert_eq!(constraints(&doc), vec![(Kind::Regular, String::new()), (Kind::DimAligned, "40".into())]);
        let mut d = doc.lock().unwrap();
        let id = d.s.items.last().unwrap().id;
        let crate::model::Shape::Path { contours } = &mut d.item_mut(id).unwrap().shape else { panic!() };
        contours[0].vertices[1].x += 5.0;
        d.touch();
        d.settle(true).unwrap();
        let v = vertices(&d);
        let side = distance(v[0], v[1]);
        assert!((0..6).all(|k| (distance(v[k], v[(k + 1) % 6]) - side).abs() < 1e-6 && (distance(v[k], v[(k + 2) % 6]) - side * 3f64.sqrt()).abs() < 1e-6), "{v:?}");
        assert!((distance(v[0], v[3]) - 40.0).abs() < 1e-6, "{v:?}");
    }

    #[test]
    fn a_polygon_drawn_with_the_mouse_is_regular_without_a_size() {
        let (doc, mut app) = app();
        app.start(Cmd::Polygon);
        app.place_point([50.0, 50.0]);
        app.place_point([70.0, 55.0]);
        assert_eq!(constraints(&doc), vec![(Kind::Regular, String::new())]);
    }
}
