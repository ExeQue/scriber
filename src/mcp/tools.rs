//! The MCP tools: one per document command, mirroring what the GUI can do.

use crate::export;
use crate::model::{BoolOp, Contour, Document, Op, Shape, Shared, Transform};
use base64::Engine;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::schemars::{self, JsonSchema};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

const INSTRUCTIONS: &str = "Scriber is a 2D CAD program for laser cutters. You edit the document that is open in the user's Scriber window; every change shows there live and the user can undo it.
Units are millimetres. Origin is the top-left corner of the material, x grows right, y grows DOWN (like SVG). Angles are degrees, positive = clockwise on screen.
Each item has an op for its lines: cut (red, cut through), score (blue, mark a line), engrave (black line), none (lines ignored) or annotate (notes and dimensions for people, never lasered). Separately, fill: true engraves the enclosed area (closed paths and text). Example: a coaster with an engraved face is op cut + fill true.
A path holds one or more contours; several contours make a compound path whose holes stay empty when filled (even-odd). Use combine_items/break_apart to build or split them.
CAD operations: fillet, chamfer, offset (also kerf compensation), boolean (union/difference/intersection), array_rectangular, array_polar and trim.
Contours are polylines whose segments can be arcs: each vertex has a bulge = tan(sweep/4) for the segment to the next vertex (0 = straight). Prefer add_items with kind rect/circle/arc/polygon/line/polyline over building arcs by hand.
Start with get_document; it includes the user's current selection, and tools that take ids use the selection when ids is empty. Call export_png without a path to look at the result.";

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
    /// Text; (x, y) is the start of the baseline; size is the font size in mm.
    Text { x: f64, y: f64, size: f64, text: String, #[serde(default)] rotation: f64 },
    /// A measurement from point 1 to point 2, drawn offset mm to the left of that direction.
    Dimension { x1: f64, y1: f64, x2: f64, y2: f64, #[serde(default = "default_dim_offset")] offset: f64 },
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
            S::Text { x, y, size, text, rotation } => ("text", Shape::Text { x, y, size, text, rotation }),
            S::Dimension { x1, y1, x2, y2, offset } => ("dimension", Shape::Dimension { x1, y1, x2, y2, offset }),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NewItem {
    pub shape: ShapeInput,
    /// Line operation. Default cut; annotate for dimensions; none for text.
    pub op: Option<Op>,
    /// Engrave the enclosed area. Default false; true for text.
    pub fill: Option<bool>,
    pub name: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddItems {
    pub items: Vec<NewItem>,
    /// Select the new items in the GUI (default true).
    pub select: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateItem {
    pub id: u64,
    pub name: Option<String>,
    pub op: Option<Op>,
    pub fill: Option<bool>,
    pub note: Option<String>,
    /// Replace the geometry.
    pub shape: Option<ShapeInput>,
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
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransformItems {
    #[serde(default)]
    pub ids: Vec<u64>,
    #[serde(flatten)]
    pub transform: Transform,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Duplicate {
    #[serde(default)]
    pub ids: Vec<u64>,
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
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
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportSvg {
    pub path: String,
    /// Include annotate items. Default false, because laser software would burn them.
    #[serde(default)]
    pub include_annotations: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportPng {
    /// Where to write the PNG. Without a path the image is returned so you can look at it (max 1600 px wide).
    pub path: Option<String>,
    /// Resolution; default 254 (10 px per mm).
    pub dpi: Option<f64>,
    /// Default true.
    pub include_annotations: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SavePath {
    /// Path to the .scriber file; missing = the current file.
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
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Offset {
    #[serde(default)]
    pub ids: Vec<u64>,
    /// mm. Positive = outward for closed shapes (holes shrink), to the left on screen for open ones; negative = inward / right.
    pub distance: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BooleanItems {
    /// Two or more closed paths; empty = the selection. For difference the first one is the base.
    #[serde(default)]
    pub ids: Vec<u64>,
    pub operation: BoolOp,
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
}

// ---------- server ----------

#[derive(Clone)]
pub struct Server {
    doc: Shared,
    repaint: Arc<dyn Fn() + Send + Sync>,
    tool_router: ToolRouter<Self>,
}

type ToolResult = Result<CallToolResult, McpError>;

fn ok(v: Value) -> ToolResult {
    Ok(CallToolResult::success(vec![ContentBlock::text(v.to_string())]))
}

fn fail(msg: impl std::fmt::Display) -> ToolResult {
    Ok(CallToolResult::error(vec![ContentBlock::text(msg.to_string())]))
}

fn summary(d: &Document) -> Value {
    let items: Vec<Value> = d
        .s
        .items
        .iter()
        .map(|i| {
            let mut v = serde_json::to_value(i).unwrap();
            v["bbox"] = json!(i.shape.metrics().bbox);
            v
        })
        .collect();
    let images: Vec<Value> = d.s.images.iter().map(|(id, i)| json!({"image": id, "px_width": i.w, "px_height": i.h})).collect();
    json!({
        "width": d.s.width, "height": d.s.height, "units": "mm",
        "file": d.path, "unsaved_changes": d.dirty,
        "notes": d.s.notes, "selection": d.selection,
        "items": items, "images": images,
    })
}

impl Server {
    pub fn new(doc: Shared, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { doc, repaint, tool_router: Self::tool_router() }
    }

    /// Run `f` on the locked document, then repaint the GUI.
    fn with(&self, f: impl FnOnce(&mut Document) -> ToolResult) -> ToolResult {
        let r = f(&mut self.doc.lock().unwrap());
        (self.repaint)();
        r
    }

    /// Run a two-object fillet or chamfer.
    fn edit_objects(&self, picks: Result<Picks, &str>, f: impl FnOnce(&mut Document, (u64, [f64; 2]), (u64, [f64; 2])) -> Result<Vec<u64>, String>) -> ToolResult {
        let [a, b] = match picks {
            Ok(p) => p,
            Err(e) => return fail(e),
        };
        self.edit_ids(&[a.0, b.0], |d, _| match f(d, a, b) {
            Ok(ids) => ok(json!({"ids": ids})),
            Err(e) => fail(e),
        })
    }

    /// Like `with`, for commands that act on ids (empty = selection) and change the document.
    fn edit_ids(&self, ids: &[u64], f: impl FnOnce(&mut Document, Vec<u64>) -> ToolResult) -> ToolResult {
        self.with(|d| {
            let ids = if ids.is_empty() { d.selection.clone() } else { ids.to_vec() };
            if ids.is_empty() {
                return fail("no ids given and nothing is selected");
            }
            match d.resolve(&ids) {
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
    #[tool(description = "Get the whole document: material size, notes, the user's selection, and every item with its geometry and bounding box.")]
    fn get_document(&self) -> ToolResult {
        ok(summary(&self.doc.lock().unwrap()))
    }

    #[tool(description = "Select items in the GUI (empty list clears the selection).")]
    fn select(&self, Parameters(p): Parameters<Ids>) -> ToolResult {
        self.with(|d| match d.resolve(&p.ids) {
            Ok(ids) => {
                d.selection = ids;
                d.rev += 1;
                ok(json!({"selection": d.selection}))
            }
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Add items (line, rect, circle, arc, polygon, polyline, path, text, dimension). Returns their ids. One undo step.")]
    fn add_items(&self, Parameters(p): Parameters<AddItems>) -> ToolResult {
        self.with(|d| {
            d.checkpoint();
            let mut ids = Vec::new();
            for it in p.items {
                let (kind, shape) = it.shape.build();
                let (default_op, default_fill) = match shape {
                    Shape::Dimension { .. } => (Op::Annotate, false),
                    Shape::Text { .. } => (Op::NoLine, true),
                    _ => (Op::Cut, false),
                };
                let id = d.add(it.name.as_deref().unwrap_or(kind), it.op.unwrap_or(default_op), it.fill.unwrap_or(default_fill), shape);
                d.item_mut(id).unwrap().note = it.note.unwrap_or_default();
                ids.push(id);
            }
            if p.select.unwrap_or(true) {
                d.selection = ids.clone();
            }
            ok(json!({"ids": ids}))
        })
    }

    #[tool(description = "Change an item's name, op, fill, note or geometry.")]
    fn update_item(&self, Parameters(p): Parameters<UpdateItem>) -> ToolResult {
        self.edit_ids(&[p.id], |d, _| {
            let item = d.item_mut(p.id).unwrap();
            if let Some(n) = p.name {
                item.name = n;
            }
            if let Some(op) = p.op {
                item.op = op;
            }
            if let Some(f) = p.fill {
                item.fill = f;
            }
            if let Some(n) = p.note {
                item.note = n;
            }
            if let Some(s) = p.shape {
                item.shape = s.build().1;
            }
            ok(serde_json::to_value(&*item).unwrap())
        })
    }

    #[tool(description = "Delete items (empty ids = the selection).")]
    fn delete_items(&self, Parameters(p): Parameters<Ids>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| {
            d.delete(&ids);
            ok(json!({"deleted": ids}))
        })
    }

    #[tool(description = "Move, rotate, scale and/or mirror items (empty ids = the selection). Order: mirror, scale, rotate around the pivot, then move by dx/dy. The pivot defaults to the centre of the items' bounding box.")]
    fn transform_items(&self, Parameters(p): Parameters<TransformItems>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| {
            d.transform(&ids, &p.transform);
            ok(json!({"ids": ids, "bbox": d.bbox(&ids)}))
        })
    }

    #[tool(description = "Copy items, offset by dx/dy (empty ids = the selection). The copies become the selection.")]
    fn duplicate_items(&self, Parameters(p): Parameters<Duplicate>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| {
            d.selection = d.duplicate(&ids, p.dx, p.dy);
            ok(json!({"ids": d.selection}))
        })
    }

    #[tool(description = "Combine paths into one compound path (empty ids = the selection). Holes stay empty when filled. Keeps the first item's name, op and fill.")]
    fn combine_items(&self, Parameters(p): Parameters<Ids>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.combine(&ids) {
            Ok(id) => ok(json!({"id": id})),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Split a compound path into one item per contour.")]
    fn break_apart(&self, Parameters(p): Parameters<OneId>) -> ToolResult {
        let ids: Vec<u64> = p.id.into_iter().collect();
        self.edit_ids(&ids, |d, ids| match ids.as_slice() {
            [id] => match d.break_apart(*id) {
                Ok(new) => ok(json!({"ids": new})),
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
                if i.op.burns_lines() && matches!(i.shape, Shape::Path { .. }) {
                    let key = format!("{}_length", serde_json::to_value(i.op).unwrap().as_str().unwrap());
                    let t = totals.get(&key).and_then(Value::as_f64).unwrap_or(0.0);
                    totals.insert(key, json!(t + m.length));
                }
                if i.engraves_area() {
                    let t = totals.get("fill_area").and_then(Value::as_f64).unwrap_or(0.0);
                    totals.insert("fill_area".into(), json!(t + m.area.unwrap_or(0.0)));
                }
                json!({"id": i.id, "name": i.name, "op": i.op, "fill": i.fill, "metrics": m})
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
            d.selection = vec![id];
            ok(serde_json::to_value(d.item(id).unwrap()).unwrap())
        })
    }

    #[tool(description = "Export the document as SVG in mm for laser software (red = cut, blue = score, black = engrave lines and filled areas).")]
    fn export_svg(&self, Parameters(p): Parameters<ExportSvg>) -> ToolResult {
        let svg = export::svg(&self.doc.lock().unwrap().s, p.include_annotations);
        match std::fs::write(&p.path, svg) {
            Ok(()) => ok(json!({"path": p.path})),
            Err(e) => fail(format!("cannot write {}: {e}", p.path)),
        }
    }

    #[tool(description = "Render the document to PNG. With a path it is written to that file; without, the image is returned so you can see the drawing.")]
    fn export_png(&self, Parameters(p): Parameters<ExportPng>) -> ToolResult {
        let state = self.doc.lock().unwrap().s.clone();
        let mut dpi = p.dpi.unwrap_or(254.0);
        if p.path.is_none() {
            dpi = dpi.min(1600.0 / (state.width / 25.4));
        }
        let png = match export::png(&state, dpi, p.include_annotations.unwrap_or(true)) {
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

    #[tool(description = "Save the document as a .scriber file (without path: the current file).")]
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

    #[tool(description = "Open a .scriber file, replacing the current document. Fails on unsaved changes unless force is set.")]
    fn open(&self, Parameters(p): Parameters<OpenFile>) -> ToolResult {
        self.with(|d| {
            if d.dirty && !p.force {
                return fail("the current document has unsaved changes; save it first or pass force: true");
            }
            match Document::open(&PathBuf::from(&p.path)) {
                Ok(new) => {
                    *d = new;
                    d.rev += 1;
                    ok(summary(d))
                }
                Err(e) => fail(e),
            }
        })
    }

    #[tool(description = "Start a new empty document (material size in mm, default 300 × 200). Fails on unsaved changes unless force is set.")]
    fn new_document(&self, Parameters(p): Parameters<NewDoc>) -> ToolResult {
        self.with(|d| {
            if d.dirty && !p.force {
                return fail("the current document has unsaved changes; save it first or pass force: true");
            }
            *d = Document::new(p.width.unwrap_or(300.0), p.height.unwrap_or(200.0));
            d.rev += 1;
            ok(summary(d))
        })
    }

    #[tool(description = "Round corners with an arc (empty ids = the selection): the corner nearest `at`, or every sharp corner between straight segments. With exactly two open lines/polylines, they are extended or shortened to meet, joined into one path, and that corner is rounded (AutoCAD FILLET of two objects). With `first` and `second` (each an id and the point it was picked at), fillets any two objects as AutoCAD does: lines, arcs, circles and end segments of open polylines (lines touching end to end count as one polyline); the picks choose the kept sides and which arc is meant, lines and arcs are trimmed or extended to the arc, circles stay, two open paths become one, parallel lines get a half-circle cap, radius 0 makes a sharp corner. Returns the new and changed ids.")]
    fn fillet(&self, Parameters(p): Parameters<Fillet>) -> ToolResult {
        if let Some(picks) = two_objects(&p.first, &p.second) {
            return self.edit_objects(picks, |d, a, b| d.fillet_objects(a, b, p.radius));
        }
        self.edit_ids(&p.ids, |d, ids| match d.fillet(&ids, p.radius, p.at) {
            Ok(()) => ok(json!({"ids": existing(d, &ids)})),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Cut corners off with a straight line (empty ids = the selection): the corner nearest `at`, or every sharp corner. distance goes back along the incoming side, distance2 (default = distance) along the outgoing one. Two open lines/polylines are joined where they meet, like fillet. With `first` and `second`, chamfers two picked objects as AutoCAD does (lines, arcs and end segments of open polylines; not circles): distance from the corner along the first (along the arc for arcs), distance2 along the second; 0 and 0 makes a sharp corner. Returns the new and changed ids.")]
    fn chamfer(&self, Parameters(p): Parameters<Chamfer>) -> ToolResult {
        if let Some(picks) = two_objects(&p.first, &p.second) {
            return self.edit_objects(picks, |d, a, b| d.chamfer_objects(a, b, p.distance, p.distance2.unwrap_or(p.distance)));
        }
        self.edit_ids(&p.ids, |d, ids| match d.chamfer(&ids, p.distance, p.distance2.unwrap_or(p.distance), p.at) {
            Ok(()) => ok(json!({"ids": existing(d, &ids)})),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Add offset copies of paths (empty ids = the selection), keeping the originals. Positive distance = outward for closed shapes (their holes shrink), to the left on screen for open ones. For laser kerf compensation of a part, offset by half the kerf and cut the copy. The copies become the selection.")]
    fn offset(&self, Parameters(p): Parameters<Offset>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.offset(&ids, p.distance) {
            Ok(new) => {
                d.selection = new.clone();
                ok(json!({"ids": new, "bbox": d.bbox(&new)}))
            }
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Union, difference or intersection of two or more closed paths (empty ids = the selection). Difference subtracts the others from the first id. The result replaces the first item (keeping its name, op and fill); holes become contours of a compound path; the other items are deleted.")]
    fn boolean(&self, Parameters(p): Parameters<BooleanItems>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.boolean(&ids, p.operation) {
            Ok(id) => ok(serde_json::to_value(d.item(id).unwrap()).unwrap()),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Copy items into a grid (empty ids = the selection): rows × columns in total, the original being the top-left cell, dx mm between columns and dy between rows. The copies become the selection.")]
    fn array_rectangular(&self, Parameters(p): Parameters<ArrayRect>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.array_rectangular(&ids, p.rows, p.columns, p.dx, p.dy) {
            Ok(new) => {
                d.selection = new.clone();
                ok(json!({"ids": new}))
            }
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Copy items around a centre (empty ids = the selection): count items in total spread over angle degrees (default 360 = evenly round, positive = clockwise). rotate (default true) turns each copy; false keeps its orientation. The copies become the selection.")]
    fn array_polar(&self, Parameters(p): Parameters<ArrayPolar>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| match d.array_polar(&ids, p.count, p.centre, p.angle.unwrap_or(360.0), p.rotate.unwrap_or(true)) {
            Ok(new) => {
                d.selection = new.clone();
                ok(json!({"ids": new}))
            }
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Trim a path (AutoCAD TRIM): the piece around `at` is removed back to the nearest crossings with the cutting edges (default: every other path). An open line keeps the parts on either side; a closed shape needs two crossings and becomes open. An item with nothing left is deleted.")]
    fn trim(&self, Parameters(p): Parameters<Trim>) -> ToolResult {
        let ids: Vec<u64> = p.id.into_iter().collect();
        self.edit_ids(&ids, |d, ids| match ids.as_slice() {
            [id] => match d.trim(*id, p.at, &p.edges) {
                Ok(()) => ok(json!({"id": id, "item": d.item(*id)})),
                Err(e) => fail(e),
            },
            _ => fail("give one id or select exactly one item"),
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

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("scriber", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}
