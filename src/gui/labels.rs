//! Plain-language names and help texts for laser operations, shared by the panels and the canvas.

use crate::model::Op;
use eframe::egui::Color32;

pub fn op_label(op: Op) -> &'static str {
    match op {
        Op::Cut => "Cut",
        Op::Score => "Score",
        Op::Engrave => "Engrave",
        Op::NoOutput => "No output",
    }
}

pub fn op_help(op: Op) -> &'static str {
    match op {
        Op::Cut => "The laser cuts all the way through along the line.",
        Op::Score => "The laser marks a thin line without cutting through.",
        Op::Engrave => "Engrave: the laser burns the whole area inside closed shapes (holes stay empty); open lines are scored.",
        Op::NoOutput => "No output: shown on the drawing only — notes, measurements, helpers; the laser skips it.",
    }
}

/// Shown when Engrave is chosen for a shape without an area.
pub const OPEN_SCORED: &str = "Open shapes are scored";

pub fn op_color(op: Op) -> Color32 {
    let [r, g, b] = op.rgb();
    Color32::from_rgb(r, g, b)
}
