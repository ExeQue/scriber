//! Bringing DXF drawings in. Entities become contours in mm (arcs stay arcs; ellipses, splines and
//! arcs under a stretching block scale are fitted as arcs within the tolerance), lines and arcs that
//! touch end to end are joined into one contour, and closed contours inside one another become a
//! compound path. Everything is sorted into groups, one per DXF layer (one per layer and colour when
//! a layer holds several colours); each group lands on a Scriber layer of its own, with the op the
//! person picks for it (`Group::guess` by default).

use super::aci_rgb;
use crate::export::svg_import::{biarc, colour_op};
use crate::model::geom::{BBox, Point, Vertex, arc_params, distance, point_in_poly};
use crate::model::text::DEFAULT_FONT;
use crate::model::{Contour, Document, Op, Shape, Transform};
use dxf::Drawing;
use dxf::entities::{Entity, EntityType, Spline};
use dxf::enums::{AttachmentPoint, Units};
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Cap height (what DXF calls the text height) of a font size: about 0.7 em.
const CAP: f64 = 0.7;
/// Distance between text lines, in text heights (AutoCAD's default line spacing).
const LINE_SPACING: f64 = 5.0 / 3.0;
/// Blocks inside blocks deeper than this are left out (a block that inserts itself).
const MAX_DEPTH: usize = 32;

/// A DXF drawing read and turned into mm, before it goes into a document.
pub struct Dxf {
    /// The file's units as people say them ("inch", or "none (read as mm)").
    pub units: &'static str,
    pub groups: Vec<Group>,
    /// Entities left out, by type (HATCH, DIMENSION, …), with how many.
    pub skipped: BTreeMap<String, usize>,
}

/// What one DXF layer (or one colour on it) holds.
pub struct Group {
    /// The DXF layer's name, with the colour as #rrggbb when the layer has several.
    pub name: String,
    pub rgb: [u8; 3],
    /// The op to use unless the person picks another; None = leave it out (the layer is off).
    pub guess: Option<Op>,
    pub shapes: Vec<Shape>,
}

/// Read a DXF (ASCII or binary). `height` is the material's: the DXF origin goes to its
/// bottom-left corner. `tol` (mm) is how far fitted arcs may stray from curves.
pub fn read(bytes: &[u8], height: f64, tol: f64) -> Result<Dxf, String> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err("tolerance must be a positive number".into());
    }
    let drawing = Drawing::load(&mut std::io::Cursor::new(bytes)).map_err(|e| format!("cannot read the DXF: {e}"))?;
    let (k, units) = units(drawing.header.default_drawing_units);
    let mut w = Walker {
        layers: drawing.layers().map(|l| (l.name.clone(), (l.color.index().map_or([255; 3], aci_rgb), l.is_layer_on))).collect(),
        blocks: drawing.blocks().map(|b| (b.name.as_str(), b)).collect(),
        tol,
        pieces: BTreeMap::new(),
        skipped: BTreeMap::new(),
    };
    let base = Affine([k, 0.0, 0.0, -k, 0.0, height]);
    for e in drawing.entities().filter(|e| !e.common.is_in_paper_space) {
        w.entity(e, &base, None, 0);
    }
    let mut colours: HashMap<String, usize> = HashMap::new();
    for (layer, _) in w.pieces.keys() {
        *colours.entry(layer.clone()).or_default() += 1;
    }
    let groups = std::mem::take(&mut w.pieces)
        .into_iter()
        .map(|((layer, rgb), pieces)| {
            let name = if colours[&layer] > 1 { format!("{layer} #{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]) } else { layer.clone() };
            let on = w.layers.get(&layer).is_none_or(|l| l.1);
            Group { guess: on.then(|| guess(&name, rgb)), name, rgb, shapes: shapes(pieces) }
        })
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return Err("the DXF has nothing to import".into());
    }
    Ok(Dxf { units, groups, skipped: w.skipped })
}

/// mm per drawing unit, and the unit's name. No units: mm.
fn units(u: Units) -> (f64, &'static str) {
    match u {
        Units::Unitless => (1.0, "none (read as mm)"),
        Units::Millimeters => (1.0, "mm"),
        Units::Centimeters => (10.0, "cm"),
        Units::Decimeters => (100.0, "dm"),
        Units::Meters => (1000.0, "m"),
        Units::Microns => (0.001, "µm"),
        Units::Inches | Units::USSurveyInch => (25.4, "inch"),
        Units::Feet | Units::USSurveyFeet => (304.8, "foot"),
        Units::Yards | Units::USSurveyYard => (914.4, "yard"),
        Units::Mils => (0.0254, "mil"),
        Units::Microinches => (25.4e-6, "microinch"),
        _ => (1.0, "unsupported (read as mm)"),
    }
}

/// The op for a group: its name when it says one (Scriber's own layer names), else its colour as
/// in an SVG (red cut, blue score, black and dark grey engrave, grey no output, any other cut;
/// AutoCAD's colour 7, white/black, counts as white: cut).
pub fn guess(name: &str, rgb: [u8; 3]) -> Op {
    let name = name.to_lowercase();
    let says = [("no output", Op::NoOutput), ("engrave", Op::Engrave), ("score", Op::Score), ("cut", Op::Cut)];
    says.iter().find(|(word, _)| name.contains(word)).map_or_else(|| colour_op(rgb), |(_, op)| *op)
}

impl Document {
    /// Add a read DXF: each group not skipped goes on a layer with its name and op (an existing one
    /// with both, else a new one). `ops` overrides the groups' guesses by name (None = skip);
    /// `at` puts the drawing's top-left corner there instead of where the file has it.
    /// All or nothing; callers checkpoint first. The new items become the selection.
    pub fn import_dxf(&mut self, dxf: &Dxf, ops: &BTreeMap<String, Option<Op>>, at: Option<Point>) -> Result<Vec<u64>, String> {
        let chosen: Vec<(&Group, Op)> = dxf.groups.iter().filter_map(|g| ops.get(&g.name).copied().unwrap_or(g.guess).map(|op| (g, op))).collect();
        let boxes: Vec<BBox> = chosen.iter().flat_map(|(g, _)| g.shapes.iter().map(|s| s.metrics().bbox)).collect();
        let Some(b) = crate::model::geom::union(&boxes) else { return Err("nothing to import: every layer is skipped".into()) };
        let shift = at.map_or(Transform::translate(0.0, 0.0), |p| Transform::translate(p[0] - b[0], p[1] - b[1]));
        let mut ids = Vec::new();
        for (g, op) in chosen {
            let layer = match self.s.layers.iter().find(|l| l.name == g.name && l.op == op) {
                Some(l) => l.id,
                None => self.add_layer(&g.name, op),
            };
            for shape in &g.shapes {
                ids.push(self.add_to(layer, &g.name, shape.transformed(&shift, [0.0, 0.0])));
            }
        }
        self.selection = ids.clone();
        Ok(ids)
    }
}

/// A 2D affine map: x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy, Debug)]
struct Affine([f64; 6]);

impl Affine {
    fn apply(&self, p: Point) -> Point {
        let [a, b, c, d, e, f] = self.0;
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }

    fn linear(&self, v: Point) -> Point {
        let [a, b, c, d, ..] = self.0;
        [a * v[0] + c * v[1], b * v[0] + d * v[1]]
    }

    /// `self`, then `outer`.
    fn then(&self, outer: &Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let (x, y, o) = (outer.linear([a, b]), outer.linear([c, d]), outer.apply([e, f]));
        Affine([x[0], x[1], y[0], y[1], o[0], o[1]])
    }

    fn det(&self) -> f64 {
        let [a, b, c, d, ..] = self.0;
        a * d - b * c
    }

    /// Largest stretch: a length of 1 becomes at most this long.
    fn scale(&self) -> f64 {
        let [a, b, c, d, ..] = self.0;
        (a.hypot(b) + c.hypot(d)).max(1e-12)
    }

    /// Keeps circles circles (rotation, uniform scale, mirror).
    fn similar(&self) -> bool {
        let [a, b, c, d, ..] = self.0;
        let (x, y) = (a * a + b * b, c * c + d * d);
        (a * c + b * d).abs() <= 1e-9 * x.max(y) && (x - y).abs() <= 1e-9 * x.max(y)
    }

    fn rotate(deg: f64) -> Affine {
        let (s, c) = deg.to_radians().sin_cos();
        Affine([c, s, -s, c, 0.0, 0.0])
    }
}

/// The map from an entity's own coordinate system (OCS) into the drawing: 2D entities drawn
/// upside down (extrusion along -z) are mirrored left to right. ponytail: tilted (3D) extrusions
/// are read as if flat.
fn ocs(m: &Affine, normal: &dxf::Vector) -> Affine {
    if normal.z < 0.0 { Affine([-1.0, 0.0, 0.0, 1.0, 0.0, 0.0]).then(m) } else { *m }
}

fn xy(p: &dxf::Point) -> Point {
    [p.x, p.y]
}

/// What an entity in a block takes from the INSERT that places it: layer "0" and colour BYBLOCK.
struct Ctx {
    layer: String,
    rgb: [u8; 3],
}

enum Piece {
    Contour(Contour),
    Text(Shape),
}

struct Walker<'a> {
    /// Layer name → colour and whether it is on.
    layers: HashMap<String, ([u8; 3], bool)>,
    blocks: HashMap<&'a str, &'a dxf::Block>,
    tol: f64,
    pieces: BTreeMap<(String, [u8; 3]), Vec<Piece>>,
    skipped: BTreeMap<String, usize>,
}

impl Walker<'_> {
    fn skip(&mut self, what: &str) {
        *self.skipped.entry(what.into()).or_default() += 1;
    }

    fn entity(&mut self, e: &Entity, m: &Affine, ctx: Option<&Ctx>, depth: usize) {
        if !e.common.is_visible {
            return;
        }
        let c = &e.common;
        let layer = match ctx {
            Some(ctx) if c.layer == "0" => ctx.layer.clone(),
            _ => c.layer.clone(),
        };
        let rgb = if c.color_24_bit != 0 {
            let [_, r, g, b] = c.color_24_bit.to_be_bytes();
            [r, g, b]
        } else if c.color.is_by_block() {
            ctx.map_or([255; 3], |ctx| ctx.rgb)
        } else if let Some(i) = c.color.index() {
            aci_rgb(i)
        } else {
            self.layers.get(&layer).map_or([255; 3], |l| l.0)
        };
        let tol = self.tol;
        let contour = match &e.specific {
            EntityType::Line(l) => polyline(&[(xy(&l.p1), 0.0), (xy(&l.p2), 0.0)], false, m, tol),
            EntityType::Arc(a) => {
                let sweep = (a.end_angle - a.start_angle).rem_euclid(360.0);
                let sweep = if sweep < 1e-9 { 360.0 } else { sweep };
                circle_arc(xy(&a.center), a.radius, a.start_angle, sweep, &ocs(m, &a.normal), tol)
            }
            EntityType::Circle(ci) => circle_arc(xy(&ci.center), ci.radius, 0.0, 360.0, &ocs(m, &ci.normal), tol),
            EntityType::LwPolyline(p) => {
                let pts: Vec<(Point, f64)> = p.vertices.iter().map(|v| ([v.x, v.y], v.bulge)).collect();
                polyline(&pts, p.flags & 1 != 0, &ocs(m, &p.extrusion_direction), tol)
            }
            EntityType::Polyline(p) if p.flags & (16 | 64) != 0 => return self.skip("POLYLINE mesh"),
            EntityType::Polyline(p) => {
                // Spline frame control points (flag 16) are not on the curve; the fitted vertices are.
                let pts: Vec<(Point, f64)> = p.vertices().filter(|v| v.flags & 16 == 0).map(|v| (xy(&v.location), v.bulge)).collect();
                let m = if p.flags & 8 != 0 { *m } else { ocs(m, &p.normal) };
                polyline(&pts, p.flags & 1 != 0, &m, tol)
            }
            EntityType::Ellipse(el) => ellipse(el, m, tol),
            EntityType::Spline(s) => match spline(s, m, tol) {
                Some(c) => Some(c),
                None => return self.skip("SPLINE without control or fit points"),
            },
            EntityType::Text(t) => {
                let m = ocs(m, &t.normal);
                return self.text(layer, rgb, &clean(&t.value), xy(&t.location), t.text_height, t.rotation, 0.0, &m);
            }
            EntityType::MText(t) => {
                let m = ocs(m, &t.extrusion_direction);
                let dir = &t.x_axis_direction;
                let rotation = if dir.x != 0.0 || dir.y != 0.0 { dir.y.atan2(dir.x).to_degrees() } else { t.rotation_angle.to_degrees() };
                let text = clean(&format!("{}{}", t.extended_text.concat(), t.text));
                let lines = text.lines().count().max(1) as f64;
                let block = t.initial_text_height * (1.0 + (lines - 1.0) * LINE_SPACING * t.line_spacing_factor.max(0.25));
                // How far the first baseline is below the insertion point.
                let drop = match t.attachment_point {
                    AttachmentPoint::TopLeft | AttachmentPoint::TopCenter | AttachmentPoint::TopRight => t.initial_text_height,
                    AttachmentPoint::MiddleLeft | AttachmentPoint::MiddleCenter | AttachmentPoint::MiddleRight => t.initial_text_height - block / 2.0,
                    _ => t.initial_text_height - block,
                };
                // ponytail: centred and right-aligned MTEXT starts at the insertion point too; measure the line widths if it matters.
                return self.text(layer, rgb, &text, xy(&t.insertion_point), t.initial_text_height, rotation, drop, &m);
            }
            EntityType::Insert(ins) => return self.insert(ins, m, Ctx { layer, rgb }, depth),
            EntityType::ModelPoint(_) => return,
            other => {
                let name = format!("{other:?}");
                return self.skip(&name[..name.find(['(', ' ']).unwrap_or(name.len())].to_uppercase());
            }
        };
        if let Some(c) = contour {
            self.pieces.entry((layer, rgb)).or_default().push(Piece::Contour(c));
        }
    }

    /// Block contents, placed (and repeated in rows and columns) as the INSERT says.
    fn insert(&mut self, ins: &dxf::entities::Insert, m: &Affine, ctx: Ctx, depth: usize) {
        let Some(block) = self.blocks.get(ins.name.as_str()).copied() else { return self.skip("INSERT of a missing block") };
        if depth >= MAX_DEPTH {
            return self.skip("INSERT nested too deep");
        }
        // Base point to origin, scale, then the array offset, rotation and position.
        let base = xy(&block.base_point);
        let (sx, sy) = (ins.x_scale_factor, ins.y_scale_factor);
        let place = Affine::rotate(ins.rotation).then(&Affine([1.0, 0.0, 0.0, 1.0, ins.location.x, ins.location.y])).then(&ocs(m, &ins.extrusion_direction));
        for col in 0..ins.column_count.max(1) {
            for row in 0..ins.row_count.max(1) {
                let (ox, oy) = (col as f64 * ins.column_spacing, row as f64 * ins.row_spacing);
                let local = Affine([sx, 0.0, 0.0, sy, ox - sx * base[0], oy - sy * base[1]]).then(&place);
                for e in &block.entities {
                    self.entity(e, &local, Some(&ctx), depth + 1);
                }
            }
        }
    }

    /// Text lines as Scriber text items; `drop` is how far the first baseline is below `at`.
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, layer: String, rgb: [u8; 3], text: &str, at: Point, height: f64, rotation: f64, drop: f64, m: &Affine) {
        let (s, c) = rotation.to_radians().sin_cos();
        let (dir, up) = (m.linear([c, s]), m.linear([-s, c]));
        let size = height * up[0].hypot(up[1]) / CAP;
        if !(size.is_finite() && size > 0.0) {
            return;
        }
        for (k, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let down = drop + k as f64 * height * LINE_SPACING;
            let p = m.apply([at[0] + s * down, at[1] - c * down]);
            let shape = Shape::Text { x: p[0], y: p[1], size, text: line.into(), rotation: dir[1].atan2(dir[0]).to_degrees(), font: DEFAULT_FONT.into(), bold: false, italic: false };
            self.pieces.entry((layer.clone(), rgb)).or_default().push(Piece::Text(shape));
        }
    }
}

/// TEXT and MTEXT without their formatting codes; MTEXT paragraphs become lines.
fn clean(s: &str) -> String {
    let s = s.replace("%%c", "Ø").replace("%%C", "Ø").replace("%%d", "°").replace("%%D", "°").replace("%%p", "±").replace("%%P", "±").replace("%%u", "").replace("%%U", "").replace("%%o", "").replace("%%O", "").replace("%%%", "%");
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(ch) = it.next() {
        match ch {
            '{' | '}' => {}
            '\\' => match it.next() {
                Some('P') => out.push('\n'),
                Some('~') => out.push(' '),
                Some(c @ ('\\' | '{' | '}')) => out.push(c),
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k' | 'N') | None => {}
                // Stacked fraction \Sa^b; or \Sa/b;
                Some('S') => {
                    let frac: String = it.by_ref().take_while(|c| *c != ';').collect();
                    out.push_str(&frac.replace(['^', '#'], "/"));
                }
                // Codes with a value up to ';': font, height, colour, width, …
                Some(_) => while it.next().is_some_and(|c| c != ';') {},
            },
            c => out.push(c),
        }
    }
    out
}

/// A contour through `pts` (each a point and the bulge to the next one) in the entity's own
/// coordinates, mapped by `m`. Arcs stay arcs unless `m` stretches them into ellipses.
fn polyline(pts: &[(Point, f64)], closed: bool, m: &Affine, tol: f64) -> Option<Contour> {
    let (first, _) = pts.first()?;
    let mut pen = Pen::new(m, tol, *first);
    let n = pts.len();
    for i in 0..if closed { n } else { n - 1 } {
        let ((a, bulge), b) = (pts[i], pts[(i + 1) % n].0);
        pen.arc(a, b, bulge);
    }
    pen.finish(closed)
}

/// An arc (or full circle, sweep 360) counterclockwise from `start`° in the entity's coordinates.
fn circle_arc(c: Point, r: f64, start: f64, sweep: f64, m: &Affine, tol: f64) -> Option<Contour> {
    if !(r > 0.0) {
        return None;
    }
    let full = sweep >= 360.0 - 1e-9;
    // Halves: bulges stay small.
    let parts = if full || sweep > 180.0 { 2 } else { 1 };
    let at = |deg: f64| [c[0] + r * deg.to_radians().cos(), c[1] + r * deg.to_radians().sin()];
    let bulge = (sweep / parts as f64 / 4.0).to_radians().tan();
    let mut pts: Vec<(Point, f64)> = (0..parts).map(|k| (at(start + sweep * k as f64 / parts as f64), bulge)).collect();
    if !full {
        pts.push((at(start + sweep), 0.0));
    }
    polyline(&pts, full, m, tol)
}

fn ellipse(el: &dxf::entities::Ellipse, m: &Affine, tol: f64) -> Option<Contour> {
    let (c, u, n) = (xy(&el.center), [el.major_axis.x, el.major_axis.y], el.normal.z.signum());
    let v = [-n * u[1] * el.minor_axis_ratio, n * u[0] * el.minor_axis_ratio];
    let (t0, mut t1) = (el.start_parameter, el.end_parameter);
    if t1 <= t0 {
        t1 += TAU;
    }
    let full = (t1 - t0 - TAU).abs() < 1e-9 || t1 - t0 >= TAU;
    let t1 = if full { t0 + TAU } else { t1 };
    let f = |t: f64| [c[0] + u[0] * t.cos() + v[0] * t.sin(), c[1] + u[1] * t.cos() + v[1] * t.sin()];
    let df = |t: f64| [-u[0] * t.sin() + v[0] * t.cos(), -u[1] * t.sin() + v[1] * t.cos()];
    let mut pen = Pen::new(m, tol, f(t0));
    pen.curve(&f, &df, t0, t1, ((t1 - t0) / FRAC_PI_2).ceil().max(1.0) as usize);
    pen.finish(full)
}

/// A NURBS curve (any degree, weights honoured), or one through its fit points when the file
/// gives only those (as a smooth Catmull-Rom curve, not AutoCAD's exact fit).
fn spline(s: &Spline, m: &Affine, tol: f64) -> Option<Contour> {
    let ctrl: Vec<Point> = s.control_points.iter().map(xy).collect();
    let p = s.degree_of_curve.max(1) as usize;
    let knots = &s.knot_values;
    if ctrl.len() > p && knots.len() == ctrl.len() + p + 1 {
        let w: Vec<f64> = if s.weight_values.len() == ctrl.len() { s.weight_values.clone() } else { vec![1.0; ctrl.len()] };
        let mut pen = Pen::new(m, tol, de_boor(&ctrl, &w, knots, p, p, knots[p]));
        for k in p..ctrl.len() {
            let (a, b) = (knots[k], knots[k + 1]);
            if b - a <= 1e-12 * (1.0 + b.abs()) {
                continue;
            }
            let f = |t: f64| de_boor(&ctrl, &w, knots, p, k, t);
            let h = (b - a) * 1e-5;
            let df = |t: f64| {
                let (q, r) = (f(t + h), f(t - h));
                [(q[0] - r[0]) / (2.0 * h), (q[1] - r[1]) / (2.0 * h)]
            };
            pen.curve(&f, &df, a, b, 1);
        }
        let closed = pen.ends_meet();
        return pen.finish(closed);
    }
    let fit: Vec<Point> = s.fit_points.iter().map(xy).collect();
    if fit.len() < 2 {
        return None;
    }
    let mut pen = Pen::new(m, tol, fit[0]);
    let last = fit.len() - 1;
    for i in 0..last {
        let tangent = |j: usize| {
            let (a, b) = (fit[j.saturating_sub(1)], fit[(j + 1).min(last)]);
            let k = if j == 0 || j == last { 1.0 } else { 0.5 };
            [(b[0] - a[0]) * k / 3.0, (b[1] - a[1]) * k / 3.0]
        };
        let (a, b, ta, tb) = (fit[i], fit[i + 1], tangent(i), tangent(i + 1));
        pen.bezier([a, [a[0] + ta[0], a[1] + ta[1]], [b[0] - tb[0], b[1] - tb[1]], b]);
    }
    pen.finish(false)
}

/// The point at `t` on knot span `k` (de Boor, in homogeneous coordinates for the weights).
/// Works a little beyond the span too: it is the span's own polynomial.
fn de_boor(ctrl: &[Point], w: &[f64], knots: &[f64], p: usize, k: usize, t: f64) -> Point {
    let mut d: Vec<[f64; 3]> = (0..=p).map(|j| {
        let (c, w) = (ctrl[j + k - p], w[j + k - p]);
        [c[0] * w, c[1] * w, w]
    }).collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let den = knots[i + p + 1 - r] - knots[i];
            let a = if den.abs() < 1e-300 { 0.0 } else { (t - knots[i]) / den };
            for n in 0..3 {
                d[j][n] = (1.0 - a) * d[j - 1][n] + a * d[j][n];
            }
        }
    }
    let [x, y, w] = d[p];
    [x / w, y / w]
}

/// Builds a contour in mm from pieces given in the entity's own coordinates.
struct Pen<'a> {
    m: &'a Affine,
    tol: f64,
    v: Vec<Vertex>,
    segs: Vec<biarc::Seg>,
}

impl<'a> Pen<'a> {
    fn new(m: &'a Affine, tol: f64, start: Point) -> Self {
        let p = m.apply(start);
        Self { m, tol, v: vec![Vertex::new(p[0], p[1], 0.0)], segs: Vec::new() }
    }

    /// A line (bulge 0) or arc from `a` to `b`.
    fn arc(&mut self, a: Point, b: Point, bulge: f64) {
        if bulge.abs() < 1e-12 || self.m.similar() {
            // A mirror (the y flip) turns arcs the other way.
            biarc::push(&mut self.v, self.m.apply(b), bulge * self.m.det().signum());
            return;
        }
        // Stretched: the arc becomes part of an ellipse.
        let (c, r, start, sweep) = arc_params(a, b, bulge);
        let f = |t: f64| [c[0] + r * t.cos(), c[1] + r * t.sin()];
        let df = |t: f64| [-r * t.sin(), r * t.cos()];
        self.curve(&f, &df, start, start + sweep, (sweep.abs() / FRAC_PI_2).ceil().max(1.0) as usize);
    }

    /// The curve `f` (derivative `df`) from `t0` to `t1`, cut in `parts` to begin with: cubic
    /// pieces within half the tolerance, each then fitted as arcs within the other half.
    fn curve(&mut self, f: &dyn Fn(f64) -> Point, df: &dyn Fn(f64) -> Point, t0: f64, t1: f64, parts: usize) {
        let tol = self.tol / 2.0 / self.m.scale();
        let mut beziers = Vec::new();
        for k in 0..parts {
            let (a, b) = (t0 + (t1 - t0) * k as f64 / parts as f64, t0 + (t1 - t0) * (k + 1) as f64 / parts as f64);
            hermite(f, df, a, b, tol, 0, &mut beziers);
        }
        for bz in beziers {
            self.bezier(bz);
        }
    }

    fn bezier(&mut self, bz: [Point; 4]) {
        let bz = bz.map(|p| self.m.apply(p));
        self.segs.clear();
        biarc::fit_cubic(bz, self.tol / 2.0, &mut self.segs);
        for (to, bulge) in &self.segs {
            biarc::push(&mut self.v, *to, *bulge);
        }
    }

    fn ends_meet(&self) -> bool {
        let (a, b) = (self.v[0], self.v[self.v.len() - 1]);
        distance([a.x, a.y], [b.x, b.y]) < 1e-6
    }

    fn finish(self, closed: bool) -> Option<Contour> {
        let mut v = self.v;
        if closed && v.len() > 1 && distance([v[0].x, v[0].y], [v[v.len() - 1].x, v[v.len() - 1].y]) < 1e-6 {
            v.pop();
        }
        (v.len() >= 2).then(|| Contour { vertices: biarc::merge_arcs(v, closed, self.tol), closed })
    }
}

/// Cubic Béziers that follow `f` from `a` to `b` within `tol` (each matches the curve's ends and
/// tangents; halved until it is close enough at seven samples).
fn hermite(f: &dyn Fn(f64) -> Point, df: &dyn Fn(f64) -> Point, a: f64, b: f64, tol: f64, depth: u32, out: &mut Vec<[Point; 4]>) {
    let h = (b - a) / 3.0;
    let (p0, p3, d0, d3) = (f(a), f(b), df(a), df(b));
    let bz = [p0, [p0[0] + d0[0] * h, p0[1] + d0[1] * h], [p3[0] - d3[0] * h, p3[1] - d3[1] * h], p3];
    let close = (1..8).all(|i| {
        let s = i as f64 / 8.0;
        distance(cubic(&bz, s), f(a + (b - a) * s)) <= tol
    });
    if close || depth >= 16 {
        out.push(bz);
        return;
    }
    let mid = (a + b) / 2.0;
    hermite(f, df, a, mid, tol, depth + 1, out);
    hermite(f, df, mid, b, tol, depth + 1, out);
}

fn cubic(p: &[Point; 4], t: f64) -> Point {
    let u = 1.0 - t;
    let k = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    [(0..4).map(|i| k[i] * p[i][0]).sum(), (0..4).map(|i| k[i] * p[i][1]).sum()]
}

/// A group's pieces as shapes: open contours that touch end to end are joined (closing when the
/// chain comes round), and each closed contour goes into the item of the outermost closed contour
/// around it (a compound path: holes stay holes when engraved).
fn shapes(pieces: Vec<Piece>) -> Vec<Shape> {
    let mut chains = Document::new(1.0, 1.0);
    let (mut open, mut closed, mut out) = (Vec::new(), Vec::new(), Vec::new());
    for p in pieces {
        match p {
            Piece::Contour(c) if c.closed => closed.push(c),
            Piece::Contour(c) => open.push(chains.add("", Op::Cut, Shape::path(c))),
            Piece::Text(t) => out.push(t),
        }
    }
    // ponytail: join_chains scans all items per step, O(n²); index the end points if huge files are slow.
    for id in chains.join_chains(&open) {
        let c = chains.item(id).unwrap().shape.contours()[0].clone();
        if c.closed { closed.push(c) } else { out.push(Shape::path(c)) }
    }
    let polys: Vec<(Vec<Point>, BBox, f64)> = closed.iter().map(|c| (c.flatten(0.05), c.bbox(), c.area().abs())).collect();
    let inside = |i: usize, j: usize| {
        let ((pi, bi, ai), (pj, bj, aj)) = (&polys[i], &polys[j]);
        i != j && aj > ai && bi[0] >= bj[0] && bi[1] >= bj[1] && bi[0] + bi[2] <= bj[0] + bj[2] && bi[1] + bi[3] <= bj[1] + bj[3] && point_in_poly(pi[0], pj)
    };
    // ponytail: O(n²) containment test; a spatial index if files with thousands of closed shapes are slow.
    let outer: Vec<usize> = (0..closed.len()).map(|i| (0..closed.len()).filter(|&j| inside(i, j)).max_by(|a, b| polys[*a].2.total_cmp(&polys[*b].2)).unwrap_or(i)).collect();
    let mut compounds: BTreeMap<usize, Vec<Contour>> = BTreeMap::new();
    for (i, c) in closed.into_iter().enumerate() {
        compounds.entry(outer[i]).or_default().push(c);
    }
    out.extend(compounds.into_values().map(|contours| Shape::Path { contours }));
    out
}

