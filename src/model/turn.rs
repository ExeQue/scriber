//! Item rotation: each item remembers the angle it was turned by, so its selection box turns with it.
//!
//! When a transform keeps or resets the angle (`after`):
//! - a rotation adds to it;
//! - a uniform scale keeps it; a mirror in one axis turns it the other way (-angle);
//! - a non-uniform scale keeps it only when it runs along the item's own axes (angle a multiple of
//!   90°); otherwise the shape skews out of its box and the angle resets to 0. A scale made along the
//!   turned axes (`Document::transform_turned`, the selection box grips) keeps it.

use super::geom::{BBox, Point, Transform, rotate_point, union};
use super::{Document, Item, Shape};

const EPS: f64 = 1e-6;

/// `a` in degrees, in (-180, 180].
pub fn normal(a: f64) -> f64 {
    let a = (a + 180.0).rem_euclid(360.0) - 180.0;
    if a <= -180.0 + 1e-12 { 180.0 } else { a }
}

/// An item's angle after `t` (see the module docs).
pub fn after(angle: f64, t: &Transform) -> f64 {
    let (sx, sy) = t.scales();
    let square = (angle / 90.0 - (angle / 90.0).round()).abs() < EPS;
    let a = if (sx.abs() - sy.abs()).abs() > EPS * sx.abs().max(sy.abs()) && !square {
        0.0
    } else if (sx < 0.0) != (sy < 0.0) {
        -angle
    } else {
        angle
    };
    normal(a + t.rotate)
}

/// The angle all `items` share, None when they differ (or there are none).
pub fn shared_rotation<'a>(items: impl IntoIterator<Item = &'a Item>) -> Option<f64> {
    let mut it = items.into_iter().map(|i| i.rotation);
    let first = it.next()?;
    it.all(|a| (normal(a - first)).abs() < EPS).then_some(first)
}

/// The angle a new item starts at: text and images keep the rotation they were made with.
pub fn own_rotation(shape: &Shape) -> f64 {
    match shape {
        Shape::Text { rotation, .. } | Shape::Image { rotation, .. } => normal(*rotation),
        _ => 0.0,
    }
}

/// The selection box of `items`: their bounds in the frame turned by their shared angle (about the
/// origin), and that angle; axis-aligned (angle 0) when the angles differ.
pub fn turned_box<'a>(items: impl IntoIterator<Item = &'a Item>) -> Option<(BBox, f64)> {
    let items: Vec<&Item> = items.into_iter().collect();
    let angle = shared_rotation(items.iter().copied()).unwrap_or(0.0);
    let back = Transform { rotate: -angle, ..Default::default() };
    // Re-measures the turned copies on every call, uncached.
    let boxes: Vec<BBox> =
        items.iter().map(|i| if angle == 0.0 { i.shape.metrics().bbox } else { i.shape.transformed(&back, [0.0, 0.0]).metrics().bbox }).collect();
    Some((union(&boxes)?, angle))
}

/// The world position of the point `p` of a box in the frame turned by `angle`.
pub fn to_world(p: Point, angle: f64) -> Point {
    rotate_point(p, [0.0, 0.0], angle)
}

/// A world point in the frame turned by `angle`.
pub fn to_frame(p: Point, angle: f64) -> Point {
    rotate_point(p, [0.0, 0.0], -angle)
}

impl Document {
    /// `t` made in the frame turned by `angle` (its pivot in that frame): a scale runs along the turned
    /// axes, so items at that angle keep it.
    pub fn transform_turned(&mut self, ids: &[u64], t: &Transform, angle: f64) {
        if angle == 0.0 {
            return self.transform(ids, t);
        }
        let turn = |rotate| Transform { rotate, pivot: Some([0.0, 0.0]), ..Default::default() };
        self.transform(ids, &turn(-angle));
        self.transform(ids, t);
        self.transform(ids, &turn(angle));
    }

    /// Turn the items to `angle` degrees about the centre of their selection box. They must share an angle.
    pub fn set_rotation(&mut self, ids: &[u64], angle: f64) -> Result<(), String> {
        let items = || self.s.items.iter().filter(|i| ids.contains(&i.id));
        let current = shared_rotation(items()).ok_or("the items have different angles; set them one at a time")?;
        let (b, turned) = turned_box(items()).unwrap();
        let pivot = to_world([b[0] + b[2] / 2.0, b[1] + b[3] / 2.0], turned);
        self.transform(ids, &Transform { rotate: normal(angle - current), pivot: Some(pivot), ..Default::default() });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Contour, Op};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn rules() {
        let t = |rotate, scale_x, scale_y, mirror_x| Transform { rotate, scale_x, scale_y, mirror_x, ..Default::default() };
        assert_eq!(after(170.0, &t(20.0, 0.0, 0.0, false)), -170.0);
        assert_eq!(after(30.0, &t(0.0, 2.0, 2.0, false)), 30.0, "uniform scale keeps it");
        assert_eq!(after(30.0, &t(0.0, 1.0, 1.0, true)), -30.0, "a mirror turns it back");
        assert_eq!(after(30.0, &t(0.0, 2.0, 1.0, false)), 0.0, "skewed: reset");
        assert_eq!(after(90.0, &t(0.0, 2.0, 1.0, false)), 90.0, "along its axes: kept");
        assert_eq!(after(0.0, &t(5.0, 2.0, 1.0, false)), 5.0);
    }

    #[test]
    fn rotate_then_box_turns_and_scale_along_it_keeps_the_angle() {
        let mut d = Document::new(200.0, 200.0);
        let id = d.add("", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 40.0, 20.0, 0.0)));
        d.transform(&[id], &Transform { rotate: 30.0, pivot: Some([30.0, 20.0]), ..Default::default() });
        assert_eq!(d.item(id).unwrap().rotation, 30.0);
        let (b, a) = turned_box(d.item(id)).unwrap();
        assert!(close(a, 30.0) && close(b[2], 40.0) && close(b[3], 20.0), "{b:?}");
        let centre = to_world([b[0] + 20.0, b[1] + 10.0], a);
        assert!(close(centre[0], 30.0) && close(centre[1], 20.0));

        // Twice as wide along the turned x axis, around the box's left edge.
        let t = Transform { scale_x: 2.0, scale_y: 1.0, pivot: Some([b[0], b[1]]), ..Default::default() };
        d.transform_turned(&[id], &t, a);
        assert!(close(d.item(id).unwrap().rotation, 30.0));
        let (b2, _) = turned_box(d.item(id)).unwrap();
        assert!(close(b2[0], b[0]) && close(b2[2], 80.0) && close(b2[3], 20.0), "{b2:?}");

        // The same scale along the world axes skews it.
        d.transform(&[id], &t);
        assert_eq!(d.item(id).unwrap().rotation, 0.0);

        d.set_rotation(&[id], 45.0).unwrap();
        assert_eq!(d.item(id).unwrap().rotation, 45.0);
    }

    #[test]
    fn saved_and_old_files() {
        let path = std::env::temp_dir().join(format!("scriber-turn-{}.scrbr", std::process::id()));
        let mut d = Document::new(100.0, 100.0);
        let id = d.add("", Op::Cut, Shape::path(Contour::rect(0.0, 0.0, 10.0, 5.0, 0.0)));
        d.transform(&[id], &Transform { rotate: 25.0, ..Default::default() });
        d.save(&path).unwrap();
        assert_eq!(Document::open(&path).unwrap().item(id).unwrap().rotation, 25.0);
        std::fs::remove_file(path).ok();
        let old: Item = serde_json::from_str(r#"{"id": 1, "layer": 1, "shape": {"kind": "path", "contours": []}}"#).unwrap();
        assert_eq!(old.rotation, 0.0);
    }

    #[test]
    fn mixed_angles_get_an_upright_box() {
        let mut d = Document::new(200.0, 200.0);
        let a = d.add("", Op::Cut, Shape::path(Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0)));
        let b = d.add("", Op::Cut, Shape::path(Contour::rect(20.0, 0.0, 10.0, 10.0, 0.0)));
        d.transform(&[a], &Transform { rotate: 45.0, ..Default::default() });
        assert_eq!(turned_box(&d.s.items).unwrap().1, 0.0);
        assert!(d.set_rotation(&[a, b], 10.0).is_err());
    }
}
