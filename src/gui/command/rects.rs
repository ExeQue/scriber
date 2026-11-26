//! The ways to draw a rectangle: by two corners (AutoCAD's default), from the centre out to a
//! corner, or by three points (the first edge gives width and rotation, the third point the
//! height). Rounded corners (RECTANG's Fillet option) work in every mode.

use super::{App, Cmd};
use crate::model::geom::distance;
use crate::model::{Contour, Point, Shape, Transform};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RectMode {
    #[default]
    TwoCorners,
    Center,
    ThreePoint,
}

impl RectMode {
    pub const ALL: [RectMode; 3] = [RectMode::TwoCorners, RectMode::Center, RectMode::ThreePoint];

    pub fn label(self) -> &'static str {
        match self {
            RectMode::TwoCorners => "2 corners",
            RectMode::Center => "Centre and corner",
            RectMode::ThreePoint => "3 points (rotated)",
        }
    }

    pub fn points(self) -> usize {
        if self == RectMode::ThreePoint { 3 } else { 2 }
    }
}

/// A rectangle `w` × `h` (h may be negative) with its first corner at `a`, turned `angle` degrees.
pub fn rotated_rect(a: Point, w: f64, h: f64, angle: f64, radius: f64) -> Contour {
    let t = Transform { rotate: angle, pivot: Some([0.0, 0.0]), dx: a[0], dy: a[1], ..Default::default() };
    Contour::rect(0.0, 0.0, w, h, radius).transformed(&t, [0.0, 0.0])
}

/// Signed distance of `p` from the line a→b, measured along the edge's left-hand normal (y down).
pub fn height_from(a: Point, b: Point, p: Point) -> f64 {
    let len = distance(a, b).max(1e-12);
    let (ux, uy) = ((b[0] - a[0]) / len, (b[1] - a[1]) / len);
    (p[0] - a[0]) * -uy + (p[1] - a[1]) * ux
}

impl App {
    pub(super) fn rect_prompt(&self, placed: usize) -> &'static str {
        match (self.cmdline.rect_mode, placed) {
            (RectMode::TwoCorners, 0) => "Specify first corner point or [Center/3P/Fillet]:",
            (RectMode::TwoCorners, _) => "Specify other corner point:",
            (RectMode::Center, 0) => "Specify center of rectangle:",
            (RectMode::Center, _) => "Specify corner point:",
            (RectMode::ThreePoint, 0) => "Specify first corner point:",
            (RectMode::ThreePoint, 1) => "Specify second corner (width and rotation):",
            (RectMode::ThreePoint, _) => "Specify height:",
        }
    }

    /// The rectangle the placed points make (the last may be the pointer).
    pub(super) fn rect_shape(&self, pts: &[Point]) -> Option<Shape> {
        let r = self.cmdline.fillet;
        let c = match (self.cmdline.rect_mode, pts) {
            (RectMode::TwoCorners, [a, b, ..]) => Contour::rect(a[0], a[1], b[0] - a[0], b[1] - a[1], r),
            (RectMode::Center, [c, b, ..]) => {
                let (w, h) = (2.0 * (b[0] - c[0]).abs(), 2.0 * (b[1] - c[1]).abs());
                Contour::rect(c[0] - w / 2.0, c[1] - h / 2.0, w, h, r)
            }
            (RectMode::ThreePoint, [a, b, p, ..]) => {
                let angle = (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees();
                rotated_rect(*a, distance(*a, *b), height_from(*a, *b, *p), angle, r)
            }
            (RectMode::ThreePoint, [a, b]) => Contour::line(*a, *b),
            _ => return None,
        };
        Some(Shape::path(c))
    }

    pub(super) fn set_rect_mode(&mut self, mode: RectMode) {
        self.cmdline.rect_mode = mode;
        self.draft.clear();
    }

    /// Whether `cmd` is a rectangle drawn in the given mode.
    pub(super) fn rect_in(&self, cmd: Cmd, mode: RectMode) -> bool {
        cmd == Cmd::Rectang && self.cmdline.rect_mode == mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_rect_and_height() {
        // Edge along +x: the left-hand normal (y down) points to +y.
        assert!((height_from([0.0, 0.0], [10.0, 0.0], [3.0, 4.0]) - 4.0).abs() < 1e-9);
        let c = rotated_rect([5.0, 5.0], 10.0, 4.0, 90.0, 0.0);
        let b = c.bbox();
        assert!((b[2] - 4.0).abs() < 1e-9 && (b[3] - 10.0).abs() < 1e-9, "{b:?}");
        assert!((c.area() - 40.0).abs() < 1e-9);
        // Negative height goes to the other side.
        assert!((rotated_rect([0.0, 0.0], 10.0, -4.0, 0.0, 0.0).bbox()[1] + 4.0).abs() < 1e-9);
    }
}
