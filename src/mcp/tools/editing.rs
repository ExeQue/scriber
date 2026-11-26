//! MCP tools for the AutoCAD editing commands of the UX pack: stretch, join, explode, align,
//! match_properties, break_item, extend, lengthen, group/ungroup and select_similar/quick_select.

use super::reply::{Detail, IdsDetail, changed};
use super::{Server, ToolResult, fail, ok};
use crate::model::{BBox, Op, Point};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_router};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Stretch {
    /// Crossing window [x, y, width, height]: every point inside it moves.
    pub window: BBox,
    pub dx: f64,
    pub dy: f64,
    /// Only these items; empty = every item the window catches.
    #[serde(default)]
    pub ids: Vec<u64>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IdList {
    /// Item ids; empty = the user's current selection.
    #[serde(default)]
    pub ids: Vec<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Align {
    /// Items to move; empty = the selection.
    #[serde(default)]
    pub ids: Vec<u64>,
    pub source1: Point,
    pub dest1: Point,
    /// Second pair: the items also turn so source1→source2 lies along dest1→dest2.
    pub source2: Option<Point>,
    pub dest2: Option<Point>,
    /// With a second pair: also scale so the two lines have the same length.
    #[serde(default)]
    pub scale: bool,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MatchProps {
    pub source: u64,
    /// Items that get the source's laser operation (and font, text to text); empty = the selection.
    #[serde(default)]
    pub targets: Vec<u64>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BreakItem {
    pub id: u64,
    /// Break point (the nearest point on the path).
    pub at: Point,
    /// Second point: the piece between `at` and `at2` is removed (on a closed shape the shorter way round).
    pub at2: Option<Point>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Extend {
    pub id: u64,
    /// A point near the open end to extend.
    pub at: Point,
    /// Boundary items; empty = every other path and guide line.
    #[serde(default)]
    pub boundaries: Vec<u64>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Lengthen {
    pub id: u64,
    /// A point near the open end to change.
    pub at: Point,
    /// Change in length, mm (negative shortens).
    pub delta: Option<f64>,
    /// Or the new total length of the contour, mm.
    pub total: Option<f64>,
    #[serde(flatten)]
    pub detail: Detail,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuickSelect {
    pub op: Option<Op>,
    /// line, arc, circle, polyline, closed shape, compound path, text, dimension, image or guide.
    pub kind: Option<String>,
    /// Size range on the longer side of the bounding box, mm.
    pub min_size: Option<f64>,
    pub max_size: Option<f64>,
}

/// `Result<T, String>` as a tool result with `json` of the value.
macro_rules! reply {
    ($r:expr, $v:ident => $json:expr) => {
        match $r {
            Ok($v) => ok($json),
            Err(e) => fail(e),
        }
    };
}

#[tool_router(router = editing_router, vis = "pub(super)")]
impl Server {
    #[tool(description = "Stretch (AutoCAD STRETCH): every point of the items that lies inside the crossing window moves by dx/dy, the others stay, so the lines between them stretch. Whole circles, texts and images move when their centre/anchor is inside. One undo step. Returns the changed ids.")]
    fn stretch(&self, Parameters(p): Parameters<Stretch>) -> ToolResult {
        self.with(|d| {
            d.checkpoint();
            let r = d.stretch(&p.ids, p.window, p.dx, p.dy);
            if r.is_err() {
                d.undo();
            }
            reply!(r, ids => changed(d, &ids, p.detail.verbose))
        })
    }

    #[tool(description = "Join (AutoCAD JOIN) open paths that touch end to end into one path each (empty ids = the selection); a chain that comes round to its start becomes closed. Returns the remaining ids.")]
    fn join(&self, Parameters(p): Parameters<IdsDetail>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| reply!(d.join(&ids), ids => changed(d, &ids, p.detail.verbose)))
    }

    #[tool(description = "Explode (AutoCAD EXPLODE, empty ids = the selection): a compound path becomes one item per contour, a single path one item per line or arc segment, a text one path of its letter outlines. Returns the new ids.")]
    fn explode(&self, Parameters(p): Parameters<IdsDetail>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| reply!(d.explode(&ids), ids => changed(d, &ids, p.detail.verbose)))
    }

    #[tool(description = "Align (AutoCAD ALIGN, empty ids = the selection): move the items so source1 lands on dest1; with source2/dest2 also rotate them so source1→source2 points along dest1→dest2, and with scale also size them to match.")]
    fn align(&self, Parameters(p): Parameters<Align>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| reply!(d.align(&ids, (p.source1, p.source2), (p.dest1, p.dest2), p.scale), _x => {
            let mut out = changed(d, &ids, p.detail.verbose);
            out["bbox"] = json!(d.bbox(&ids));
            out
        }))
    }

    #[tool(description = "Match properties (AutoCAD MATCHPROP): give the targets (empty = the selection) the layer (and so the laser operation) of the source item, and its font when both are text.")]
    fn match_properties(&self, Parameters(p): Parameters<MatchProps>) -> ToolResult {
        self.edit_ids(&p.targets, |d, ids| reply!(d.match_properties(p.source, &ids), _x => changed(d, &ids, p.detail.verbose)))
    }

    #[tool(description = "Break a path (AutoCAD BREAK) at the point nearest `at`: an open path splits in two items, a closed one opens there. With at2 the piece between the two points is removed instead (a closed shape loses the shorter way round). Returns the ids of the pieces.")]
    fn break_item(&self, Parameters(p): Parameters<BreakItem>) -> ToolResult {
        self.edit_ids(&[p.id], |d, _| reply!(d.break_item(p.id, p.at, p.at2), ids => changed(d, &ids, p.detail.verbose)))
    }

    #[tool(description = "Extend (AutoCAD EXTEND) the open end of a path nearest `at` until it meets the nearest boundary ahead of it (lines along their direction, arcs round their circle). Boundaries default to every other path and guide line.")]
    fn extend(&self, Parameters(p): Parameters<Extend>) -> ToolResult {
        self.edit_ids(&[p.id], |d, _| reply!(d.extend(p.id, p.at, &p.boundaries), _x => changed(d, &[p.id], p.detail.verbose)))
    }

    #[tool(description = "Lengthen (AutoCAD LENGTHEN) the open end of a path nearest `at` by delta mm (negative shortens), or so the contour is total mm long.")]
    fn lengthen(&self, Parameters(p): Parameters<Lengthen>) -> ToolResult {
        self.edit_ids(&[p.id], |d, _| reply!(d.lengthen(p.id, p.at, p.delta, p.total), _x => changed(d, &[p.id], p.detail.verbose)))
    }

    #[tool(description = "Group items (empty ids = the selection) so they are picked together; an item leaves any group it was in.")]
    fn group(&self, Parameters(p): Parameters<IdList>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| reply!(d.group(&ids), _x => json!({"groups": d.s.groups})))
    }

    #[tool(description = "Dissolve the groups that hold any of the items (empty ids = the selection).")]
    fn ungroup(&self, Parameters(p): Parameters<IdList>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| reply!(d.ungroup(&ids), _x => json!({"groups": d.s.groups})))
    }

    #[tool(description = "Select similar: select every item on the same layer and with the same kind of shape as the given items (empty = the selection). Returns the new selection.")]
    fn select_similar(&self, Parameters(p): Parameters<IdList>) -> ToolResult {
        self.with(|d| {
            let ids = if p.ids.is_empty() { d.selection.clone() } else { p.ids.clone() };
            d.selection = d.similar(&ids);
            d.rev += 1;
            ok(json!({"selection": d.selection}))
        })
    }

    #[tool(description = "Quick select: select the items with the given laser operation (from their layer), kind of shape and size (longer side of the bounding box, mm); missing filters match anything. Returns the new selection.")]
    fn quick_select(&self, Parameters(p): Parameters<QuickSelect>) -> ToolResult {
        self.with(|d| {
            d.selection = d.quick_select(p.op, p.kind.as_deref(), p.min_size, p.max_size);
            d.rev += 1;
            ok(json!({"selection": d.selection}))
        })
    }
}
