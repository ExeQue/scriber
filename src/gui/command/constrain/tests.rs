//! The constraint commands driven like the user does: clicks, typed values, label edits.

use super::*;
use crate::model::{Contour, Op, Shape};
use parking_lot::Mutex;
use std::sync::Arc;

fn app() -> (App, Arc<Mutex<Document>>) {
    let doc = Arc::new(Mutex::new(Document::new(200.0, 200.0)));
    let mut app = App::new(crate::model::Workspace::from_shared(doc.clone()), Err(String::new()));
    app.view.zoom = 4.0;
    (app, doc)
}

fn add(doc: &Arc<Mutex<Document>>, c: Contour) -> u64 {
    doc.lock().add("", Op::Cut, Shape::path(c))
}

#[test]
fn picking_two_lines_makes_them_parallel() {
    let (mut app, doc) = app();
    let a = add(&doc, Contour::line([0.0, 0.0], [100.0, 0.0]));
    let b = add(&doc, Contour::line([0.0, 20.0], [100.0, 40.0]));
    app.start(Cmd::find("GCPARALLEL").unwrap());
    assert_eq!(app.prompt(), "Select first object:");
    app.place_point([50.0, 0.2]);
    assert_eq!(app.prompt(), "Select second object:");
    app.place_point([50.0, 30.0]);
    assert!(app.cmdline.active.is_none());
    let d = doc.lock();
    assert_eq!(d.s.constraints.len(), 1);
    let v = &d.item(b).unwrap().shape.contours()[0].vertices;
    assert!((v[1].y - v[0].y).abs() < 1e-6, "the second line turned level: {v:?}");
    assert_eq!(d.item(a).unwrap().shape.contours()[0].vertices[1].y, 0.0);
}

#[test]
fn dclinear_places_a_driving_dimension_and_takes_a_value() {
    let (mut app, doc) = app();
    let r = add(&doc, Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0));
    app.start(Cmd::find("DCLINEAR").unwrap());
    app.place_point([10.5, 10.5]);
    app.place_point([49.5, 10.5]);
    assert_eq!(app.prompt(), "Specify dimension line location:");
    app.place_point([30.0, 2.0]);
    assert_eq!(app.prompt(), "Enter value or expression <d1=40>:");
    app.cmdline.input = "width=60".into();
    app.submit(None);
    assert!(app.cmdline.active.is_none());
    {
        let d = doc.lock();
        let c = d.parameter("width").unwrap();
        assert_eq!((c.kind, c.offset), (Kind::DimHorizontal, -8.0));
        let v = &d.item(r).unwrap().shape.contours()[0].vertices;
        assert!(((v[1].x - v[0].x) - 60.0).abs() < 1e-6, "{v:?}");
    }
    // Double-clicking its label asks again; each edit is one undo step.
    let id = doc.lock().parameter("width").unwrap().id;
    app.edit_dimension(id);
    app.cmdline.input = "45".into();
    app.submit(None);
    let width = |doc: &Arc<Mutex<Document>>| {
        let d = doc.lock();
        let v = &d.item(r).unwrap().shape.contours()[0].vertices;
        (v[1].x - v[0].x).round()
    };
    assert_eq!(width(&doc), 45.0);
    doc.lock().undo();
    assert_eq!(width(&doc), 60.0);
}

#[test]
fn a_dimension_snapped_to_two_points_drives_them() {
    let (mut app, doc) = app();
    let l = add(&doc, Contour::line([0.0, 0.0], [30.0, 40.0]));
    assert!(app.associative_dimension([0.0, 0.0], [30.0, 40.0]));
    assert!(!app.associative_dimension([0.0, 0.0], [31.0, 40.0]), "a free end makes a plain note");
    let id = doc.lock().parameter("d1").unwrap().id;
    assert_eq!(doc.lock().constraint(id).unwrap().expr, "50");
    app.edit_dimension(id);
    app.cmdline.input = "100".into();
    app.submit(None);
    assert!((doc.lock().item(l).unwrap().shape.metrics().length - 100.0).abs() < 1e-6);
}

#[test]
fn a_line_chain_stays_connected_and_snaps_join() {
    let (mut app, doc) = app();
    let other = add(&doc, Contour::line([0.0, 100.0], [30.0, 100.0]));
    app.start(Cmd::Line);
    app.aids.hit = Some(crate::gui::snap::Snap { p: [30.0, 100.0], kind: crate::gui::snap::Kind::Endpoint, target: Some(Ref::Point { item: other, contour: 0, vertex: 1 }) });
    app.place_point([30.0, 100.0]);
    app.aids.hit = None;
    app.place_point([50.0, 100.0]);
    app.place_point([50.0, 140.0]);
    app.cancel();
    let mut d = doc.lock();
    assert_eq!(d.s.constraints.len(), 2, "{:?}", d.s.constraints);
    // Dragging the first line's start drags the chain along.
    let first = d.s.items[1].id;
    d.transform(&[other], &crate::model::Transform::translate(0.0, -10.0));
    d.settle(true).unwrap();
    let v = |d: &Document, id: u64, k: usize| {
        let v = d.item(id).unwrap().shape.contours()[0].vertices[k];
        [v.x, v.y]
    };
    let second = d.s.items[2].id;
    assert_eq!(v(&d, first, 0), [30.0, 90.0]);
    assert_eq!(v(&d, second, 0), v(&d, first, 1));
}

#[test]
fn selected_lines_are_made_parallel_in_one_step() {
    let (mut app, doc) = app();
    let a = add(&doc, Contour::line([0.0, 0.0], [100.0, 0.0]));
    let b = add(&doc, Contour::line([0.0, 20.0], [100.0, 40.0]));
    doc.lock().selection = vec![a, b];
    app.start(Cmd::find("GCPARALLEL").unwrap());
    assert!(app.cmdline.active.is_none(), "no picks asked");
    let mut d = doc.lock();
    assert_eq!(d.s.constraints.len(), 1);
    let v = &d.item(b).unwrap().shape.contours()[0].vertices;
    assert!((v[1].y - v[0].y).abs() < 1e-6);
    d.undo();
    assert!(d.s.constraints.is_empty());
    assert_eq!(d.item(b).unwrap().shape.contours()[0].vertices[1].y, 40.0);
}

#[test]
fn right_click_add_constraint_uses_that_shape() {
    let (mut app, doc) = app();
    let a = add(&doc, Contour::line([0.0, 0.0], [100.0, 0.0]));
    let b = add(&doc, Contour::line([0.0, 20.0], [100.0, 40.0]));
    doc.lock().selection = vec![a];
    app.canvas_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    app.right_click(app.w2s([50.0, 30.0]));
    assert_eq!(doc.lock().selection, vec![a, b]);
    app.start(Cmd::Constrain(Kind::Parallel));
    assert_eq!(doc.lock().s.constraints.len(), 1);
}
