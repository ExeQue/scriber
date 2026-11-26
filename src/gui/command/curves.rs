//! The ways to draw arcs and circles, as in AutoCAD: ARC by 3 points, start–centre–end,
//! centre–start–end or start–end–radius; CIRCLE by centre and radius (or diameter), 2 points or
//! 3 points. The last way used is remembered.

use super::parse::{Entry, is_option, toward};
use super::{App, Cmd};
use crate::model::geom::{circle_3p, distance, rotate_point};
use crate::model::{Contour, Point, Shape};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ArcMode {
    /// Start, a point on the arc, end (AutoCAD's default).
    #[default]
    ThreePoint,
    StartCenterEnd,
    CenterStartEnd,
    StartEndRadius,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CircleMode {
    #[default]
    CenterRadius,
    /// Two ends of a diameter.
    TwoPoint,
    ThreePoint,
}

impl ArcMode {
    pub const ALL: [ArcMode; 4] = [ArcMode::ThreePoint, ArcMode::StartCenterEnd, ArcMode::CenterStartEnd, ArcMode::StartEndRadius];

    pub fn label(self) -> &'static str {
        match self {
            ArcMode::ThreePoint => "3 points",
            ArcMode::StartCenterEnd => "Start, centre, end",
            ArcMode::CenterStartEnd => "Centre, start, end",
            ArcMode::StartEndRadius => "Start, end, radius",
        }
    }
}

impl CircleMode {
    pub const ALL: [CircleMode; 3] = [CircleMode::CenterRadius, CircleMode::TwoPoint, CircleMode::ThreePoint];

    pub fn label(self) -> &'static str {
        match self {
            CircleMode::CenterRadius => "Centre and radius",
            CircleMode::TwoPoint => "2 points (diameter)",
            CircleMode::ThreePoint => "3 points",
        }
    }
}

impl App {
    /// How many points ARC/CIRCLE need in the current mode; None for other commands.
    pub(super) fn curve_points(&self, cmd: Cmd) -> Option<usize> {
        match cmd {
            Cmd::Arc => Some(3),
            Cmd::Rectang => Some(self.cmdline.rect_mode.points()),
            Cmd::Circle => Some(if self.cmdline.circle_mode == CircleMode::ThreePoint { 3 } else { 2 }),
            _ => None,
        }
    }

    pub(super) fn curve_prompt(&self, cmd: Cmd, placed: usize) -> Option<&'static str> {
        if cmd == Cmd::Rectang {
            return Some(self.rect_prompt(placed));
        }
        let text = match (cmd, placed) {
            (Cmd::Arc, n) => match (self.cmdline.arc_mode, n) {
                (ArcMode::ThreePoint, 0) => "Specify start point of arc or [Center]:",
                (ArcMode::ThreePoint, 1) => "Specify second point of arc or [Center/End]:",
                (ArcMode::ThreePoint, _) => "Specify end point of arc:",
                (ArcMode::StartCenterEnd, 0) => "Specify start point of arc:",
                (ArcMode::StartCenterEnd, 1) => "Specify center point of arc:",
                (ArcMode::CenterStartEnd, 0) => "Specify center point of arc:",
                (ArcMode::CenterStartEnd, 1) => "Specify start point of arc:",
                (ArcMode::StartCenterEnd | ArcMode::CenterStartEnd, _) => "Specify end point of arc (counter-clockwise) or an included angle:",
                (ArcMode::StartEndRadius, 0) => "Specify start point of arc:",
                (ArcMode::StartEndRadius, 1) => "Specify end point of arc:",
                (ArcMode::StartEndRadius, _) => "Specify radius of arc:",
            },
            (Cmd::Circle, n) => match (self.cmdline.circle_mode, n) {
                (CircleMode::CenterRadius, 0) => "Specify center point for circle or [3P/2P]:",
                (CircleMode::CenterRadius, _) => "Specify radius of circle or [Diameter]:",
                (CircleMode::TwoPoint, 0) => "Specify first end point of circle's diameter:",
                (CircleMode::TwoPoint, _) => "Specify second end point of circle's diameter:",
                (CircleMode::ThreePoint, 0) => "Specify first point on circle:",
                (CircleMode::ThreePoint, 1) => "Specify second point on circle:",
                (CircleMode::ThreePoint, _) => "Specify third point on circle:",
            },
            _ => return None,
        };
        Some(text)
    }

    /// Mode keywords typed on the command line (`C`, `E`, `3P`, `2P`). True when handled.
    pub(super) fn curve_option(&mut self, cmd: Cmd, word: &str) -> bool {
        let n = self.draft.len();
        let (arc, circle) = (&mut self.cmdline.arc_mode, &mut self.cmdline.circle_mode);
        match cmd {
            Cmd::Arc if n == 0 && is_option(word, "Center") => *arc = ArcMode::CenterStartEnd,
            Cmd::Arc if n == 1 && *arc == ArcMode::ThreePoint && is_option(word, "Center") => *arc = ArcMode::StartCenterEnd,
            Cmd::Arc if n == 1 && *arc == ArcMode::ThreePoint && is_option(word, "End") => *arc = ArcMode::StartEndRadius,
            Cmd::Circle if n == 0 && word == "3P" => *circle = CircleMode::ThreePoint,
            Cmd::Rectang if n == 0 && is_option(word, "Center") => self.cmdline.rect_mode = super::rects::RectMode::Center,
            Cmd::Rectang if n == 0 && word == "3P" => self.cmdline.rect_mode = super::rects::RectMode::ThreePoint,
            Cmd::Circle if n == 0 && word == "2P" => *circle = CircleMode::TwoPoint,
            _ => return false,
        }
        true
    }

    /// A bare number while drawing an arc: the included angle (start–centre–end, centre–start–end)
    /// or the radius (start–end–radius), turned into the point the shape builder expects.
    pub(super) fn curve_number(&self, cmd: Cmd, n: f64, pointer: Option<Point>) -> Option<Point> {
        if cmd != Cmd::Arc || self.draft.len() != 2 {
            return None;
        }
        let [a, b] = [self.draft[0], self.draft[1]];
        match self.cmdline.arc_mode {
            // Counter-clockwise on screen is a negative rotation in our y-down model.
            ArcMode::StartCenterEnd => Some(rotate_point(a, b, -n)),
            ArcMode::CenterStartEnd => Some(rotate_point(b, a, -n)),
            ArcMode::StartEndRadius => Some(toward(b, pointer, n)),
            ArcMode::ThreePoint => None,
        }
    }

    /// The shape the placed points make, in the current mode; other tools use `Tool::shape`.
    pub(in crate::gui) fn draft_shape(&self, pts: &[Point]) -> Option<(&'static str, Shape)> {
        let cmd = self.active_cmd();
        if cmd == Some(Cmd::Rectang) {
            return self.rect_shape(pts).map(|s| ("rectangle", s));
        }
        let c = match (cmd, pts) {
            (Some(Cmd::Arc), [a, b, c, ..]) => match self.cmdline.arc_mode {
                ArcMode::ThreePoint => Contour::arc_3p(*a, *b, *c)?,
                ArcMode::StartCenterEnd => Contour::arc_ccw(*b, *a, *c),
                ArcMode::CenterStartEnd => Contour::arc_ccw(*a, *b, *c),
                ArcMode::StartEndRadius => Contour::arc_start_end_radius(*a, *b, distance(*b, *c))?,
            },
            // With two points, show the circle the arc lies on (or the chord).
            (Some(Cmd::Arc), [a, b]) => match self.cmdline.arc_mode {
                ArcMode::StartCenterEnd => Contour::circle(b[0], b[1], distance(*a, *b)),
                ArcMode::CenterStartEnd => Contour::circle(a[0], a[1], distance(*a, *b)),
                _ => Contour::line(*a, *b),
            },
            (Some(Cmd::Circle), [a, b, c, ..]) if self.cmdline.circle_mode == CircleMode::ThreePoint => {
                let (centre, r) = circle_3p(*a, *b, *c)?;
                Contour::circle(centre[0], centre[1], r)
            }
            (Some(Cmd::Circle), [a, b]) if self.cmdline.circle_mode == CircleMode::ThreePoint => Contour::line(*a, *b),
            (Some(Cmd::Circle), [a, b, ..]) if self.cmdline.circle_mode == CircleMode::TwoPoint => {
                let centre = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                Contour::circle(centre[0], centre[1], distance(*a, *b) / 2.0)
            }
            _ => return self.tool.shape(pts, self.sides),
        };
        let name = if cmd == Some(Cmd::Arc) { "arc" } else { "circle" };
        Some((name, Shape::path(c)))
    }

    /// Switch mode from the command card: starts the drawing over.
    pub(super) fn set_arc_mode(&mut self, mode: ArcMode) {
        self.cmdline.arc_mode = mode;
        self.draft.clear();
    }

    pub(super) fn set_circle_mode(&mut self, mode: CircleMode) {
        self.cmdline.circle_mode = mode;
        self.draft.clear();
    }

    /// Whether a typed number is a curve value here rather than a distance from the last point.
    pub(super) fn curve_entry(&self, cmd: Cmd, entry: &Entry) -> bool {
        matches!(entry, Entry::Number(_)) && cmd == Cmd::Arc && self.draft.len() == 2 && self.cmdline.arc_mode != ArcMode::ThreePoint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_has_a_label() {
        assert!(ArcMode::ALL.iter().all(|m| !m.label().is_empty()));
        assert!(CircleMode::ALL.iter().all(|m| !m.label().is_empty()));
    }
}
