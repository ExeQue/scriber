//! What a drawing is made of: items with a shape, a laser operation and an optional fill.

use super::geom::{BBox, Contour, Point, Transform, bbox_of, rotate_point};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

/// What the laser does with an item's lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Cut through the material (red in SVG).
    #[default]
    Cut,
    /// Mark a line without cutting through (blue in SVG).
    Score,
    /// Burn the line into the surface (black in SVG).
    Engrave,
    /// Do nothing with the lines; useful with `fill` to only engrave the area.
    #[serde(rename = "none")]
    NoLine,
    /// Notes, guides and dimensions for people; never sent to the laser.
    Annotate,
}

impl Op {
    pub const ALL: [Op; 5] = [Op::Cut, Op::Score, Op::Engrave, Op::NoLine, Op::Annotate];

    pub fn rgb(self) -> [u8; 3] {
        match self {
            Op::Cut => [255, 0, 0],
            Op::Score => [0, 0, 255],
            Op::Engrave => [0, 0, 0],
            Op::NoLine => [150, 150, 150],
            Op::Annotate => [0, 150, 70],
        }
    }

    pub fn hex(self) -> String {
        let [r, g, b] = self.rgb();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// Whether the laser follows the lines.
    pub fn burns_lines(self) -> bool {
        matches!(self, Op::Cut | Op::Score | Op::Engrave)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    /// One or more contours. Several contours make a compound path: with fill, holes stay empty (even-odd rule).
    Path { contours: Vec<Contour> },
    /// Text with its baseline starting at (x, y). `size` is the font size in mm.
    Text {
        x: f64,
        y: f64,
        size: f64,
        text: String,
        #[serde(default)]
        rotation: f64,
    },
    /// A measurement between two points, drawn `offset` mm to the left of the line from 1 to 2.
    Dimension {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        #[serde(default)]
        offset: f64,
    },
    /// A greyscale image; (x, y) is its top-left corner before rotation, rotation is around its centre.
    Image {
        image: u64,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        #[serde(default)]
        rotation: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    pub id: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// What happens to the lines.
    pub op: Op,
    /// Engrave the enclosed area (closed paths and text).
    #[serde(default)]
    pub fill: bool,
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl Item {
    /// Whether the fill is actually engraved (only closed paths, text and images have an area).
    pub fn engraves_area(&self) -> bool {
        self.op != Op::Annotate
            && match &self.shape {
                Shape::Path { contours } => self.fill && contours.iter().any(|c| c.closed),
                Shape::Text { .. } => self.fill,
                Shape::Image { .. } => true,
                Shape::Dimension { .. } => false,
            }
    }
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Metrics {
    /// Bounding box: [x, y, width, height].
    pub bbox: BBox,
    /// Length of all outlines (what the laser travels), in mm.
    pub length: f64,
    /// Enclosed area in mm² (holes subtracted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<f64>,
}

/// Where a dimension's lines and label go.
pub struct DimGeom {
    pub a: Point,
    pub b: Point,
    pub a2: Point,
    pub b2: Point,
    pub mid: Point,
    pub angle: f64,
    pub length: f64,
}

impl Shape {
    pub fn path(c: Contour) -> Shape {
        Shape::Path { contours: vec![c] }
    }

    pub fn contours(&self) -> &[Contour] {
        match self {
            Shape::Path { contours } => contours,
            _ => &[],
        }
    }

    /// The four corners of a text or image box after rotation.
    fn box_corners(&self) -> Option<[Point; 4]> {
        let (x, y, w, h, rot, pivot) = match self {
            Shape::Text { x, y, size, text, rotation } => {
                // ponytail: text width estimated at 0.6 × size per character; measure with the real font if it matters.
                let w = 0.6 * size * text.chars().count().max(1) as f64;
                (*x, y - size * 0.8, w, *size, *rotation, [*x, *y])
            }
            Shape::Image { x, y, w, h, rotation, .. } => (*x, *y, *w, *h, *rotation, [x + w / 2.0, y + h / 2.0]),
            _ => return None,
        };
        Some([[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| rotate_point(p, pivot, rot)))
    }

    pub fn metrics(&self) -> Metrics {
        match self {
            Shape::Path { contours } => {
                let boxes: Vec<BBox> = contours.iter().map(Contour::bbox).collect();
                let closed: Vec<&Contour> = contours.iter().filter(|c| c.closed).collect();
                // Even-odd: a contour inside an odd number of others is a hole.
                let area: f64 = closed
                    .iter()
                    .map(|c| {
                        let p = c.vertices.first().map(|v| [v.x, v.y]).unwrap_or_default();
                        let depth = closed.iter().filter(|o| !std::ptr::eq(**o, *c) && o.contains(p)).count();
                        if depth % 2 == 0 { c.area() } else { -c.area() }
                    })
                    .sum();
                Metrics {
                    bbox: super::geom::union(&boxes).unwrap_or([0.0; 4]),
                    length: contours.iter().map(Contour::length).sum(),
                    area: (!closed.is_empty()).then_some(area.abs()),
                }
            }
            Shape::Dimension { x1, y1, x2, y2, .. } => Metrics {
                bbox: bbox_of(&[[*x1, *y1], [*x2, *y2]]),
                length: (x2 - x1).hypot(y2 - y1),
                area: None,
            },
            Shape::Text { .. } | Shape::Image { .. } => {
                let c = self.box_corners().unwrap();
                let w = (c[1][0] - c[0][0]).hypot(c[1][1] - c[0][1]);
                let h = (c[3][0] - c[0][0]).hypot(c[3][1] - c[0][1]);
                Metrics { bbox: bbox_of(&c), length: 2.0 * (w + h), area: Some(w * h) }
            }
        }
    }

    pub fn transformed(&self, t: &Transform, pivot: Point) -> Shape {
        let (sx, sy) = t.scales();
        match self {
            Shape::Path { contours } => Shape::Path { contours: contours.iter().map(|c| c.transformed(t, pivot)).collect() },
            Shape::Dimension { x1, y1, x2, y2, offset } => {
                let [a, b] = [t.apply([*x1, *y1], pivot), t.apply([*x2, *y2], pivot)];
                let flip = if (sx < 0.0) != (sy < 0.0) { -1.0 } else { 1.0 };
                Shape::Dimension { x1: a[0], y1: a[1], x2: b[0], y2: b[1], offset: offset * flip * sx.abs().max(sy.abs()) }
            }
            Shape::Text { x, y, size, text, rotation } => {
                let [nx, ny] = t.apply([*x, *y], pivot);
                Shape::Text { x: nx, y: ny, size: size * (sx.abs() * sy.abs()).sqrt(), text: text.clone(), rotation: rotation + t.rotate }
            }
            Shape::Image { image, x, y, w, h, rotation } => {
                let [cx, cy] = t.apply([x + w / 2.0, y + h / 2.0], pivot);
                let (nw, nh) = (w * sx.abs(), h * sy.abs());
                Shape::Image { image: *image, x: cx - nw / 2.0, y: cy - nh / 2.0, w: nw, h: nh, rotation: rotation + t.rotate }
            }
        }
    }

    /// The outline as polylines in mm (arcs flattened to within `tol` mm). Used for drawing and picking.
    pub fn outline(&self, tol: f64) -> Vec<Vec<Point>> {
        match self {
            Shape::Path { contours } => contours.iter().map(|c| c.flatten(tol)).collect(),
            Shape::Dimension { .. } => {
                let d = self.dimension().unwrap();
                vec![vec![d.a, d.a2], vec![d.b, d.b2], vec![d.a2, d.b2]]
            }
            Shape::Text { .. } | Shape::Image { .. } => {
                let c = self.box_corners().unwrap();
                vec![vec![c[0], c[1], c[2], c[3], c[0]]]
            }
        }
    }

    /// Point inside the shape's area (even-odd for compound paths).
    pub fn contains(&self, p: Point) -> bool {
        match self {
            Shape::Path { contours } => contours.iter().filter(|c| c.contains(p)).count() % 2 == 1,
            Shape::Text { .. } | Shape::Image { .. } => super::geom::point_in_poly(p, &self.outline(1.0)[0]),
            Shape::Dimension { .. } => false,
        }
    }

    pub fn dimension(&self) -> Option<DimGeom> {
        let Shape::Dimension { x1, y1, x2, y2, offset } = *self else { return None };
        let length = (x2 - x1).hypot(y2 - y1);
        let (ux, uy) = if length > 0.0 { ((x2 - x1) / length, (y2 - y1) / length) } else { (1.0, 0.0) };
        // Left of the direction 1 → 2 on screen (y down): (uy, -ux).
        let (nx, ny) = (uy * offset, -ux * offset);
        let (a2, b2) = ([x1 + nx, y1 + ny], [x2 + nx, y2 + ny]);
        Some(DimGeom {
            a: [x1, y1],
            b: [x2, y2],
            a2,
            b2,
            mid: [(a2[0] + b2[0]) / 2.0, (a2[1] + b2[1]) / 2.0],
            angle: uy.atan2(ux).to_degrees(),
            length,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn compound_area_subtracts_holes() {
        let s = Shape::Path {
            contours: vec![Contour::rect(0.0, 0.0, 40.0, 40.0, 0.0), Contour::circle(20.0, 20.0, 10.0)],
        };
        let m = s.metrics();
        assert!((m.area.unwrap() - (1600.0 - PI * 100.0)).abs() < 1e-6);
        assert!((m.length - (160.0 + 2.0 * PI * 10.0)).abs() < 1e-6);
        assert!(s.contains([2.0, 2.0]));
        assert!(!s.contains([20.0, 20.0]), "the hole is not part of the area");
    }
}
