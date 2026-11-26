//! The constraint commands driven like the user does: clicks, typed values, label edits.

use super::*;
use crate::model::{Contour, Op, Shape};
use std::sync::{Arc, Mutex};

fn app() -> (App, Arc<Mutex<Document>>) {
    let doc = Arc::new(Mutex::new(Document::new(200.0, 200.0)));
    let mut app = App::new(doc.clone(), Err(String::new()));
    app.view.zoom = 4.0;
    (app, doc)
}

fn add(doc: &Arc<Mutex<Document>>, c: Contour) -> u64 {
    doc.lock().unwrap().add("", Op::Cut, false, Shape::path(c))
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
    let d = doc.lock().unwrap();
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
        let d = doc.lock().unwrap();
        let c = d.parameter("width").unwrap();
        assert_eq!((c.kind, c.offset), (Kind::DimHorizontal, -8.0));
        let v = &d.item(r).unwrap().shape.contours()[0].vertices;
        assert!(((v[1].x - v[0].x) - 60.0).abs() < 1e-6, "{v:?}");
    }
    // Double-clicking its label asks again; each edit is one undo step.
    let id = doc.lock().unwrap().parameter("width").unwrap().id;
    app.edit_dimension(id);
    app.cmdline.input = "45".into();
    app.submit(None);
    let width = |doc: &Arc<Mutex<Document>>| {
        let d = doc.lock().unwrap();
        let v = &d.item(r).unwrap().shape.contours()[0].vertices;
        (v[1].x - v[0].x).round()
    };
    assert_eq!(width(&doc), 45.0);
    doc.lock().unwrap().undo();
    assert_eq!(width(&doc), 60.0);
}

#[test]
fn a_dimension_snapped_to_two_points_drives_them() {
    let (mut app, doc) = app();
    let l = add(&doc, Contour::line([0.0, 0.0], [30.0, 40.0]));
    assert!(app.associative_dimension([0.0, 0.0], [30.0, 40.0]));
    assert!(!app.associative_dimension([0.0, 0.0], [31.0, 40.0]), "a free end makes a plain note");
    let id = doc.lock().unwrap().parameter("d1").unwrap().id;
    assert_eq!(doc.lock().unwrap().constraint(id).unwrap().expr, "50");
    app.edit_dimension(id);
    app.cmdline.input = "100".into();
    app.submit(None);
    assert!((doc.lock().unwrap().item(l).unwrap().shape.metrics().length - 100.0).abs() < 1e-6);
}
