//! What a drawing is made of: items with a shape and a laser operation.

use super::geom::{BBox, Contour, Point, Transform, bbox_of, rotate_point};
use super::guides::Guide;
use super::text;
use cavalier_contours::polyline::PlineSource;
use std::borrow::Cow;
use std::sync::Arc;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

/// What the laser does with an item (the four operations of xTool Studio).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Cut through the material along the line (red in SVG).
    #[default]
    Cut,
    /// Mark a line without cutting through (blue in SVG).
    Score,
    /// Burn the area inside closed shapes, text and images (holes stay empty; black in SVG).
    /// Open shapes have no area: they are scored (see `Item::effective_op`).
    Engrave,
    /// Notes, guides and dimensions for people; never sent to the laser (grey).
    /// Old files and clients may say "annotate" or "none".
    #[serde(alias = "annotate", alias = "none")]
    NoOutput,
}

impl Op {
    pub const ALL: [Op; 4] = [Op::Cut, Op::Score, Op::Engrave, Op::NoOutput];

    pub fn rgb(self) -> [u8; 3] {
        match self {
            Op::Cut => [255, 0, 0],
            Op::Score => [0, 0, 255],
            Op::Engrave => [0, 0, 0],
            Op::NoOutput => [150, 150, 150],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Op::Cut => "Cut",
            Op::Score => "Score",
            Op::Engrave => "Engrave",
            Op::NoOutput => "No output",
        }
    }

    /// What the laser really does with this op on `shape`: Engrave on an open shape (no area) is scored.
    pub fn on(self, shape: &Shape) -> Op {
        if self == Op::Engrave && !shape.has_area() { Op::Score } else { self }
    }

    pub fn hex(self) -> String {
        let [r, g, b] = self.rgb();
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    /// One or more contours. Several contours make a compound path: engraved, holes stay empty (even-odd rule).
    Path { contours: Vec<Contour> },
    /// Text with its baseline starting at (x, y). `size` is the font size in mm. It is drawn,
    /// measured and exported as the font's glyph outlines.
    Text {
        x: f64,
        y: f64,
        size: f64,
        text: String,
        #[serde(default)]
        rotation: f64,
        /// Font family; "sans-serif", "serif" and "monospace" pick the system's default of that kind.
        #[serde(default = "default_font")]
        font: String,
        #[serde(default, skip_serializing_if = "is_false")]
        bold: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        italic: bool,
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
    /// A guide line (XLINE, RAY, CENTERLINE, CENTERMARK): drawn on the canvas, never lasered or exported.
    Guide(Guide),
}

fn default_font() -> String {
    text::DEFAULT_FONT.to_owned()
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    pub id: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The layer it is on (see `layer`); the layer gives the laser op.
    pub layer: u64,
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
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

    /// Closed paths, text and images enclose an area; open paths and dimensions do not.
    pub fn has_area(&self) -> bool {
        match self {
            Shape::Path { contours } => contours.iter().any(|c| c.closed),
            Shape::Text { .. } | Shape::Image { .. } => true,
            Shape::Dimension { .. } | Shape::Guide(_) => false,
        }
    }

    pub fn contours(&self) -> &[Contour] {
        match self {
            Shape::Path { contours } => contours,
            _ => &[],
        }
    }

    /// A text's glyph outlines before placing (baseline start at 0, 0, unrotated).
    fn text_glyphs(&self) -> Option<Arc<text::Glyphs>> {
        let Shape::Text { size, text, font, bold, italic, .. } = self else { return None };
        Some(text::glyphs(&text::Style { text, font, size: size.to_bits(), bold: *bold, italic: *italic }))
    }

    /// The contours the laser follows: a path's own, or a text's glyph outlines in place.
    pub fn laser_contours(&self) -> Cow<'_, [Contour]> {
        match self {
            Shape::Path { contours } => Cow::Borrowed(contours),
            Shape::Text { x, y, rotation, .. } => Cow::Owned(text::place(&self.text_glyphs().unwrap().contours, [*x, *y], *rotation)),
            _ => Cow::Borrowed(&[]),
        }
    }

    /// The four corners of a text or image box after rotation.
    fn box_corners(&self) -> Option<[Point; 4]> {
        let (x, y, w, h, rot, pivot) = match self {
            Shape::Text { x, y, size, text, rotation, .. } => {
                // Without outlines (no usable font), estimate 0.6 × size per character so it stays pickable.
                let [bx, by, w, h] = self.text_glyphs().unwrap().bbox.unwrap_or([0.0, -0.8 * size, 0.6 * size * text.chars().count().max(1) as f64, *size]);
                (x + bx, y + by, w, h, *rotation, [*x, *y])
            }
            Shape::Image { x, y, w, h, rotation, .. } => (*x, *y, *w, *h, *rotation, [x + w / 2.0, y + h / 2.0]),
            _ => return None,
        };
        Some([[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| rotate_point(p, pivot, rot)))
    }

    pub fn metrics(&self) -> Metrics {
        match self {
            Shape::Path { contours } => {
                use cavalier_contours::core::math::Vector2;
                use cavalier_contours::polyline::PlineSource;
                // Build each contour's polyline once; this runs every frame for the selection and totals.
                let plines: Vec<_> = contours.iter().map(|c| (c, c.pline())).collect();
                let boxes: Vec<BBox> = plines
                    .iter()
                    .map(|(c, p)| p.extents().map(|e| [e.min_x, e.min_y, e.max_x - e.min_x, e.max_y - e.min_y]).unwrap_or_else(|| c.bbox()))
                    .collect();
                let closed: Vec<usize> = (0..contours.len()).filter(|i| contours[*i].closed).collect();
                // Even-odd: a contour inside an odd number of others is a hole.
                let inside = |o: usize, q: [f64; 2]| {
                    let b = boxes[o];
                    q[0] >= b[0] && q[0] <= b[0] + b[2] && q[1] >= b[1] && q[1] <= b[1] + b[3] && plines[o].1.winding_number(Vector2::new(q[0], q[1])) != 0
                };
                let area: f64 = closed
                    .iter()
                    .map(|&i| {
                        let q = contours[i].vertices.first().map(|v| [v.x, v.y]).unwrap_or_default();
                        let depth = closed.iter().filter(|&&o| o != i && inside(o, q)).count();
                        let a = plines[i].1.area().abs();
                        if depth % 2 == 0 { a } else { -a }
                    })
                    .sum();
                Metrics {
                    bbox: super::geom::union(&boxes).unwrap_or([0.0; 4]),
                    length: plines.iter().map(|(_, p)| p.path_length()).sum(),
                    area: (!closed.is_empty()).then_some(area.abs()),
                }
            }
            Shape::Dimension { x1, y1, x2, y2, .. } => Metrics {
                bbox: bbox_of(&[[*x1, *y1], [*x2, *y2]]),
                length: (x2 - x1).hypot(y2 - y1),
                area: None,
            },
            Shape::Text { .. } if self.text_glyphs().unwrap().bbox.is_some() => {
                let contours = self.laser_contours();
                let boxes: Vec<BBox> = contours.iter().map(Contour::bbox).collect();
                // Glyph holes wind the other way (non-zero rule), so signed areas add up to the ink.
                let area: f64 = contours.iter().map(|c| c.pline().area()).sum();
                Metrics {
                    bbox: super::geom::union(&boxes).unwrap_or([0.0; 4]),
                    length: contours.iter().map(Contour::length).sum(),
                    area: Some(area.abs()),
                }
            }
            Shape::Guide(g) => Metrics { bbox: g.bbox(), length: 0.0, area: None },
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
            Shape::Text { x, y, size, rotation, .. } => {
                let mut out = self.clone();
                if let Shape::Text { x: nx, y: ny, size: ns, rotation: nr, .. } = &mut out {
                    [*nx, *ny] = t.apply([*x, *y], pivot);
                    *ns = size * (sx.abs() * sy.abs()).sqrt();
                    *nr = rotation + t.rotate;
                }
                out
            }
            Shape::Image { image, x, y, w, h, rotation } => {
                let [cx, cy] = t.apply([x + w / 2.0, y + h / 2.0], pivot);
                let (nw, nh) = (w * sx.abs(), h * sy.abs());
                Shape::Image { image: *image, x: cx - nw / 2.0, y: cy - nh / 2.0, w: nw, h: nh, rotation: rotation + t.rotate }
            }
            Shape::Guide(g) => Shape::Guide(g.transformed(t, pivot)),
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
            Shape::Text { .. } if self.text_glyphs().unwrap().bbox.is_some() => self.laser_contours().iter().map(|c| c.flatten(tol)).collect(),
            Shape::Text { .. } | Shape::Image { .. } => vec![self.box_outline()],
            Shape::Guide(g) => g.outline(),
        }
    }

    fn box_outline(&self) -> Vec<Point> {
        let c = self.box_corners().unwrap();
        vec![c[0], c[1], c[2], c[3], c[0]]
    }

    /// Point inside the shape's area (even-odd for compound paths).
    pub fn contains(&self, p: Point) -> bool {
        match self {
            Shape::Path { contours } => contours.iter().filter(|c| c.contains(p)).count() % 2 == 1,
            // Text is picked by its box, so clicking between letters works.
            Shape::Text { .. } | Shape::Image { .. } => super::geom::point_in_poly(p, &self.box_outline()),
            Shape::Dimension { .. } | Shape::Guide(_) => false,
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

    #[test]
    fn engrave_fills_closed_shapes_and_scores_open_ones() {
        let (closed, open) = (Shape::path(Contour::circle(0.0, 0.0, 5.0)), Shape::path(Contour::line([0.0, 0.0], [5.0, 0.0])));
        assert_eq!((Op::Engrave.on(&closed), Op::Engrave.on(&open)), (Op::Engrave, Op::Score));
        assert_eq!(Op::NoOutput.on(&open), Op::NoOutput);
        assert_eq!(serde_json::from_str::<Op>("\"annotate\"").unwrap(), Op::NoOutput);
        assert_eq!(serde_json::to_string(&Op::NoOutput).unwrap(), "\"no_output\"");
    }

    #[test]
    fn old_text_without_font_loads_with_the_default() {
        let s: Shape = serde_json::from_str(r#"{"kind":"text","x":1,"y":2,"size":5,"text":"Hi"}"#).unwrap();
        let Shape::Text { font, bold, italic, .. } = &s else { panic!() };
        assert_eq!((font.as_str(), *bold, *italic), ("sans-serif", false, false));
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"font\":\"sans-serif\"") && !json.contains("bold"), "{json}");
    }

    #[test]
    fn text_bbox_comes_from_the_glyph_outlines() {
        let text = |t: &str| Shape::Text { x: 10.0, y: 20.0, size: 10.0, text: t.into(), rotation: 0.0, font: "sans-serif".into(), bold: false, italic: false };
        let (s, wide) = (text("i"), text("WWWW"));
        if s.laser_contours().is_empty() {
            return eprintln!("no font for sans-serif; skipped");
        }
        let (m, mw) = (s.metrics(), wide.metrics());
        // Glyphs sit on the baseline (y = 20) and are much narrower than the old 0.6 × size estimate for "i".
        assert!((m.bbox[1] + m.bbox[3] - 20.0).abs() < 0.2, "{:?}", m.bbox);
        assert!(m.bbox[2] < 3.0 && mw.bbox[2] > 4.0 * m.bbox[2], "{:?} {:?}", m.bbox, mw.bbox);
        assert!(m.area.unwrap() > 0.0 && m.area.unwrap() < m.bbox[2] * m.bbox[3]);
        assert!(s.contains([m.bbox[0] + m.bbox[2] / 2.0, m.bbox[1] + m.bbox[3] / 2.0]));
    }
}
