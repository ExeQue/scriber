//! The canvas tools: hints and the shape the placed points make. Commands
//! (see `command.rs`) switch between them.

use crate::model::geom::distance;
use crate::model::{Contour, Point, Shape};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Line,
    Rect,
    Circle,
    Arc,
    Polygon,
    Polyline,
    Text,
    Dimension,
    Measure,
    /// Points for an editing command (base point, second point); not on the ribbon.
    Pick,
}

impl Tool {
    /// What to do next, given how many points are placed.
    pub fn hint(self, placed: usize) -> &'static str {
        match (self, placed) {
            (Tool::Select, _) => "Click shapes to add them, Shift-click to remove. Drag on empty space for a window (left to right) or crossing (right to left). Drag grips to edit. Pick a tool in the ribbon at the top to draw.",
            (Tool::Rect, 0) => "Click (or press and drag) the first corner.",
            (Tool::Rect, _) => "Click the opposite corner.",
            (Tool::Circle, _) => "Click the points of the circle; the card shows which point comes next and other ways to draw it.",
            (Tool::Polygon, 0) => "Click the centre.",
            (Tool::Polygon, _) => "Click the edge to set the size.",
            (Tool::Arc, _) => "Click the points of the arc; the card shows which point comes next and other ways to draw it.",
            (Tool::Polyline, 0) => "Click the first point.",
            (Tool::Polyline, _) => "Click the next point. Double-click or Enter finishes, Esc cancels.",
            (Tool::Text, _) => "Click where the text should start, then type the text and its height on the card.",
            (Tool::Line | Tool::Dimension | Tool::Measure, 0) => "Click (or press and drag) the start point.",
            (Tool::Line | Tool::Dimension | Tool::Measure, _) => "Click the end point.",
            (Tool::Pick, _) => "Click a point, or type an exact value on the card.",
        }
    }

    /// Points needed to finish; 0 = until the user finishes (or not a drawing tool).
    pub fn points(self) -> usize {
        match self {
            Tool::Select | Tool::Polyline | Tool::Pick => 0,
            Tool::Text => 1,
            Tool::Arc => 3,
            _ => 2,
        }
    }

    /// Arc, polyline, text and command points are placed by clicking; the others can also be dragged.
    pub fn clicks_only(self) -> bool {
        matches!(self, Tool::Arc | Tool::Polyline | Tool::Text | Tool::Pick)
    }

    /// The shape made by the placed points (the last one may be the pointer). Default name too.
    pub fn shape(self, pts: &[Point], sides: u32) -> Option<(&'static str, Shape)> {
        let ang = |c: Point, p: Point| (p[1] - c[1]).atan2(p[0] - c[0]).to_degrees();
        let c = match (self, pts) {
            (Tool::Line | Tool::Pick, [a, b, ..]) => ("line", Contour::line(*a, *b)),
            (Tool::Rect, [a, b, ..]) => ("rectangle", Contour::rect(a[0], a[1], b[0] - a[0], b[1] - a[1], 0.0)),
            (Tool::Circle, [c, e, ..]) => ("circle", Contour::circle(c[0], c[1], distance(*c, *e))),
            (Tool::Polygon, [c, e, ..]) => ("polygon", Contour::polygon(c[0], c[1], distance(*c, *e), sides, ang(*c, *e) + 90.0)),
            (Tool::Arc, [c, s, e, ..]) => ("arc", Contour::arc(c[0], c[1], distance(*c, *s), ang(*c, *s), ang(*c, *e))),
            (Tool::Arc, [c, s]) => ("arc", Contour::circle(c[0], c[1], distance(*c, *s))),
            (Tool::Polyline, pts) if pts.len() >= 2 => ("polyline", Contour::polyline(pts, false)),
            (Tool::Dimension, [a, b, ..]) => {
                return Some(("dimension", Shape::Dimension { x1: a[0], y1: a[1], x2: b[0], y2: b[1], offset: 5.0 }));
            }
            _ => return None,
        };
        Some((c.0, Shape::path(c.1)))
    }
}
