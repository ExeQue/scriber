//! The MCP tools for constraints (AutoCAD's GEOMCONSTRAINT, DIMCONSTRAINT, PARAMETERS, DELCONSTRAINT
//! and AUTOCONSTRAIN).

use super::tools::{Server, ToolResult, fail, ok};
use crate::model::constraints::{expr, pick};
use crate::model::{Document, Kind, Ref};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddConstraint {
    /// A geometric constraint (not dim_*). Two-object constraints keep the first object and move the second, as in AutoCAD.
    #[serde(rename = "type")]
    pub kind: Kind,
    /// The geometry: e.g. [{"kind":"segment","item":3,"segment":0}, {"kind":"point","item":4,"vertex":1}].
    /// Symmetric takes the two objects, then the symmetry line.
    pub refs: Vec<Ref>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DimType {
    /// Horizontal or vertical, whichever the points are further apart in.
    Linear,
    Horizontal,
    Vertical,
    /// Straight distance (two points, a line's length, or a point to a line).
    Aligned,
    /// Between two lines, in degrees.
    Angular,
    Radius,
    Diameter,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddDimension {
    #[serde(rename = "type")]
    pub kind: DimType,
    /// Two points, or one line (linear, aligned); two lines (angular); an arc or circle segment (radius, diameter).
    pub refs: Vec<Ref>,
    /// Parameter name; default d1, ang1, rad1, dia1, ….
    pub name: Option<String>,
    /// A number (mm or degrees) or an expression of other parameters such as "d1 / 2". Default: what it measures now.
    pub value: Option<Value>,
    /// Where the dimension is drawn: mm from the geometry (linear, aligned), the arc radius (angular), or the leader angle in degrees (radius, diameter). Default 8 / 0.
    pub offset: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetParameter {
    pub name: String,
    /// A number or an expression of other parameters ("d1 * 2 + 5").
    pub value: Value,
    /// Rename the parameter (expressions that use it follow).
    pub rename: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteConstraints {
    /// Constraint ids.
    #[serde(default)]
    pub ids: Vec<u64>,
    /// Also every constraint on these items (AutoCAD DELCONSTRAINT).
    #[serde(default)]
    pub items: Vec<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AutoConstrain {
    /// Item ids; empty = the selection.
    #[serde(default)]
    pub ids: Vec<u64>,
    /// Distance tolerance in mm (default 0.05).
    pub tolerance: Option<f64>,
    /// Angle tolerance in degrees (default 1).
    pub angle_tolerance: Option<f64>,
}

/// A number or string as expression text.
fn value_text(v: &Value) -> Result<String, String> {
    match v {
        Value::Number(n) => Ok(expr::fmt_num(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => Ok(s.clone()),
        _ => Err("value must be a number or an expression string".into()),
    }
}

/// The dimensional kind; linear picks horizontal or vertical from the geometry.
fn dim_kind(d: &Document, t: &DimType, refs: &[Ref]) -> Kind {
    match t {
        DimType::Linear => {
            let pts: Vec<[f64; 2]> = refs.iter().flat_map(|r| pick::outline(&d.s, r)).collect();
            match (pts.first(), pts.last()) {
                (Some(a), Some(b)) if (b[1] - a[1]).abs() > (b[0] - a[0]).abs() => Kind::DimVertical,
                _ => Kind::DimHorizontal,
            }
        }
        DimType::Horizontal => Kind::DimHorizontal,
        DimType::Vertical => Kind::DimVertical,
        DimType::Aligned => Kind::DimAligned,
        DimType::Angular => Kind::DimAngular,
        DimType::Radius => Kind::DimRadial,
        DimType::Diameter => Kind::DimDiameter,
    }
}

/// Run `f` as one undo step and settle; the result shows the constraint `f` returns (if any).
fn change(d: &mut Document, f: impl FnOnce(&mut Document) -> Result<Option<u64>, String>) -> ToolResult {
    let before = d.s.clone();
    let id = match f(d) {
        Ok(id) => id,
        Err(e) => {
            d.s = before;
            return fail(e);
        }
    };
    d.push_undo(before);
    match d.settle(true) {
        Ok(()) => ok(id.map_or(json!({"ok": true}), |id| info(d, id))),
        Err(e) => fail(format!("refused: {e}; the change was undone")),
    }
}

fn info(d: &Document, id: u64) -> Value {
    json!(d.list_constraints().into_iter().find(|c| c.constraint.id == id))
}

#[tool_router(router = constraint_router, vis = "pub(super)")]
impl Server {
    #[tool(description = "Add a geometric constraint (AutoCAD GEOMCONSTRAINT): coincident, collinear, concentric, fix, parallel, perpendicular, horizontal, vertical, tangent, smooth, symmetric or equal. The geometry moves to satisfy it at once and stays bound from then on. Refused (with the conflicting constraints named) if it cannot be satisfied.")]
    fn add_constraint(&self, Parameters(p): Parameters<AddConstraint>) -> ToolResult {
        self.with(|d| {
            change(d, |d| {
                d.add_constraint(p.kind, p.refs).map(Some)
            })
        })
    }

    #[tool(description = "Add a dimensional constraint (AutoCAD DIMCONSTRAINT): a named parameter (d1, ang1, rad1, dia1) that drives the geometry to its value. The value can be a number or an expression of other parameters. The dimension shows on the drawing as name=value.")]
    fn add_dimensional_constraint(&self, Parameters(p): Parameters<AddDimension>) -> ToolResult {
        let value = match p.value.as_ref().map(value_text).transpose() {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        self.with(|d| {
            change(d, |d| {
                let kind = dim_kind(d, &p.kind, &p.refs);
                let offset = p.offset.unwrap_or(if matches!(kind, Kind::DimRadial | Kind::DimDiameter) { 0.0 } else { 8.0 });
                d.add_dimension(kind, p.refs, p.name.as_deref(), value.as_deref(), offset).map(Some)
            })
        })
    }

    #[tool(description = "Set a parameter (a dimensional constraint) to a number or an expression such as \"d1 / 2\"; the geometry follows. Optionally rename it.")]
    fn set_parameter(&self, Parameters(p): Parameters<SetParameter>) -> ToolResult {
        let value = match value_text(&p.value) {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        self.with(|d| {
            change(d, |d| {
                d.set_parameter(&p.name, &value)?;
                let mut name = p.name.clone();
                if let Some(new) = &p.rename {
                    d.rename_parameter(&p.name, new)?;
                    name = new.clone();
                }
                Ok(d.parameter(&name).map(|c| c.id))
            })
        })
    }

    #[tool(description = "List every constraint: type, references, the parameter name/expression/value of dimensional ones, and whether it is satisfied.")]
    fn list_constraints(&self) -> ToolResult {
        let d = self.doc.lock().unwrap();
        ok(json!({"constraints": d.list_constraints()}))
    }

    #[tool(description = "Delete constraints by id, and/or every constraint on the given items (AutoCAD DELCONSTRAINT). The geometry stays where it is.")]
    fn delete_constraints(&self, Parameters(p): Parameters<DeleteConstraints>) -> ToolResult {
        self.with(|d| {
            let mut ids = p.ids.clone();
            ids.extend(d.constraints_on(&p.items));
            if ids.is_empty() {
                return fail("nothing to delete: give constraint ids or item ids");
            }
            change(d, |d| {
                d.delete_constraints(&ids).map(|()| None)
            })
        })
    }

    #[tool(description = "Infer constraints (AutoCAD AUTOCONSTRAIN) for items (empty = the selection): coincident ends, horizontal, vertical, parallel, perpendicular, tangent, concentric and equal, within the tolerances. Returns the new constraints.")]
    fn auto_constrain(&self, Parameters(p): Parameters<AutoConstrain>) -> ToolResult {
        self.edit_ids(&p.ids, |d, ids| {
            let new = d.auto_constrain(&ids, p.tolerance.unwrap_or(0.05), p.angle_tolerance.unwrap_or(1.0));
            if let Err(e) = d.settle(true) {
                return fail(e);
            }
            let all = d.list_constraints();
            ok(json!({"added": all.iter().filter(|c| new.contains(&c.constraint.id)).collect::<Vec<_>>()}))
        })
    }
}
