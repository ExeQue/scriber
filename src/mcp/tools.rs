//! The MCP tools: one per document command, mirroring what the GUI can do.

use crate::export;
use crate::model::constraints::attach;
use crate::model::{BoolOp, Contour, Document, Guide, Kind, Op, Ref, Shape, Shared, Transform, text, turn};
use base64::Engine;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::schemars::{self, JsonSchema};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

mod annotations;
mod editing;
mod items_file;
mod laser;
mod layers;
mod reply;
mod xtool;

use reply::{Detail, IdsDetail, changed};

const INSTRUCTIONS: &str = "Scriber is a 2D CAD program for laser cutters. You edit the document that is open in the user's Scriber window; every change shows there live and the user can undo it.
Units are millimetres. Origin is the top-left corner of the material, x grows right, y grows DOWN (like SVG). Angles are degrees, positive = clockwise on screen.
Layers, as in xTool Studio: every item sits on one layer, and the layer's op says what the laser does with it: cut (red, cut through along the line), score (blue, mark a line), engrave (black, burn the whole area inside closed paths, text and images; an open path on an engrave layer is scored) or no_output (grey, notes, dimensions and helpers for people, never lasered). A layer can also carry its own power/speed/passes (else the material library's preset for its op), so two engrave layers can engrave at different depths. Hidden layers are not lasered or exported; items on locked layers cannot be edited. list_layers, add_layer, update_layer, delete_layer manage them; add_items/update_item take a layer id, or an op, which means the first layer with that op (made if there is none). Example: a coaster with an engraved face is two items with the same shape, one on an engrave layer and one on a cut layer.
A path holds one or more contours; several contours make a compound path whose holes stay empty when engraved (even-odd). Use combine_items/break_apart to build or split them.
CAD operations: fillet, chamfer, offset (also kerf compensation), boolean (union/difference/intersection), array_rectangular, array_polar, trim, stretch, join, explode, align, break_item, extend, lengthen, match_properties, group/ungroup, select_similar and quick_select. Guide lines (add_items kind xline/ray/centerline/centermark) help placing things and are never lasered or exported.
Contours are polylines whose segments can be arcs: each vertex has a bulge = tan(sweep/4) for the segment to the next vertex (0 = straight). Prefer add_items with kind rect/circle/arc/polygon/line/polyline over building arcs by hand; for Bézier curves use kind curve (quadratic or cubic control points, turned into arcs), for SVG path data import_svg with data. Big batches: write the items array to a JSON file and give add_items its path.
Constraints (as in AutoCAD) bind geometry: add_constraint (coincident, parallel, tangent, …; the first object stays, the second moves), add_dimensional_constraint (a named parameter like d1 = 40 or d2 = d1 / 2 that drives the size), set_parameter, list_constraints, delete_constraints, auto_constrain. A reference names geometry in a path: {kind: point, item, vertex}, {kind: segment, item, segment} (segment k runs from vertex k to k+1; a circle is segment 0), {kind: centre, item, segment}, {kind: item, item}; contour defaults to 0. After every change the solver keeps all constraints satisfied, moving as little as possible; a change that cannot be satisfied is refused and undone, naming the conflicting constraints.
Start with get_document; it includes the user's current selection, and tools that take ids use the selection when ids is empty. Your changes leave the user's selection alone (items you add or copy are not selected); only select, select_similar, quick_select and add_items with select: true change it. A large document comes back as a summary without geometry (or ask for one with geometry: false); get_document with ids gives the geometry of particular items. Tools that change items reply compactly (per item id, name, layer, op, kind, bbox, rotation, contour and vertex counts); pass verbose: true for the full geometry. Call export_png without a path to look at the result.
Mirrored copy: transform_items with mirror_x/mirror_y and copy: true. boolean keeps the first item's layer (and so its op). import_svg maps colours to ops (shapes with neither fill nor stroke, or a grey or green fill, come in as no_output, useful as boolean cutters) and says which SVG element (svg_id, element index) each item came from.
Laser settings: list_machines and list_materials show the user's machines and material library; set_laser_setup picks the document's machine, laser module and material, and get_laser_setup returns the power/speed/passes to use per operation and per layer. Before the user lasers, run preflight_check and look at export_png with preview: true.
Import and share: search_xtool_materials and import_xtool_materials copy settings from xTool Studio's local material cache (only materials the user opened there), import_xtool_project from an .xs project; export_library and import_library write and merge .scrlib files for other Scriber users.
Review comments: the person can pin comments on the canvas (on a spot or on shapes) to tell you what to change. To follow a review, loop: wait_for_annotations (pass the revision from its last reply) blocks until the person comments, replies or reopens one, then act on each comment, and answer with reply_annotation (resolve: true when it is done, saying what you changed; ask in a reply when unsure). list_annotations shows them all (status: open for the open ones); add_annotation leaves a note for the person. Comments are not lasered, exported or undone with undo.";

// ---------- parameters ----------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShapeInput {
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    /// Rectangle with top-left corner (x, y); corner_radius rounds the corners.
    Rect { x: f64, y: f64, w: f64, h: f64, #[serde(default)] corner_radius: f64 },
    Circle { cx: f64, cy: f64, r: f64 },
    /// Arc around (cx, cy) from start to end degrees, clockwise on screen (0° = right, 90° = down).
    Arc { cx: f64, cy: f64, r: f64, start: f64, end: f64 },
    /// Regular polygon; r is the distance from centre to corners; the first corner points up before rotation.
    Polygon { cx: f64, cy: f64, r: f64, sides: u32, #[serde(default)] rotation: f64 },
    /// Straight segments through the points.
    Polyline { points: Vec<[f64; 2]>, #[serde(default)] closed: bool },
    /// Full control: one or more contours with arc bulges (several = compound path).
    Path { contours: Vec<Contour> },
    /// Bézier curves (as in SVG paths): from start, each segment goes to `to` straight (no controls),
    /// as a quadratic (one control point) or a cubic curve (two). Curves become arcs within tolerance
    /// mm (default 0.01). For SVG path data (a `d` string) use import_svg with data instead.
    Curve {
        start: [f64; 2],
        segments: Vec<CurveSegment>,
        #[serde(default)]
        closed: bool,
        tolerance: Option<f64>,
    },
    /// Text; (x, y) is the start of the baseline; size is the font size in mm. font is an installed family
    /// (see list_fonts) or sans-serif (default), serif or monospace. It is lasered as the glyph outlines.
    Text {
        x: f64,
        y: f64,
        size: f64,
        text: String,
        #[serde(default)]
        rotation: f64,
        font: Option<String>,
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        italic: bool,
    },
    /// A measurement from point 1 to point 2, drawn offset mm to the left of that direction.
    Dimension { x1: f64, y1: f64, x2: f64, y2: f64, #[serde(default = "default_dim_offset")] offset: f64 },
    /// Guide line (construction line, never lasered or exported): endless through (x, y) at angle degrees (clockwise, 0 = right).
    Xline { x: f64, y: f64, angle: f64 },
    /// Guide ray from (x, y) to infinity at angle degrees.
    Ray { x: f64, y: f64, angle: f64 },
    /// Guide centre line between two lines: give `of` (the two line ids; it then follows them) or the end points.
    Centerline {
        #[serde(default)]
        x1: f64,
        #[serde(default)]
        y1: f64,
        #[serde(default)]
        x2: f64,
        #[serde(default)]
        y2: f64,
        #[serde(default)]
        of: Vec<u64>,
    },
    /// Guide centre mark (cross) on a circle or arc: give `of` (its id; it then follows it) or centre and radius.
    Centermark {
        #[serde(default)]
        cx: f64,
        #[serde(default)]
        cy: f64,
        #[serde(default)]
        r: f64,
        of: Option<u64>,
    },
}

/// One segment of a curve.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CurveSegment {
    pub to: [f64; 2],
    /// 0 = straight line, 1 = quadratic, 2 = cubic Bézier control points.
    #[serde(default)]
    pub controls: Vec<[f64; 2]>,
}

fn default_dim_offset() -> f64 {
    5.0
}

impl ShapeInput {
    /// Default name and the shape.
    fn build(self) -> (&'static str, Shape) {
        use ShapeInput as S;
        match self {
            S::Line { x1, y1, x2, y2 } => ("line", Shape::path(Contour::line([x1, y1], [x2, y2]))),
            S::Rect { x, y, w, h, corner_radius } => ("rectangle", Shape::path(Contour::rect(x, y, w, h, corner_radius))),
            S::Circle { cx, cy, r } => ("circle", Shape::path(Contour::circle(cx, cy, r))),
            S::Arc { cx, cy, r, start, end } => ("arc", Shape::path(Contour::arc(cx, cy, r, start, end))),
            S::Polygon { cx, cy, r, sides, rotation } => ("polygon", Shape::path(Contour::polygon(cx, cy, r, sides, rotation))),
            S::Polyline { points, closed } => ("polyline", Shape::path(Contour::polyline(&points, closed))),
            S::Path { contours } => ("path", Shape::Path { contours }),
            S::Curve { start, segments, closed, tolerance } => {
                let segments: Vec<_> = segments.into_iter().map(|s| (s.controls, s.to)).collect();
                let contour = export::svg_import::bezier_contour(start, &segments, closed, tolerance.unwrap_or(0.01));
                ("curve", Shape::Path { contours: contour.into_iter().collect() })
            }
            S::Text { x, y, size, text, rotation, font, bold, italic } => {
                let font = font.unwrap_or_else(|| text::DEFAULT_FONT.into());
                ("text", Shape::Text { x, y, size, text, rotation, font, bold, italic })
            }
            S::Dimension { x1, y1, x2, y2, offset } => ("dimension", Shape::Dimension { x1, y1, x2, y2, offset }),
            S::Xline { x, y, angle } => ("guide line", Shape::Guide(Guide::Xline { x, y, angle })),
            S::Ray { x, y, angle } => ("ray", Shape::Guide(Guide::Ray { x, y, angle })),
            S::Centerline { x1, y1, x2, y2, of } => ("centre line", Shape::Guide(Guide::Centerline { x1, y1, x2, y2, of })),
            S::Centermark { cx, cy, r, of } => ("centre mark", Shape::Guide(Guide::Centermark { cx, cy, r, of })),
        }
    }

    /// Why the input cannot be used: a text font that is not installed, a bad curve.
    fn input_error(&self) -> Option<String> {
        match self {
            ShapeInput::Text { font: Some(f), .. } => text::font_error(f),
            ShapeInput::Curve { start, segments, tolerance, .. } => {
                if segments.iter().all(|s| (s.to[0] - start[0]).hypot(s.to[1] - start[1]) < 1e-9) {
                    Some("a curve needs a segment that ends away from its start".into())
                } else if segments.iter().any(|s| s.controls.len() > 2) {
                    Some("a curve segment has 0, 1 or 2 control points".into())
                } else if tolerance.is_some_and(|t| !(t.is_finite() && t > 0.0)) {
                    Some("tolerance must be a positive number".into())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NewItem {
    pub shape: ShapeInput,
    /// Layer id (see list_layers); wins over op.
    pub layer: Option<u64>,
    /// Without a layer: the first layer with this op (made if there is none). Default cut; engrave
    /// for text; no_output for dimensions and guides.
    pub op: Option<Op>,
    /// Deprecated, kept for old clients: true means op engrave.
    pub fill: Option<bool>,
    pub name: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddItems {
    #[serde(default)]
    pub items: Vec<NewItem>,
    /// Or a JSON file (absolute path) holding the same items array (or {"items": [...]}), for big batches.
    pub path: Option<String>,
    /// Select the new items in the GUI (default false: the user's selection stays as it is).
    pub select: Option<bool>,
    /// Join the new items to what they touch, as drawing in the GUI does (INFER): coincident on
    /// ends and centres within 1e-6 mm of another item's, concentric for shared arc centres.
    /// Default false.
    pub infer: Option<bool>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateItem {
    pub id: u64,
    pub name: Option<String>,
    /// Move it to this layer (see list_layers); wins over op.
    pub layer: Option<u64>,
    /// Move it to the first layer with this op (made if there is none).
    pub op: Option<Op>,
    /// Deprecated, kept for old clients: true means op engrave.
    pub fill: Option<bool>,
    pub note: Option<String>,
    /// Replace the geometry.
    pub shape: Option<ShapeInput>,
    /// Text only: change the font family (see list_fonts), keeping everything else.
    pub font: Option<String>,
    /// Turn the item to this angle in degrees (clockwise positive) about the centre of its own box.
    /// get_document reports each item's rotation; a new shape starts at 0 (text and images: their rotation).
    pub rotation: Option<f64>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct Ids {
    /// Item ids; empty = the user's current selection.
    #[serde(default)]
    pub ids: Vec<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OneId {
    /// Item id; missing = the single selected item.
    pub id: Option<u64>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetDocument {
    /// Only these items; empty = all. Picked items come with their geometry unless geometry is false.
    #[serde(default)]
    pub ids: Vec<u64>,
    /// true: every item's geometry; false: summary only (id, name, layer, op, kind, bbox, rotation,
    /// contour and vertex counts). Default: geometry, unless the reply would be very large.
    pub geometry: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransformItems {
    #[serde(default)]
    pub ids: Vec<u64>,
    #[serde(flatten)]
    pub transform: Transform,
    /// Transform copies and leave the originals where they are (e.g. mirror_x with copy for a mirrored copy).
    #[serde(default)]
    pub copy: bool,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Duplicate {
    #[serde(default)]
    pub ids: Vec<u64>,
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Measure {
    #[serde(default)]
    pub ids: Vec<u64>,
    /// Also measure the straight distance between two points.
    pub from: Option<[f64; 2]>,
    pub to: Option<[f64; 2]>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Notes {
    pub notes: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Canvas {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportImage {
    /// File to import (PNG, JPEG, WebP, BMP, GIF, TIFF).
    pub path: Option<String>,
    /// Or the image bytes, base64 encoded.
    pub data_base64: Option<String>,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    /// Width in mm; height follows the aspect ratio. Default 10 px per mm, capped to the material.
    pub width: Option<f64>,
    pub name: Option<String>,    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportSvg {
    /// SVG file to import.
    pub path: Option<String>,
    /// Or the SVG itself: its text, or base64.
    pub data: Option<String>,
    /// Left edge of the drawing in mm; default: where the file puts it.
    pub x: Option<f64>,
    /// Top edge of the drawing in mm; default: where the file puts it.
    pub y: Option<f64>,
    /// Scale the drawing to this width in mm (overrides scale).
    pub width: Option<f64>,
    /// Extra factor on the file's own size; 1.333 (96/72) for 72 DPI files. Default 1.
    pub scale: Option<f64>,
    /// Largest distance in mm between a curve and the arcs that replace it. Default 0.01.
    pub tolerance: Option<f64>,    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportSvg {
    pub path: String,
    /// Include no_output items (notes, dimensions, helpers). Default false, because laser software would burn them.
    #[serde(default)]
    pub include_annotations: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportPng {
    /// Where to write the PNG. Without a path the image is returned so you can look at it.
    pub path: Option<String>,
    /// Resolution; default: 2048 px on the material's longest side. Images returned inline stay under 8000 px.
    pub dpi: Option<f64>,
    /// Default true.
    pub include_annotations: Option<bool>,
    /// Render what the laser makes instead of the drawing: material colour, burned areas, thin cut and
    /// score lines, cut-out pieces shadowed with their holes open; no-output items and guides hidden.
    #[serde(default)]
    pub preview: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SavePath {
    /// Path to the .scrbr file; missing = the current file.
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenFile {
    pub path: String,
    /// Discard unsaved changes.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NewDoc {
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// Discard unsaved changes.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Fillet {
    /// Item ids; empty = the selection. Two open lines/polylines are joined where they meet, and that corner is rounded.
    #[serde(default)]
    pub ids: Vec<u64>,
    /// Arc radius in mm (0 with two open items just joins them into a sharp corner).
    pub radius: f64,
    /// Only round the corner nearest this point; missing = every sharp corner.
    pub at: Option<[f64; 2]>,
    /// Two-object form (AutoCAD FILLET/CHAMFER picking two objects): the first object and where it
    /// was picked; needs `second` too, and then `ids` and `at` are ignored.
    pub first: Option<ObjectPick>,
    /// The second object and where it was picked.
    pub second: Option<ObjectPick>,
    #[serde(flatten)]
    pub detail: Detail,
}

/// An object picked at a point: the point chooses the segment, the side kept and which corner is meant.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ObjectPick {
    pub id: u64,
    pub at: [f64; 2],
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Chamfer {
    /// Item ids; empty = the selection. Two open lines/polylines are joined where they meet, and that corner is cut.
    #[serde(default)]
    pub ids: Vec<u64>,
    /// How far back along the incoming side (or the first item) the cut starts, in mm.
    pub distance: f64,
    /// Along the outgoing side (or the second item); default = distance.
    pub distance2: Option<f64>,
    /// Only cut the corner nearest this point; missing = every sharp corner.
    pub at: Option<[f64; 2]>,
    /// Two-object form (AutoCAD FILLET/CHAMFER picking two objects): the first object and where it
    /// was picked; needs `second` too, and then `ids` and `at` are ignored.
    pub first: Option<ObjectPick>,
    /// The second object and where it was picked.
    pub second: Option<ObjectPick>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Offset {
    #[serde(default)]
    pub ids: Vec<u64>,
    /// mm. Positive = outward for closed shapes (holes shrink), to the left on screen for open ones; negative = inward / right.
    pub distance: f64,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BooleanItems {
    /// Two or more closed paths; empty = the selection. For difference the first one is the base.
    #[serde(default)]
    pub ids: Vec<u64>,
    pub operation: BoolOp,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArrayRect {
    #[serde(default)]
    pub ids: Vec<u64>,
    pub rows: u32,
    pub columns: u32,
    /// Distance between columns in mm (negative = to the left).
    #[serde(default)]
    pub dx: f64,
    /// Distance between rows in mm (positive = down).
    #[serde(default)]
    pub dy: f64,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArrayPolar {
    #[serde(default)]
    pub ids: Vec<u64>,
    /// Items in total, the original included.
    pub count: u32,
    pub centre: [f64; 2],
    /// Degrees to spread them over, positive = clockwise; default 360 (evenly round).
    pub angle: Option<f64>,
    /// Turn the copies with the angle (default true); false only moves them.
    pub rotate: Option<bool>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Trim {
    /// The item to trim; missing = the single selected item.
    pub id: Option<u64>,
    /// A point on the piece to remove.
    pub at: [f64; 2],
    /// Cutting edges; empty = every other path.
    #[serde(default)]
    pub edges: Vec<u64>,
    /// Keep each new end on the edge it was trimmed against (a coincident constraint, as INFER
    /// does while drawing). Default true.
    pub constrain: Option<bool>,
    #[serde(flatten)]
    pub detail: Detail,
}

// ---------- server ----------

#[derive(Clone)]
pub struct Server {
    pub(super) doc: Shared,
    repaint: Arc<dyn Fn() + Send + Sync>,
    tool_router: ToolRouter<Self>,
}

pub(super) type ToolResult = Result<CallToolResult, McpError>;

pub(super) fn ok(v: Value) -> ToolResult {
    Ok(CallToolResult::success(vec![ContentBlock::text(v.to_string())]))
}

pub(super) fn fail(msg: impl std::fmt::Display) -> ToolResult {
    Ok(CallToolResult::error(vec![ContentBlock::text(msg.to_string())]))
}

impl Server {
    pub fn new(doc: Shared, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { doc, repaint, tool_router: Self::tool_router() + Self::constraint_router() + Self::laser_router() + Self::editing_router() + Self::layers_router() + Self::xtool_router() + Self::annotations_router() }
    }

    /// Run `f` on the locked document, then repaint the GUI.
    /// The document settles its constraints afterwards; a change that breaks them is undone and
    /// the tool fails with the conflicting constraints.
    /// The person's selection stays as it was (minus items that are gone or now locked or hidden):
    /// items an agent adds never replace it. Tools that select on purpose use `with_select`.
    pub(super) fn with(&self, f: impl FnOnce(&mut Document) -> ToolResult) -> ToolResult {
        self.with_select(false, f)
    }

    /// Like `with`; `selects` = the tool sets the selection on purpose, so it is kept.
    pub(super) fn with_select(&self, selects: bool, f: impl FnOnce(&mut Document) -> ToolResult) -> ToolResult {
        let mut d = self.doc.lock().unwrap();
        let before = d.selection.clone();
        let r = f(&mut d);
        let settled = d.settle(true);
        if !selects {
            let s = &d.s;
            let kept = before.into_iter().filter(|id| s.items.iter().any(|i| i.id == *id && s.editable(i))).collect();
            d.selection = kept;
        }
        drop(d);
        (self.repaint)();
        match settled {
            Ok(()) => r,
            Err(e) => fail(format!("refused: {e}; the change was undone")),
        }
    }

    /// Run a two-object fillet or chamfer.
    fn edit_objects(&self, picks: Result<Picks, &str>, verbose: bool, f: impl FnOnce(&mut Document, (u64, [f64; 2]), (u64, [f64; 2])) -> Result<Vec<u64>, String>) -> ToolResult {
        let [a, b] = match picks {
            Ok(p) => p,
            Err(e) => return fail(e),
        };
        self.edit_ids(&[a.0, b.0], |d, _| match f(d, a, b) {
            Ok(ids) => ok(changed(d, &ids, verbose)),
            Err(e) => fail(e),
        })
    }

    /// Like `with`, for commands that act on ids (empty = selection) and change the document.
    pub(super) fn edit_ids(&self, ids: &[u64], f: impl FnOnce(&mut Document, Vec<u64>) -> ToolResult) -> ToolResult {
        self.with(|d| {
            let ids = if ids.is_empty() { d.selection.clone() } else { ids.to_vec() };
            if ids.is_empty() {
                return fail("no ids given and nothing is selected");
            }
            match d.resolve_editable(&ids) {
                Ok(ids) => {
                    d.checkpoint();
                    f(d, ids)
                }
                Err(e) => fail(e),
            }
        })
    }
}

#[tool_router]
impl Server {
    #[tool(description = "Get the document: material size, notes, layers, the user's selection, and every item (or only ids) with op, bounding box, rotation and its geometry. geometry: false gives a summary without geometry; a large document gets the summary unless geometry is set, with a note saying so: then ask for the geometry of particular ids.")]
    fn get_document(&self, Parameters(p): Parameters<GetDocument>) -> ToolResult {
        ok(reply::document(&self.doc.lock().unwrap(), &p.ids, p.geometry))
    }

    #[tool(description = "List the font families installed on this computer, for a text's font. sans-serif, serif and monospace also work and pick the system's default of that kind.")]
    fn list_fonts(&self) -> ToolResult {
        ok(json!({"fonts": text::families()}))
    }

    #[tool(description = "Select items in the GUI (empty list clears the selection).")]
    fn select(&self, Parameters(p): Parameters<Ids>) -> ToolResult {
        self.with_select(true, |d| match d.resolve_editable(&p.ids) {
            Ok(ids) => {
                d.selection = ids;
                d.rev += 1;
                ok(json!({"selection": d.selection}))
            }
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Add items (line, rect, circle, arc, polygon, polyline, path, curve (Bézier), text, dimension) and guide lines (xline, ray, centerline, centermark: construction lines for drawing, never lasered or exported), each on a layer (by id, or by op: the first layer with it). Returns ids and the items compactly (id, name, layer, op, kind, bbox, rotation, contour/vertex counts; verbose: true for geometry). Big batches: write the items array to a JSON file and pass its path instead of items. One undo step. The old fill: true is still accepted and means op engrave.")]
    fn add_items(&self, Parameters(mut p): Parameters<AddItems>) -> ToolResult {
        match (&p.path, p.items.is_empty()) {
            (Some(_), false) => return fail("give items or path, not both"),
            (Some(path), true) => match items_file::load(path) {
                Ok(items) => p.items = items,
                Err(e) => return fail(e),
            },
            (None, _) => {}
        }
        if p.items.is_empty() {
            return fail("no items given");
        }
        if let Some(e) = p.items.iter().find_map(|it| it.shape.input_error()) {
            return fail(e);
        }
        let select = p.select.unwrap_or(false);
        self.with_select(select, |d| {
            if let Some(l) = p.items.iter().filter_map(|it| it.layer).find(|l| d.s.layer(*l).is_none()) {
                return fail(format!("no layer with id {l}; see list_layers"));
            }
            d.checkpoint();
            let (mut ids, mut own) = (Vec::new(), Vec::new());
            for it in p.items {
                let (kind, shape) = it.shape.build();
                let default_op = match shape {
                    Shape::Dimension { .. } | Shape::Guide(_) => Op::NoOutput,
                    Shape::Text { .. } => Op::Engrave,
                    _ => Op::Cut,
                };
                let op = if it.fill == Some(true) { Op::Engrave } else { it.op.unwrap_or(default_op) };
                let layer = it.layer.unwrap_or_else(|| d.layer_for(op));
                let id = d.add_to(layer, it.name.as_deref().unwrap_or(kind), shape);
                d.item_mut(id).unwrap().note = it.note.unwrap_or_default();
                if kind == "polygon" {
                    own.push((Kind::Regular, vec![Ref::Item { item: id }]));
                }
                ids.push(id);
            }
            if select {
                d.selection = ids.clone();
            }
            let found: Vec<_> = if p.infer.unwrap_or(false) { ids.iter().flat_map(|id| attach::link_constraints(&d.s, *id, &attach::touching(&d.s, *id))).collect() } else { Vec::new() };
            let inferred = d.constrain_new(own, found);
            let mut out = changed(d, &ids, p.detail.verbose);
            out["inferred_constraints"] = json!(inferred);
            ok(out)
        })
    }

    #[tool(description = "Change an item's name, layer (or op: moves it to the first layer with that op), note, geometry, rotation (degrees, about its own centre) or (text only) font. The old fill: true is still accepted and means op engrave.")]
    fn update_item(&self, Parameters(p): Parameters<UpdateItem>) -> ToolResult {
        if let Some(e) = p.shape.as_ref().and_then(ShapeInput::input_error).or_else(|| p.font.as_deref().and_then(text::font_error)) {
            return fail(e);
        }
        let is_text = matches!(self.doc.lock().unwrap().item(p.id).map(|i| &i.shape), Some(Shape::Text { .. }));
        if p.font.is_some() && !is_text && !matches!(p.shape, Some(ShapeInput::Text { .. })) {
            return fail("font only applies to text items");
        }
        self.edit_ids(&[p.id], |d, _| {
            let op = if p.fill == Some(true) { Some(Op::Engrave) } else { p.op };
            let layer = match (p.layer, op) {
                (Some(l), _) if d.s.layer(l).is_none() => return fail(format!("no layer with id {l}; see list_layers")),
                (Some(l), _) => Some(l),
                (None, Some(op)) => Some(d.layer_for(op)),
                (None, None) => None,
            };
            let item = d.item_mut(p.id).unwrap();
            if let Some(n) = p.name {
                item.name = n;
            }
            if let Some(l) = layer {
                item.layer = l;
            }
            if let Some(n) = p.note {
                item.note = n;
            }
            if let Some(s) = p.shape {
                item.shape = s.build().1;
                item.rotation = turn::own_rotation(&item.shape);
            }
            if let (Some(f), Shape::Text { font, .. }) = (p.font, &mut item.shape) {
                *font = f;
            }
            if let Some(a) = p.rotation
                && let Err(e) = d.set_rotation(&[p.id], a)
            {
                return fail(e);
            }
            ok(changed(d, &[p.id], p.detail.verbose))
        })
    }

    #[tool(description = "Delete items (empty ids = the selection).")]
    fn delete_items(&self, Parameters(p): Parameters<Ids>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| {
            d.delete(&ids);
            ok(json!({"deleted": ids}))
        })
    }

    #[tool(description = "Move, rotate, scale and/or mirror items (empty ids = the selection). Order: mirror, scale, rotate around the pivot, then move by dx/dy. The pivot defaults to the centre of the items' bounding box. Each item's rotation (see get_document) adds the rotate angle; a mirror negates it; a non-uniform scale resets it to 0 unless the item's angle is a multiple of 90°. copy: true transforms copies and keeps the originals (a mirrored copy: mirror_x or mirror_y with copy).")]
    fn transform_items(&self, Parameters(p): Parameters<TransformItems>) -> ToolResult {
        self.edit_ids(&p.ids, |d, mut ids| {
            if p.copy {
                ids = d.duplicate(&ids, 0.0, 0.0);
            }
            d.transform(&ids, &p.transform);
            let mut out = changed(d, &ids, p.detail.verbose);
            out["bbox"] = json!(d.bbox(&ids));
            ok(out)
        })
    }

    #[tool(description = "Copy items, offset by dx/dy (empty ids = the selection).")]
    fn duplicate_items(&self, Parameters(p): Parameters<Duplicate>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| {
            let new = d.duplicate(&ids, p.dx, p.dy);
            ok(changed(d, &new, p.detail.verbose))
        })
    }

    #[tool(description = "Combine paths into one compound path (empty ids = the selection). Holes stay empty when engraved. Keeps the first item's name and op.")]
    fn combine_items(&self, Parameters(p): Parameters<IdsDetail>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.combine(&ids) {
            Ok(id) => ok(changed(d, &[id], p.detail.verbose)),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Split a compound path into one item per contour.")]
    fn break_apart(&self, Parameters(p): Parameters<OneId>) -> ToolResult {
        let ids: Vec<u64> = p.id.into_iter().collect();
        self.edit_ids(&ids, |d, ids| match ids.as_slice() {
            [id] => match d.break_apart(*id) {
                Ok(new) => ok(changed(d, &new, p.detail.verbose)),
                Err(e) => fail(e),
            },
            _ => fail("give one id or select exactly one item"),
        })
    }

    #[tool(description = "Measure items (empty ids = the selection, or everything when nothing is selected): bounding box, outline length and area per item, plus the total laser path length per op. Pass from/to to measure a distance.")]
    fn measure(&self, Parameters(p): Parameters<Measure>) -> ToolResult {
        let d = self.doc.lock().unwrap();
        let ids: Vec<u64> = match (p.ids.is_empty(), d.selection.is_empty()) {
            (true, true) => d.s.items.iter().map(|i| i.id).collect(),
            (true, false) => d.selection.clone(),
            _ => match d.resolve(&p.ids) {
                Ok(ids) => ids,
                Err(e) => return fail(e),
            },
        };
        let mut totals = serde_json::Map::new();
        let items: Vec<Value> = d
            .s
            .items
            .iter()
            .filter(|i| ids.contains(&i.id))
            .map(|i| {
                let m = i.shape.metrics();
                // Open engrave paths are scored, so they count as score length.
                let op = d.s.effective_op(i);
                if matches!(op, Op::Cut | Op::Score) && matches!(i.shape, Shape::Path { .. } | Shape::Text { .. }) {
                    let key = format!("{}_length", serde_json::to_value(op).unwrap().as_str().unwrap());
                    let t = totals.get(&key).and_then(Value::as_f64).unwrap_or(0.0);
                    totals.insert(key, json!(t + m.length));
                }
                if d.s.engraves_area(i) {
                    let t = totals.get("engrave_area").and_then(Value::as_f64).unwrap_or(0.0);
                    totals.insert("engrave_area".into(), json!(t + m.area.unwrap_or(0.0)));
                }
                json!({"id": i.id, "name": i.name, "layer": i.layer, "op": d.s.op(i), "metrics": m})
            })
            .collect();
        let mut out = json!({"items": items, "bbox": d.bbox(&ids), "totals": totals});
        if let (Some(a), Some(b)) = (p.from, p.to) {
            out["distance"] = json!((b[0] - a[0]).hypot(b[1] - a[1]));
            out["angle"] = json!((b[1] - a[1]).atan2(b[0] - a[0]).to_degrees());
        }
        ok(out)
    }

    #[tool(description = "Replace the document's free-form notes (shown in the GUI's notes panel; saved with the file).")]
    fn set_notes(&self, Parameters(p): Parameters<Notes>) -> ToolResult {
        self.with(|d| {
            d.checkpoint();
            d.s.notes = p.notes;
            ok(json!({"ok": true}))
        })
    }

    #[tool(description = "Set the material (canvas) size in mm.")]
    fn set_canvas(&self, Parameters(p): Parameters<Canvas>) -> ToolResult {
        if p.width <= 0.0 || p.height <= 0.0 {
            return fail("width and height must be positive");
        }
        self.with(|d| {
            d.checkpoint();
            d.s.width = p.width;
            d.s.height = p.height;
            ok(json!({"width": p.width, "height": p.height}))
        })
    }

    #[tool(description = "Import an image from a file or base64 data. It is converted to greyscale and placed as an engraved item. Returns the item.")]
    fn import_image(&self, Parameters(p): Parameters<ImportImage>) -> ToolResult {
        let bytes = match (&p.path, &p.data_base64) {
            (Some(path), _) => std::fs::read(path).map_err(anyhow::Error::from),
            (None, Some(b64)) => base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(anyhow::Error::from),
            _ => Err(anyhow::anyhow!("give path or data_base64")),
        };
        let img = match bytes.and_then(|b| export::image::to_grey(&b)) {
            Ok(i) => i,
            Err(e) => return fail(format!("cannot import image: {e}")),
        };
        let file_name = p.path.as_deref().and_then(|s| PathBuf::from(s).file_name().map(|f| f.to_string_lossy().into_owned()));
        let name = p.name.or(file_name).unwrap_or_else(|| "image".into());
        self.with(|d| {
            d.checkpoint();
            let id = d.place_image(img, [p.x, p.y], p.width, &name);
            ok(changed(d, &[id], p.detail.verbose))
        })
    }

    #[tool(description = "Export the document as SVG in mm for laser software (red = cut, blue = score, black = engrave lines and filled areas), one group (Inkscape layer) per shown layer in laser order; hidden layers are left out.")]
    fn export_svg(&self, Parameters(p): Parameters<ExportSvg>) -> ToolResult {
        let svg = export::svg(&self.doc.lock().unwrap().s, p.include_annotations);
        match std::fs::write(&p.path, svg) {
            Ok(()) => ok(json!({"path": p.path})),
            Err(e) => fail(format!("cannot write {}: {e}", p.path)),
        }
    }

    #[tool(description = "Render the document to PNG (preview: true shows what the laser makes). With a path it is written to that file; without, the image is returned so you can see the drawing.")]
    fn export_png(&self, Parameters(p): Parameters<ExportPng>) -> ToolResult {
        let state = self.doc.lock().unwrap().s.clone();
        let dpi = export::png_dpi(&state, p.dpi, p.path.is_none());
        let png = if p.preview { export::preview_png(&state, dpi) } else { export::png(&state, dpi, p.include_annotations.unwrap_or(true)) };
        let png = match png {
            Ok(png) => png,
            Err(e) => return fail(e),
        };
        match p.path {
            Some(path) => match std::fs::write(&path, png) {
                Ok(()) => ok(json!({"path": path, "dpi": dpi})),
                Err(e) => fail(format!("cannot write {path}: {e}")),
            },
            None => Ok(CallToolResult::success(vec![ContentBlock::image(base64::engine::general_purpose::STANDARD.encode(png), "image/png")])),
        }
    }

    #[tool(description = "Undo the last change (the user's or yours).")]
    fn undo(&self) -> ToolResult {
        self.with(|d| ok(json!({"undone": d.undo()})))
    }

    #[tool(description = "Redo the last undone change.")]
    fn redo(&self) -> ToolResult {
        self.with(|d| ok(json!({"redone": d.redo()})))
    }

    #[tool(description = "Save the document as a .scrbr file (without path: the current file).")]
    fn save(&self, Parameters(p): Parameters<SavePath>) -> ToolResult {
        self.with(|d| {
            let Some(path) = p.path.map(PathBuf::from).or_else(|| d.path.clone()) else {
                return fail("the document has no file yet; give a path");
            };
            match d.save(&path) {
                Ok(()) => ok(json!({"path": path})),
                Err(e) => fail(e),
            }
        })
    }

    #[tool(description = "Open a .scrbr file, replacing the current document. Fails on unsaved changes unless force is set.")]
    fn open(&self, Parameters(p): Parameters<OpenFile>) -> ToolResult {
        self.with_select(true, |d| {
            if d.dirty && !p.force {
                return fail("the current document has unsaved changes; save it first or pass force: true");
            }
            match Document::open(&PathBuf::from(&p.path)) {
                Ok(new) => {
                    *d = new;
                    d.rev += 1;
                    ok(reply::document(d, &[], None))
                }
                Err(e) => fail(e),
            }
        })
    }

    #[tool(description = "Start a new empty document (material size in mm, default 300 × 200). Fails on unsaved changes unless force is set.")]
    fn new_document(&self, Parameters(p): Parameters<NewDoc>) -> ToolResult {
        self.with_select(true, |d| {
            if d.dirty && !p.force {
                return fail("the current document has unsaved changes; save it first or pass force: true");
            }
            *d = Document::new(p.width.unwrap_or(300.0), p.height.unwrap_or(200.0));
            d.rev += 1;
            ok(reply::document(d, &[], None))
        })
    }

    #[tool(description = "Round corners with an arc (empty ids = the selection): the corner nearest `at`, or every sharp corner between straight segments. With exactly two open lines/polylines, they are extended or shortened to meet, joined into one path, and that corner is rounded (AutoCAD FILLET of two objects). With `first` and `second` (each an id and the point it was picked at), fillets any two objects as AutoCAD does: lines, arcs, circles and end segments of open polylines (lines touching end to end count as one polyline); the picks choose the kept sides and which arc is meant, lines and arcs are trimmed or extended to the arc, circles stay, two open paths become one, parallel lines get a half-circle cap, radius 0 makes a sharp corner. Returns the new and changed ids.")]
    fn fillet(&self, Parameters(p): Parameters<Fillet>) -> ToolResult {
        if let Some(picks) = two_objects(&p.first, &p.second) {
            return self.edit_objects(picks, p.detail.verbose, |d, a, b| d.fillet_objects(a, b, p.radius));
        }
        self.edit_ids(&p.ids, |d, ids| match d.fillet(&ids, p.radius, p.at) {
            Ok(()) => ok(changed(d, &existing(d, &ids), p.detail.verbose)),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Cut corners off with a straight line (empty ids = the selection): the corner nearest `at`, or every sharp corner. distance goes back along the incoming side, distance2 (default = distance) along the outgoing one. Two open lines/polylines are joined where they meet, like fillet. With `first` and `second`, chamfers two picked objects as AutoCAD does (lines, arcs and end segments of open polylines; not circles): distance from the corner along the first (along the arc for arcs), distance2 along the second; 0 and 0 makes a sharp corner. Returns the new and changed ids.")]
    fn chamfer(&self, Parameters(p): Parameters<Chamfer>) -> ToolResult {
        if let Some(picks) = two_objects(&p.first, &p.second) {
            return self.edit_objects(picks, p.detail.verbose, |d, a, b| d.chamfer_objects(a, b, p.distance, p.distance2.unwrap_or(p.distance)));
        }
        self.edit_ids(&p.ids, |d, ids| match d.chamfer(&ids, p.distance, p.distance2.unwrap_or(p.distance), p.at) {
            Ok(()) => ok(changed(d, &existing(d, &ids), p.detail.verbose)),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Add offset copies of paths (empty ids = the selection), keeping the originals. Positive distance = outward for closed shapes (their holes shrink), to the left on screen for open ones. For laser kerf compensation of a part, offset by half the kerf and cut the copy.")]
    fn offset(&self, Parameters(p): Parameters<Offset>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.offset(&ids, p.distance) {
            Ok(new) => {
                let mut out = changed(d, &new, p.detail.verbose);
                out["bbox"] = json!(d.bbox(&new));
                ok(out)
            }
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Union, difference or intersection of two or more closed paths (empty ids = the selection). Difference subtracts the others from the first id. The result replaces the first item, keeping its name, layer and so its op (the other items' layers do not matter: a no-output cutter subtracted from a cut part leaves a cut part); holes become contours of a compound path; the other items are deleted. Returns the result compactly (verbose: true for its geometry).")]
    fn boolean(&self, Parameters(p): Parameters<BooleanItems>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.boolean(&ids, p.operation) {
            Ok(id) => ok(changed(d, &[id], p.detail.verbose)),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Copy items into a grid (empty ids = the selection): rows × columns in total, the original being the top-left cell, dx mm between columns and dy between rows.")]
    fn array_rectangular(&self, Parameters(p): Parameters<ArrayRect>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.array_rectangular(&ids, p.rows, p.columns, p.dx, p.dy) {
            Ok(new) => {
                ok(changed(d, &new, p.detail.verbose))
            }
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Copy items around a centre (empty ids = the selection): count items in total spread over angle degrees (default 360 = evenly round, positive = clockwise). rotate (default true) turns each copy; false keeps its orientation.")]
    fn array_polar(&self, Parameters(p): Parameters<ArrayPolar>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.array_polar(&ids, p.count, p.centre, p.angle.unwrap_or(360.0), p.rotate.unwrap_or(true)) {
            Ok(new) => {
                ok(changed(d, &new, p.detail.verbose))
            }
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Trim a path (AutoCAD TRIM): the piece around `at` is removed back to the nearest crossings with the cutting edges (default: every other path). An open line keeps the parts on either side; a closed shape needs two crossings and becomes open. An item with nothing left is deleted. Constraints on the item follow what is left (a trimmed circle's tangents move to the arc), and each new end stays on its cutting edge unless constrain is false.")]
    fn trim(&self, Parameters(p): Parameters<Trim>) -> ToolResult {
        let ids: Vec<u64> = p.id.into_iter().collect();
        self.edit_ids(&ids, |d, ids| match ids.as_slice() {
            [id] => match d.trim(*id, p.at, &p.edges, p.constrain.unwrap_or(true)) {
                Ok(()) => ok(changed(d, &[*id], p.detail.verbose)),
                Err(e) => fail(e),
            },
            _ => fail("give one id or select exactly one item"),
        })
    }

    #[tool(description = "Import an SVG file (path) or SVG text/base64 (data). Each path becomes an item (subpaths stay one compound path), curves become arcs, text becomes outlines and raster images greyscale images. Stroke colour sets the op: red = cut, blue = score, black/dark = engrave, grey or green = no_output, other colours = cut. A fill on a closed path means engrave the area, except a grey or green fill (no_output); a path with both a fill and a stroke becomes two items (the fill's op, then the stroke's) unless both give the same op. A path with neither fill nor stroke (or a fully transparent fill) comes in as no_output, handy as a boolean cutter. Each item goes on the current layer when that has its op, else on the first layer with it. Units in the file (mm, cm, in) are honoured; plain numbers are px at 96 per inch. Returns ids and items (compact, verbose: true for geometry), each with svg_id (the element's id, empty when it has none) and element (index of the SVG path/image in document order; one element can give two items).")]
    fn import_svg(&self, Parameters(p): Parameters<ImportSvg>) -> ToolResult {
        let bytes = match (&p.path, &p.data) {
            (Some(path), _) => std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}")),
            (None, Some(d)) if d.trim_start().starts_with('<') => Ok(d.clone().into_bytes()),
            (None, Some(d)) => base64::engine::general_purpose::STANDARD.decode(d.trim()).map_err(|e| format!("data is neither SVG text nor base64: {e}")),
            _ => Err("give path or data".into()),
        };
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => return fail(e),
        };
        let defaults = export::svg_import::ImportOptions::default();
        let opts = export::svg_import::ImportOptions {
            x: p.x,
            y: p.y,
            width: p.width,
            scale: p.scale.unwrap_or(defaults.scale),
            tolerance: p.tolerance.unwrap_or(defaults.tolerance),
            dir: p.path.as_deref().and_then(|f| PathBuf::from(f).parent().map(PathBuf::from)),
        };
        self.with(|d| {
            let before = d.s.clone();
            match d.import_svg_sources(&bytes, &opts) {
                Ok(new) => {
                    d.push_undo(before);
                    let ids: Vec<u64> = new.iter().map(|(id, _)| *id).collect();
                    let items: Vec<Value> = new
                        .iter()
                        .map(|(id, src)| {
                            let mut v = reply::item(d, d.item(*id).unwrap(), p.detail.verbose);
                            v["svg_id"] = json!(src.svg_id);
                            v["element"] = json!(src.element);
                            v
                        })
                        .collect();
                    ok(json!({"ids": ids, "bbox": d.bbox(&ids), "items": items}))
                }
                Err(e) => fail(e),
            }
        })
    }
}

/// The two picks of the two-object fillet/chamfer form: (id, point) each.
type Picks = [(u64, [f64; 2]); 2];

/// Both picks of the two-object fillet/chamfer form, None when neither is given.
fn two_objects(first: &Option<ObjectPick>, second: &Option<ObjectPick>) -> Option<Result<Picks, &'static str>> {
    match (first, second) {
        (None, None) => None,
        (Some(a), Some(b)) => Some(Ok([(a.id, a.at), (b.id, b.at)])),
        _ => Some(Err("give both first and second, or neither")),
    }
}

/// The ids that still exist (fillet and chamfer may merge two items into one).
fn existing(d: &Document, ids: &[u64]) -> Vec<u64> {
    ids.iter().copied().filter(|id| d.item(*id).is_some()).collect()
}

#[cfg(test)]
mod tests;

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("scriber", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}
