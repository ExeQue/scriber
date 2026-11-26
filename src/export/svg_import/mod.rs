//! Bringing SVG drawings in. usvg does the parsing (CSS, transforms, `use`, units, text to outlines);
//! this module turns its paths into contours in mm, with curves fitted as arcs, and maps the paint
//! to laser operations the way laser software reads colours.

pub(crate) mod biarc;

use crate::model::geom::{Point, Vertex, distance};
use crate::model::{Contour, Document, Img, Op, Shape};
use resvg::usvg::{self, Node, Paint, tiny_skia_path::PathSegment};
use std::path::PathBuf;

/// usvg works in CSS pixels: 96 per inch.
const MM_PER_PX: f64 = 25.4 / 96.0;

/// How to place an SVG drawing.
#[derive(Clone, Debug)]
pub struct ImportOptions {
    /// Left edge of the imported drawing in mm; default: where the file puts it.
    pub x: Option<f64>,
    /// Top edge in mm; default: where the file puts it.
    pub y: Option<f64>,
    /// Scale the drawing to this width in mm (overrides `scale`).
    pub width: Option<f64>,
    /// Extra scale factor on the file's own size (which honours mm/cm/in units and counts plain
    /// numbers as 96 px per inch). For 72 DPI files (older Illustrator) use 96 / 72 = 1.333.
    pub scale: f64,
    /// Largest distance in mm between a curve and the arcs that replace it.
    pub tolerance: f64,
    /// Folder that relative image links resolve against (the SVG's own folder).
    pub dir: Option<PathBuf>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self { x: None, y: None, width: None, scale: 1.0, tolerance: 0.01, dir: None }
    }
}

/// Stroke colour → laser operation, as laser software reads colours. The nearest reference colour
/// within `NEAR` wins (so #e00, #c00 and #f33 are cut, #333 is engraved); any other colour is cut.
/// A fill on a closed shape means "engrave the area", unless its colour is a no-output grey or green
/// (then it is a helper); a path with both a fill and a stroke becomes two items (the fill's op, then
/// the stroke's) unless both give the same op. A path with neither (or a fully transparent fill)
/// comes in as no output: agents use such shapes as boolean cutters.
const STROKE_OPS: [([u8; 3], Op); 5] = [
    ([255, 0, 0], Op::Cut),           // red
    ([0, 0, 255], Op::Score),         // blue
    ([0, 0, 0], Op::Engrave),         // black and dark greys
    ([150, 150, 150], Op::NoOutput),  // Scriber's no-output grey
    ([0, 150, 70], Op::NoOutput),     // Scriber's old annotation green
];
const NEAR: f64 = 100.0;

fn stroke_op(paint: &Paint) -> Op {
    let Paint::Color(c) = paint else { return Op::Cut };
    colour_op([c.red, c.green, c.blue])
}

/// A line colour → laser operation (also used for DXF layer colours).
pub(crate) fn colour_op(c: [u8; 3]) -> Op {
    let dist = |rgb: [u8; 3]| {
        let d = |a: u8, b: u8| (a as f64 - b as f64).powi(2);
        (d(c[0], rgb[0]) + d(c[1], rgb[1]) + d(c[2], rgb[2])).sqrt()
    };
    STROKE_OPS.iter().map(|(rgb, op)| (dist(*rgb), *op)).filter(|(d, _)| *d <= NEAR).min_by(|a, b| a.0.total_cmp(&b.0)).map_or(Op::Cut, |(_, op)| op)
}

/// Fill → laser operation: engrave the area, unless the colour means no output.
fn fill_op(paint: &Paint) -> Op {
    if stroke_op(paint) == Op::NoOutput { Op::NoOutput } else { Op::Engrave }
}

/// Where an imported item came from: the SVG element's id (empty when it has none) and the
/// element's index among the paths and images in document order (text counts one per glyph path).
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub svg_id: String,
    pub element: usize,
}

/// Something to add to the document.
enum Piece {
    Path { name: String, op: Op, contours: Vec<Contour> },
    /// A picture; (x, y) is its top-left corner before rotating `rotation`° around its centre.
    Image { name: String, img: Img, x: f64, y: f64, w: f64, h: f64, rotation: f64 },
}

/// Affine map from usvg's canvas pixels to millimetres in the document.
#[derive(Clone, Copy)]
struct Map {
    t: usvg::Transform,
    k: f64,
    off: Point,
}

impl Map {
    fn apply(&self, p: (f32, f32)) -> Point {
        let (x, y) = (p.0 as f64, p.1 as f64);
        let t = &self.t;
        let px = [t.sx as f64 * x + t.kx as f64 * y + t.tx as f64, t.ky as f64 * x + t.sy as f64 * y + t.ty as f64];
        [px[0] * self.k + self.off[0], px[1] * self.k + self.off[1]]
    }

    fn with(&self, t: usvg::Transform) -> Self {
        Self { t, ..*self }
    }
}

impl Document {
    /// Add an SVG drawing: one item per path (one contour per subpath), one per raster image.
    /// All or nothing; callers checkpoint first. The new items become the selection. A path goes on
    /// the current layer when that has the op of its colour, else on the first layer with that op.
    pub fn import_svg(&mut self, svg: &[u8], opts: &ImportOptions) -> Result<Vec<u64>, String> {
        Ok(self.import_svg_sources(svg, opts)?.into_iter().map(|(id, _)| id).collect())
    }

    /// `import_svg`, also saying which SVG element each new item came from.
    pub fn import_svg_sources(&mut self, svg: &[u8], opts: &ImportOptions) -> Result<Vec<(u64, Source)>, String> {
        let mut ids = Vec::new();
        let mut out = Vec::new();
        for (source, piece) in parse(svg, opts)? {
            let id = match piece {
                Piece::Path { name, op, contours } => {
                    let layer = self.new_item_layer(Some(op));
                    self.add_to(layer, &name, Shape::Path { contours })
                }
                Piece::Image { name, img, x, y, w, h, rotation } => {
                    let id = self.place_image(img, [x, y], Some(w), &name);
                    if let Some(Shape::Image { h: ih, rotation: r, .. }) = self.item_mut(id).map(|i| &mut i.shape) {
                        (*ih, *r) = (h, rotation);
                    }
                    id
                }
            };
            ids.push(id);
            out.push((id, source));
        }
        self.selection = ids;
        Ok(out)
    }
}

fn parse(svg: &[u8], opts: &ImportOptions) -> Result<Vec<(Source, Piece)>, String> {
    let tol = opts.tolerance;
    if !(tol.is_finite() && tol > 0.0) || !(opts.scale.is_finite() && opts.scale > 0.0) || opts.width.is_some_and(|w| !(w.is_finite() && w > 0.0)) {
        return Err("tolerance, scale and width must be positive numbers".into());
    }
    let usvg_opts = usvg::Options { resources_dir: opts.dir.clone(), fontdb: super::png::options().fontdb.clone(), ..Default::default() };
    let tree = usvg::Tree::from_data(svg, &usvg_opts).map_err(|e| format!("cannot read the SVG: {e}"))?;
    let bbox = tree.root().abs_bounding_box().ok_or("the SVG has nothing to import")?;
    let k = match opts.width {
        Some(w) => w / (bbox.width() as f64).max(1e-9),
        None => MM_PER_PX * opts.scale,
    };
    let off = [opts.x.map_or(0.0, |x| x - bbox.x() as f64 * k), opts.y.map_or(0.0, |y| y - bbox.y() as f64 * k)];
    let mut pieces = Vec::new();
    walk(tree.root(), Map { t: usvg::Transform::identity(), k, off }, tol, &mut 0, &mut pieces)?;
    if pieces.is_empty() {
        return Err("the SVG has no paths or images to import".into());
    }
    Ok(pieces)
}

/// Collect the pieces under `group`; `element` counts the paths and images seen so far.
fn walk(group: &usvg::Group, map: Map, tol: f64, element: &mut usize, out: &mut Vec<(Source, Piece)>) -> Result<(), String> {
    for node in group.children() {
        let source = |id: &str| Source { svg_id: id.into(), element: *element };
        match node {
            Node::Group(g) => walk(g, map, tol, element, out)?,
            Node::Text(t) => walk(t.flattened(), map, tol, element, out)?,
            // usvg hides paths with neither fill nor stroke; they still come in, as helpers.
            Node::Path(p) if p.is_visible() || (p.fill().is_none() && p.stroke().is_none()) => {
                let contours = contours(p.data(), map.with(p.abs_transform()), tol);
                // An open line has no area to engrave, even with SVG's default black fill.
                let area = contours.iter().any(|c| c.closed);
                let fill = p.fill().filter(|f| area && f.opacity().get() > 0.0).map(|f| fill_op(f.paint()));
                // A black stroke on an engraved area adds nothing.
                let line = p.stroke().map(|s| stroke_op(s.paint())).filter(|op| fill != Some(*op));
                // No paint at all (or a lone fill on open lines): a helper, not lasered.
                let ops = if fill.is_none() && line.is_none() { vec![Op::NoOutput] } else { fill.into_iter().chain(line).collect() };
                if !contours.is_empty() {
                    out.extend(ops.into_iter().map(|op| (source(p.id()), Piece::Path { name: name(p.id()), op, contours: contours.clone() })));
                }
            }
            Node::Image(i) if i.is_visible() => out.push((source(i.id()), image(i, map.with(i.abs_transform()))?)),
            Node::Path(_) | Node::Image(_) => {}
        }
        if matches!(node, Node::Path(_) | Node::Image(_)) {
            *element += 1;
        }
    }
    Ok(())
}

fn name(id: &str) -> String {
    if id.is_empty() { "SVG".into() } else { id.into() }
}

/// A contour from `start` through Bézier segments: each is (control points, end point) with no
/// control point for a line, one for a quadratic and two for a cubic curve; curves are fitted as
/// arcs within `tol` mm, like an imported SVG's. None when it has fewer than two distinct points.
pub fn bezier_contour(start: Point, segments: &[(Vec<Point>, Point)], closed: bool, tol: f64) -> Option<Contour> {
    let mut v = vec![Vertex::new(start[0], start[1], 0.0)];
    let mut segs = Vec::new();
    for (controls, to) in segments {
        segs.clear();
        curve_to(xy(v.last().unwrap()), controls, *to, tol, &mut segs);
        for (p, bulge) in &segs {
            biarc::push(&mut v, *p, *bulge);
        }
    }
    let mut out = Vec::new();
    finish(&mut v, closed, tol, &mut out);
    out.pop()
}

/// The segments from `a` to `b`: a line, or a quadratic (one control) or cubic (two) curve as arcs.
fn curve_to(a: Point, controls: &[Point], b: Point, tol: f64, out: &mut Vec<biarc::Seg>) {
    match *controls {
        [c] => {
            let lerp = |p: Point| [p[0] + (c[0] - p[0]) * 2.0 / 3.0, p[1] + (c[1] - p[1]) * 2.0 / 3.0];
            biarc::fit_cubic([a, lerp(a), lerp(b), b], tol, out);
        }
        [c1, c2] => biarc::fit_cubic([a, c1, c2, b], tol, out),
        _ => out.push((b, 0.0)),
    }
}

/// One contour per subpath; quadratic and cubic curves fitted as arcs.
fn contours(data: &usvg::tiny_skia_path::Path, map: Map, tol: f64) -> Vec<Contour> {
    let mut out = Vec::new();
    let mut v: Vec<Vertex> = Vec::new();
    let mut segs = Vec::new();
    for seg in data.segments() {
        let pt = |p: usvg::tiny_skia_path::Point| map.apply((p.x, p.y));
        let from = v.last().map(xy);
        segs.clear();
        match (seg, from) {
            (PathSegment::MoveTo(p), _) => {
                finish(&mut v, false, tol, &mut out);
                v.push(Vertex::new(pt(p)[0], pt(p)[1], 0.0));
            }
            (PathSegment::LineTo(p), Some(a)) => curve_to(a, &[], pt(p), tol, &mut segs),
            (PathSegment::QuadTo(c, p), Some(a)) => curve_to(a, &[pt(c)], pt(p), tol, &mut segs),
            (PathSegment::CubicTo(c1, c2, p), Some(a)) => curve_to(a, &[pt(c1), pt(c2)], pt(p), tol, &mut segs),
            (PathSegment::Close, _) => finish(&mut v, true, tol, &mut out),
            _ => {}
        }
        for (to, bulge) in &segs {
            biarc::push(&mut v, *to, *bulge);
        }
    }
    finish(&mut v, false, tol, &mut out);
    out
}

/// End the subpath being built; a closed one that ends on its start drops the repeated point.
fn finish(v: &mut Vec<Vertex>, closed: bool, tol: f64, out: &mut Vec<Contour>) {
    let mut vertices = std::mem::take(v);
    if closed && vertices.len() > 1 && distance(xy(&vertices[0]), xy(vertices.last().unwrap())) < 1e-6 {
        vertices.pop();
    }
    if vertices.len() >= 2 {
        out.push(Contour { vertices: biarc::merge_arcs(vertices, closed, tol), closed });
    }
}

fn xy(v: &Vertex) -> Point {
    [v.x, v.y]
}

/// A raster image as a greyscale picture, placed where the SVG shows it (mirroring is ignored).
fn image(i: &usvg::Image, map: Map) -> Result<Piece, String> {
    let bytes = match i.kind() {
        usvg::ImageKind::PNG(b) | usvg::ImageKind::JPEG(b) | usvg::ImageKind::GIF(b) | usvg::ImageKind::WEBP(b) => b,
        // ponytail: SVGs embedded as images are skipped with an error; import their paths if people need it.
        usvg::ImageKind::SVG(_) => return Err("SVG files embedded as <image> are not supported".into()),
    };
    let img = super::image::to_grey(bytes).map_err(|e| format!("cannot read an embedded image: {e}"))?;
    let (w, h) = (i.size().width(), i.size().height());
    let (o, right, down, centre) = (map.apply((0.0, 0.0)), map.apply((w, 0.0)), map.apply((0.0, h)), map.apply((w / 2.0, h / 2.0)));
    let (mw, mh) = (distance(o, right), distance(o, down));
    let rotation = (right[1] - o[1]).atan2(right[0] - o[0]).to_degrees();
    Ok(Piece::Image { name: name(i.id()), img, x: centre[0] - mw / 2.0, y: centre[1] - mh / 2.0, w: mw, h: mh, rotation })
}

#[cfg(test)]
mod tests;
