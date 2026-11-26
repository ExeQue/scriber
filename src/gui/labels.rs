//! Plain-language names and help texts for laser operations, shared by the panels and the canvas.

use crate::model::Op;
use eframe::egui::Color32;

pub fn op_label(op: Op) -> &'static str {
    match op {
        Op::Cut => "Cut",
        Op::Score => "Score",
        Op::Engrave => "Engrave line",
        Op::NoLine => "No line",
        Op::Annotate => "Note only",
    }
}

pub fn op_help(op: Op) -> &'static str {
    match op {
        Op::Cut => "The laser cuts all the way through along the line.",
        Op::Score => "The laser marks a thin line without cutting through.",
        Op::Engrave => "The laser burns the line into the surface.",
        Op::NoLine => "The laser ignores the line. Use with Fill to only engrave the area.",
        Op::Annotate => "Only for you: notes and measurements. The laser ignores it.",
    }
}

pub fn fill_help() -> &'static str {
    "Fill: the laser engraves the whole area inside the shape. Holes in combined shapes stay empty."
}

pub fn op_color(op: Op) -> Color32 {
    let [r, g, b] = op.rgb();
    Color32::from_rgb(r, g, b)
}
