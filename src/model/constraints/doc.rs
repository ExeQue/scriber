//! The constraint commands on `Document`, and `settle`, which every change ends with.
//!
//! How edits stay constrained: `Document` keeps the last solved state. `settle` compares the
//! current state with it, drops constraints whose geometry is gone or changed shape (a deleted item,
//! a filleted corner), and runs the solver with the last solved state as the "before". If the
//! solver fails, the state goes back to the last solved one and the error says which constraints
//! conflict. The MCP server settles after every tool call, the GUI once per frame before painting,
//! so every command, grip drag and panel edit is covered without each one calling the solver.

use super::geo::{System, path_contours};
use super::residual::measure;
use super::{Constraint, Kind, Ref, expr, infer, solve};
use crate::model::{Document, State};
use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

/// A constraint as listed for agents and the GUI.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ConstraintInfo {
    #[serde(flatten)]
    pub constraint: Constraint,
    /// Dimensional: the parameter's current value (mm or degrees).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    pub satisfied: bool,
}

impl Document {
    pub fn constraint(&self, id: u64) -> Option<&Constraint> {
        self.s.constraints.iter().find(|c| c.id == id)
    }

    /// Ids of the constraints on any of `items`.
    pub fn constraints_on(&self, items: &[u64]) -> Vec<u64> {
        self.s.constraints.iter().filter(|c| c.refs.iter().any(|r| items.contains(&r.item()))).map(|c| c.id).collect()
    }

    /// Add a geometric constraint. The first reference stays put; the others move to satisfy it
    /// (as AutoCAD does) when the document settles.
    pub fn add_constraint(&mut self, kind: Kind, refs: Vec<Ref>) -> Result<u64, String> {
        if kind.is_dimension() {
            return Err(format!("{} is dimensional; add it with a value", kind.name()));
        }
        distinct(&refs)?;
        let id = self.s.next_id;
        let c = Constraint { id, kind, refs, name: String::new(), expr: String::new(), offset: 0.0 };
        System::default().add(&self.s, &c, 0.0)?;
        if self.s.constraints.iter().any(|o| o.kind == kind && same_refs(&o.refs, &c.refs)) {
            return Err(format!("that {} constraint is already there", kind.name()));
        }
        self.s.next_id += 1;
        // Everything but the second reference (for symmetric: the first object and the line).
        if c.refs.len() > 1 {
            self.hold.extend(c.refs.iter().enumerate().filter(|(i, _)| *i != 1).map(|(_, r)| *r));
        }
        self.s.constraints.push(c);
        self.touch();
        Ok(id)
    }

    /// Add a dimensional constraint named `name` (default d1, ang1, rad1, dia1, …) with `value`, a
    /// number or expression (default: what it measures now). `offset` places the dimension.
    pub fn add_dimension(&mut self, kind: Kind, refs: Vec<Ref>, name: Option<&str>, value: Option<&str>, offset: f64) -> Result<u64, String> {
        if !kind.is_dimension() {
            return Err(format!("{} is not dimensional", kind.name()));
        }
        distinct(&refs)?;
        let name = match name {
            Some(n) => n.trim().to_string(),
            None => (1..).map(|n| format!("{}{n}", kind.prefix())).find(|n| self.parameter(n).is_none()).unwrap(),
        };
        if !expr::is_name(&name) || self.parameter(&name).is_some() {
            return Err(format!("\"{name}\" is not a free parameter name (a letter, then letters, digits or _)"));
        }
        let id = self.s.next_id;
        let mut c = Constraint { id, kind, refs, name, expr: String::new(), offset };
        let mut sys = System::default();
        sys.add(&self.s, &c, 0.0)?;
        let now = measure(kind, &sys.cons[0].g, &sys.x).ok_or("cannot measure that")?;
        c.expr = value.map(|v| v.trim().to_string()).unwrap_or_else(|| expr::fmt_num(now));
        self.s.next_id += 1;
        self.hold.push(first_point(&c.refs));
        self.s.constraints.push(c);
        if let Err(e) = solve::parameter_values(&self.s) {
            self.s.constraints.pop();
            return Err(e);
        }
        self.touch();
        Ok(id)
    }

    /// The dimensional constraint that is parameter `name`.
    pub fn parameter(&self, name: &str) -> Option<&Constraint> {
        self.s.constraints.iter().find(|c| c.kind.is_dimension() && c.name == name)
    }

    /// Give parameter `name` a new value or expression; its geometry follows when the document settles.
    pub fn set_parameter(&mut self, name: &str, value: &str) -> Result<(), String> {
        let c = self.s.constraints.iter_mut().find(|c| c.kind.is_dimension() && c.name == name).ok_or_else(|| format!("there is no parameter \"{name}\""))?;
        let old = std::mem::replace(&mut c.expr, value.trim().to_string());
        let hold = first_point(&c.refs);
        if let Err(e) = solve::parameter_values(&self.s) {
            self.s.constraints.iter_mut().find(|c| c.name == name).unwrap().expr = old;
            return Err(e);
        }
        self.hold.push(hold);
        self.touch();
        Ok(())
    }

    /// Rename a parameter, and every expression that uses it.
    pub fn rename_parameter(&mut self, old: &str, new: &str) -> Result<(), String> {
        let new = new.trim();
        if old == new {
            return Ok(());
        }
        if !expr::is_name(new) || self.parameter(new).is_some() {
            return Err(format!("\"{new}\" is not a free parameter name"));
        }
        self.parameter(old).ok_or_else(|| format!("there is no parameter \"{old}\""))?;
        for c in self.s.constraints.iter_mut().filter(|c| c.kind.is_dimension()) {
            if c.name == old {
                c.name = new.to_string();
            }
            c.expr = expr::rename(&c.expr, old, new);
        }
        self.touch();
        Ok(())
    }

    /// Delete constraints by id. Parameters used in other expressions are refused.
    pub fn delete_constraints(&mut self, ids: &[u64]) -> Result<(), String> {
        let before = self.s.constraints.clone();
        self.s.constraints.retain(|c| !ids.contains(&c.id));
        if let Err(e) = solve::parameter_values(&self.s) {
            self.s.constraints = before;
            return Err(format!("cannot delete: {e}"));
        }
        self.touch();
        Ok(())
    }

    /// Every constraint with its parameter value and whether it holds now.
    pub fn list_constraints(&self) -> Vec<ConstraintInfo> {
        let values = solve::parameter_values(&self.s).unwrap_or_default();
        let ok = solve::satisfied(&self.s);
        self.s
            .constraints
            .iter()
            .map(|c| ConstraintInfo { constraint: c.clone(), value: values.get(&c.name).copied().filter(|_| c.kind.is_dimension()), satisfied: ok.get(&c.id).copied().unwrap_or(false) })
            .collect()
    }

    /// Infer constraints for `items` (AUTOCONSTRAIN) within `tol` mm and `angle_tol` degrees. Returns the new ids.
    pub fn auto_constrain(&mut self, items: &[u64], tol: f64, angle_tol: f64) -> Vec<u64> {
        let found = infer::infer(&self.s, items, tol, angle_tol);
        let ids: Vec<u64> = found.into_iter().filter_map(|(kind, refs)| self.add_constraint(kind, refs).ok()).collect();
        self.hold.clear();
        ids
    }

    /// Add constraints inferred for new geometry (INFER) and settle. When they cannot all hold
    /// together they are all dropped and the geometry stays as drawn. Returns how many were added.
    pub fn infer_new(&mut self, found: Vec<(Kind, Vec<Ref>)>) -> usize {
        self.constrain_new(Vec::new(), found)
    }

    /// Add the constraints new geometry was made with (`own`: typed sizes, a polygon's regularity;
    /// dimensional kinds become parameters at their current value) and the `inferred` ones, and
    /// settle. When they cannot all hold together the inferred ones are dropped, then all of them;
    /// the geometry stays as drawn. Returns how many were added.
    pub fn constrain_new(&mut self, own: Vec<(Kind, Vec<Ref>)>, inferred: Vec<(Kind, Vec<Ref>)>) -> usize {
        if (own.is_empty() && inferred.is_empty()) || self.settle(false).is_err() {
            return 0;
        }
        let before = self.s.clone();
        for set in [[own.clone(), inferred].concat(), own] {
            let n = set.into_iter().filter(|(kind, refs)| self.add_new(*kind, refs.clone()).is_ok()).count();
            if n == 0 || self.settle(true).is_ok() {
                return n;
            }
            self.s = before.clone();
            self.solved = None;
            self.hold.clear();
            self.touch();
        }
        0
    }

    fn add_new(&mut self, kind: Kind, refs: Vec<Ref>) -> Result<u64, String> {
        match kind {
            Kind::DimRadial | Kind::DimDiameter => self.add_dimension(kind, refs, None, None, 0.0),
            k if k.is_dimension() => self.add_dimension(kind, refs, None, None, 8.0),
            _ => self.add_constraint(kind, refs),
        }
    }

    /// Solve after a change (see the module doc). On failure the change is undone and the error returned;
    /// `explain` names the conflicting constraints (slower; off while dragging).
    pub fn settle(&mut self, explain: bool) -> Result<(), String> {
        // Nothing to keep: no copy needed (the next constraint starts from the drawing as it is then).
        if self.s.constraints.is_empty() {
            self.solved = None;
            self.hold.clear();
            return Ok(());
        }
        let fresh = self.solved.is_none();
        let prev = self.solved.take().unwrap_or_else(|| self.s.clone());
        if !fresh && self.s.items == prev.items && self.s.constraints == prev.constraints && self.hold.is_empty() {
            self.solved = Some(prev);
            return Ok(());
        }
        prune(&prev, &mut self.s);
        let hold = std::mem::take(&mut self.hold);
        let result = solve::solve(&prev, &mut self.s, &hold, explain);
        match result {
            Ok(moved) => {
                if moved {
                    self.touch();
                }
                self.solved = Some(self.s.clone());
                Ok(())
            }
            Err(e) => {
                let images = std::mem::take(&mut self.s.images);
                self.s = prev.clone();
                self.s.images.extend(images);
                // A refused command leaves no empty undo step behind.
                if self.undo.last().is_some_and(|u| u.items == prev.items && u.constraints == prev.constraints) {
                    self.undo.pop();
                }
                self.solved = Some(prev);
                self.touch();
                Err(e)
            }
        }
    }
}

/// What stays put when a dimension changes: its first point (a segment's start).
fn first_point(refs: &[Ref]) -> Ref {
    match refs[0] {
        Ref::Segment { item, contour, segment } if refs.len() == 1 => Ref::Point { item, contour, vertex: segment },
        r => r,
    }
}

/// A constraint between two (or three) things needs different ones: not the same point twice.
pub(crate) fn distinct(refs: &[Ref]) -> Result<(), String> {
    for (i, r) in refs.iter().enumerate() {
        if refs[..i].contains(r) {
            return Err(if matches!(r, Ref::Point { .. } | Ref::Centre { .. }) { "Pick two different points" } else { "Pick two different objects" }.into());
        }
    }
    Ok(())
}

fn same_refs(a: &[Ref], b: &[Ref]) -> bool {
    a == b || (a.len() == 2 && b.len() == 2 && a[0] == b[1] && a[1] == b[0])
}

/// Drop constraints whose geometry is gone, or whose contour gained or lost vertices since `prev`
/// (fillet, chamfer, trim, join): their vertex numbers no longer mean the same points.
fn prune(prev: &State, s: &mut State) {
    let shape = |st: &State, r: &Ref| -> Option<Vec<usize>> {
        let contours = path_contours(st, r.item()).ok()?;
        Some(contours.iter().map(|c| c.vertices.len() * 2 + usize::from(c.closed)).collect())
    };
    let snapshot = s.clone();
    s.constraints.retain(|c| {
        c.refs.iter().all(|r| {
            let now = shape(&snapshot, r);
            now.is_some() && (path_contours(prev, r.item()).is_err() || shape(prev, r) == now)
        }) && System::default().add(&snapshot, c, 0.0).is_ok()
    });
    freeze_orphans(s, &solve::parameter_values(prev).unwrap_or_default());
}

/// Expressions that use a parameter that is gone become its last value (from `values`).
pub(crate) fn freeze_orphans(s: &mut State, values: &std::collections::HashMap<String, f64>) {
    let names: Vec<String> = s.constraints.iter().filter(|c| c.kind.is_dimension()).map(|c| c.name.clone()).collect();
    for c in s.constraints.iter_mut().filter(|c| c.kind.is_dimension()) {
        let mut missing = false;
        let _ = expr::eval(&c.expr, &mut |n| {
            missing |= !names.iter().any(|m| m == n);
            Ok(0.0)
        });
        if missing && let Some(v) = values.get(&c.name) {
            c.expr = expr::fmt_num(*v);
        }
    }
}
