//! DIM driven like the user does: clicks, typed values, repeats.

use super::*;
use crate::model::{Contour, Op, Shape};
use std::sync::{Arc, Mutex};

fn app() -> (App, Arc<Mutex<Document>>) {
    let doc = Arc::new(Mutex::new(Document::new(200.0, 200.0)));
    let mut app = App::new(crate::model::Workspace::from_shared(doc.clone()), Err(String::new()));
    app.view.zoom = 4.0;
    (app, doc)
}

fn add(doc: &Arc<Mutex<Document>>, c: Contour) -> u64 {
    doc.lock().unwrap().add("", Op::Cut, Shape::path(c))
}

fn dims(doc: &Arc<Mutex<Document>>) -> Vec<(Kind, String)> {
    doc.lock().unwrap().s.constraints.iter().map(|c| (c.kind, c.expr.clone())).collect()
}

#[test]
fn a_line_then_a_typed_value_drives_its_length_and_repeats() {
    let (mut app, doc) = app();
    let r = add(&doc, Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0));
    app.start(Cmd::find("DIM").unwrap());
    app.place_point([30.0, 10.0]);
    assert_eq!(app.prompt(), "Specify dimension location or select a second line for an angle:");
    // Typed in dynamic input (or on the card), then a click above places it.
    app.cmdline.smart_dim.value = "60".into();
    app.place_point([30.0, 2.0]);
    assert_eq!(dims(&doc), vec![(Kind::DimHorizontal, "60".into())]);
    let v = doc.lock().unwrap().item(r).unwrap().shape.contours()[0].vertices.clone();
    assert!(((v[1].x - v[0].x) - 60.0).abs() < 1e-6, "{v:?}");
    // Back to picking, for the next one.
    assert_eq!(app.prompt(), "Select a line, circle or arc, or specify first point:");
    // The left side, placed to its left, as it is (the top side's end moved; the left one stayed).
    app.place_point([10.0, 20.0]);
    app.place_point([0.0, 20.0]);
    assert_eq!(dims(&doc)[1], (Kind::DimVertical, "20".into()), "{} {v:?}", app.message);
    // Each dimension is its own undo step.
    doc.lock().unwrap().undo();
    assert_eq!(dims(&doc).len(), 1);
}

#[test]
fn circles_arcs_angles_and_points() {
    let (mut app, doc) = app();
    add(&doc, Contour::circle(100.0, 100.0, 10.0));
    add(&doc, Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0));
    app.start(Cmd::Dim);
    app.place_point([110.0, 100.0]);
    app.place_point([120.0, 90.0]);
    // The top side, then the right side: their angle.
    app.place_point([30.0, 10.0]);
    app.place_point([50.0, 20.0]);
    assert!(app.dim_is_angle());
    app.place_point([40.0, 15.0]);
    // Two corners placed beside them.
    app.place_point([10.0, 10.0]);
    assert_eq!(app.prompt(), "Specify second point or select a line:");
    app.place_point([10.0, 30.0]);
    app.place_point([0.0, 20.0]);
    let kinds: Vec<Kind> = dims(&doc).into_iter().map(|d| d.0).collect();
    assert_eq!(kinds, [Kind::DimDiameter, Kind::DimAngular, Kind::DimVertical]);
}

#[test]
fn says_so_when_already_locked_or_refused() {
    let (mut app, doc) = app();
    add(&doc, Contour::line([10.0, 10.0], [50.0, 10.0]));
    app.start(Cmd::Dim);
    app.place_point([30.0, 10.0]);
    app.place_point([30.0, 20.0]);
    // The same length again: already fixed, nothing added.
    app.place_point([30.0, 10.0]);
    app.place_point([30.0, 25.0]);
    assert_eq!(dims(&doc).len(), 1);
    assert!(app.message.contains("already fixed"), "{}", app.message);
    // A value that cannot be: refused, and still placing so another can be typed.
    add(&doc, Contour::circle(100.0, 100.0, 10.0));
    app.place_point([110.0, 100.0]);
    app.cmdline.smart_dim.value = "nope + 1".into();
    app.place_point([120.0, 100.0]);
    assert_eq!(dims(&doc).len(), 1);
    assert!(app.message.starts_with("Cannot make it nope + 1"), "{}", app.message);
    assert_eq!(app.cmdline.active.map(|a| a.1), Some(Step::Location));
}

#[test]
fn a_name_typed_on_the_card_names_the_dimension_and_its_label_drags() {
    let (mut app, doc) = app();
    add(&doc, Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0));
    app.start(Cmd::find("DIM").unwrap());
    app.place_point([30.0, 10.0]);
    app.cmdline.smart_dim.name = "width".into();
    app.place_point([30.0, 2.0]);
    let id = doc.lock().unwrap().parameter("width").expect("named width").id;
    assert!(app.cmdline.smart_dim.name.is_empty(), "the next one gets the next free name");
    // Dragging the label: grabbed 1 mm right of its anchor, let go at (60, -4).
    let drag = app.grab_label(&doc.lock().unwrap(), id, [31.0, 2.0]);
    let Some(crate::gui::select::Drag::Label { grab, .. }) = drag else { panic!("no label drag") };
    app.drag_label(id, [60.0 - grab[0], -4.0 - grab[1]]);
    let c = doc.lock().unwrap().constraint(id).unwrap().clone();
    assert_eq!((c.offset, c.shift), (-14.0, 29.0));
    // Its edit card (double-click on the label) renames it too.
    app.edit_dimension(id);
    assert_eq!(app.cmdline.constrain.name, "width");
    app.cmdline.constrain.name = "w2".into();
    app.cmdline.input = "45".into();
    app.submit(None);
    assert_eq!(doc.lock().unwrap().constraint(id).map(|c| (c.name.clone(), c.expr.clone())), Some(("w2".into(), "45".into())));
}
