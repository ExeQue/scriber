//! Plain-language names and help texts for laser operations, shared by the panels and the canvas.

use crate::model::{Item, Op, State};
use eframe::egui::Color32;

pub fn op_label(op: Op) -> &'static str {
    op.label()
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

/// The colour of what the laser does, made to read on the theme's material.
pub fn op_color(op: Op) -> Color32 {
    super::theme::get().op(op)
}

pub fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// An item's colour on the canvas: its layer's colour, else the colour of what the laser does with it
/// (an open shape on an Engrave layer shows as scored).
pub fn item_color(s: &State, item: &Item) -> Color32 {
    s.layer(item.layer).and_then(|l| l.color).map_or(op_color(s.effective_op(item)), rgb)
}
