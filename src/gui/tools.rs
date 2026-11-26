//! The canvas tools: their names, help texts, and the shape the placed points make. Commands
//! (see `command.rs`) switch between them.

use crate::model::geom::distance;
use crate::model::{Contour, Point, Shape};
use super::command::Cmd;

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
    /// Points for an editing command (base point, second point); not on the toolbar.
    Pick,
}

impl Tool {
    pub const ALL: [Tool; 10] = [
        Tool::Select,
        Tool::Line,
        Tool::Rect,
        Tool::Circle,
        Tool::Arc,
        Tool::Polygon,
        Tool::Polyline,
        Tool::Text,
        Tool::Dimension,
        Tool::Measure,
    ];

    pub fn icon(self) -> &'static str {
        match self {
            Tool::Select => "⬉",
            Tool::Line => "╱",
            Tool::Rect => "▭",
            Tool::Circle => "◯",
            Tool::Arc => "◠",
            Tool::Polygon => "⬡",
            Tool::Polyline => "〰",
            Tool::Text => "T",
            Tool::Dimension => "↔",
            Tool::Measure => "📏",
            Tool::Pick => "+",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Line => "Line",
            Tool::Rect => "Rectangle",
            Tool::Circle => "Circle",
            Tool::Arc => "Arc",
            Tool::Polygon => "Polygon",
            Tool::Polyline => "Polyline",
            Tool::Text => "Text",
            Tool::Dimension => "Dimension",
            Tool::Measure => "Measure",
            Tool::Pick => "Point",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Tool::Select => "Click a shape to select it, drag to move it. Drag on empty space to select several.",
            Tool::Line => "A straight line between two points.",
            Tool::Rect => "A rectangle from one corner to the opposite corner.",
            Tool::Circle => "A circle from its centre out to the edge.",
            Tool::Arc => "Part of a circle: centre, start, end.",
            Tool::Polygon => "A shape with equal sides, e.g. a hexagon.",
            Tool::Polyline => "Connected lines. Double-click or press Enter to finish; click the first point to close it.",
            Tool::Text => "Text you can engrave or cut.",
            Tool::Dimension => "A measurement shown on the drawing.",
            Tool::Measure => "Measure a distance without drawing anything.",
            Tool::Pick => "A point for the running command.",
        }
    }

    /// The command a toolbar button runs, as shown in its tooltip.
    pub fn command(self) -> String {
        Cmd::for_tool(self).map_or("Esc".into(), |c| format!("{} ({})", c.name(), c.alias()))
    }

    /// What to do next, given how many points are placed.
    pub fn hint(self, placed: usize) -> &'static str {
        match (self, placed) {
            (Tool::Select, _) => "Click shapes to add them, Shift-click to remove. Drag on empty space for a window (left to right) or crossing (right to left). Drag grips to edit. Type a command below.",
            (Tool::Rect, 0) => "Click (or press and drag) the first corner.",
            (Tool::Rect, _) => "Click the opposite corner.",
            (Tool::Circle | Tool::Polygon, 0) => "Click the centre.",
            (Tool::Circle | Tool::Polygon, _) => "Click the edge to set the size.",
            (Tool::Arc, 0) => "Click the centre of the arc.",
            (Tool::Arc, 1) => "Click where the arc starts.",
            (Tool::Arc, _) => "Click where the arc ends (it runs clockwise).",
            (Tool::Polyline, 0) => "Click the first point.",
            (Tool::Polyline, _) => "Click the next point. Double-click or Enter finishes, Esc cancels.",
            (Tool::Text, _) => "Click where the text should start, then give its height and the text on the command line.",
            (Tool::Line | Tool::Dimension | Tool::Measure, 0) => "Click (or press and drag) the start point.",
            (Tool::Line | Tool::Dimension | Tool::Measure, _) => "Click the end point.",
            (Tool::Pick, _) => "Click a point, or type coordinates on the command line.",
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
