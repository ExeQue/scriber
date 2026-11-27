//! The solver: damped Gauss–Newton (Levenberg–Marquardt style) on the residuals, with minimal change.
//!
//! The solve starts from the edited state. Each step is the smallest weighted change that removes the
//! residuals to first order: dx = −W⁻¹Jᵀ (J W⁻¹ Jᵀ + λ·diag)⁻¹ r. Unknowns the edit changed (the
//! dragged vertex, the moved item) and the held references (the first object of a new constraint)
//! are locked, so the rest of the drawing gives way and the edit stays where the user put it; when
//! that has no solution they only weigh `HEAVY`, so they give way as little as possible.
//! The Jacobian is numeric (central differences per constraint) and the dense system is solved
//! with Cholesky. Independent groups of constraints are solved separately, and only when broken.

use super::geo::{Compiled, System, Var};
use super::residual::residuals;
use super::{Constraint, Ref, expr};
use crate::model::{Contour, State};
use std::collections::HashMap;

/// A residual below this (mm, or sine of an angle) counts as satisfied.
pub const TOL: f64 = 1e-8;
/// Weight of unknowns the edit changed or that are held, against 1 for the rest.
const HEAVY: f64 = 1e4;
const MAX_STEPS: usize = 200;

/// The values of every dimensional parameter.
pub fn parameter_values(s: &State) -> Result<HashMap<String, f64>, String> {
    let params: Vec<(&str, &str)> = s.constraints.iter().filter(|c| c.kind.is_dimension()).map(|c| (c.name.as_str(), c.expr.as_str())).collect();
    expr::evaluate_all(&params)
}

/// Compile every constraint of `s`.
pub fn compile(s: &State) -> Result<System, String> {
    let values = parameter_values(s)?;
    let mut sys = System::default();
    for c in &s.constraints {
        let value = values.get(&c.name).copied().unwrap_or(0.0);
        sys.add(s, c, value).map_err(|e| format!("{}: {e}", c.label()))?;
    }
    Ok(sys)
}

/// The value an unknown had in `prev`, when its contour still has the same shape there.
fn prev_value(prev: &State, var: Var, nth: usize) -> Option<f64> {
    let (Var::Vertex((item, ci, k)) | Var::Bulge((item, ci, k))) = var;
    let c: &Contour = super::geo::path_contours(prev, item).ok()?.get(ci)?;
    let v = c.vertices.get(k)?;
    Some(match var {
        Var::Bulge(_) => v.bulge,
        _ if nth == 0 => v.x,
        _ => v.y,
    })
}

/// Make `s` satisfy its constraints, changing it as little as possible from the edit (see the
/// module doc). `prev` is the last solved state; `hold` are references that should not move.
/// On failure `s` is unchanged and the error names the conflicting constraints (when `explain`).
/// Returns whether anything moved.
pub fn solve(prev: &State, s: &mut State, hold: &[Ref], explain: bool) -> Result<bool, String> {
    if s.constraints.is_empty() {
        return Ok(false);
    }
    let mut sys = compile(s)?;
    let mut weight = vec![1.0; sys.x.len()];
    for (i, var) in sys.vars.iter().enumerate() {
        let nth = usize::from(i > 0 && sys.vars[i - 1] == *var);
        // New geometry has nothing to keep; changed geometry is the edit.
        if prev_value(prev, *var, nth).is_some_and(|p| (p - sys.x[i]).abs() > 1e-12) {
            weight[i] = HEAVY;
        }
    }
    for r in hold {
        if let Ok(g) = sys.resolve(s, r) {
            weight.resize(sys.x.len(), 1.0);
            for i in g.vars() {
                weight[i] = HEAVY;
            }
        }
    }
    // Fix keeps the value from before the edit.
    for c in sys.cons.iter_mut().filter(|c| c.kind == super::Kind::Fix) {
        for (k, i) in c.vars.iter().enumerate() {
            let nth = usize::from(*i > 0 && sys.vars[*i - 1] == sys.vars[*i]);
            if let Some(p) = prev_value(prev, sys.vars[*i], nth) {
                c.target[k] = p;
            }
        }
    }
    let mut x = sys.x.clone();
    for group in groups(&sys.cons, x.len()) {
        let cons: Vec<Compiled> = group.iter().map(|i| sys.cons[*i].clone()).collect();
        // First keep the edit exactly where it is; if that cannot work, let it give way a little.
        let locked: Vec<f64> = weight.iter().map(|w| if *w == HEAVY { f64::INFINITY } else { *w }).collect();
        let mut first = x.clone();
        if newton(&cons, &mut first, &locked) {
            x = first;
        } else if !newton(&cons, &mut x, &weight) {
            return Err(if explain { conflict_message(s, &cons, &sys.x, &weight) } else { "the constraints cannot be satisfied".into() });
        }
    }
    let moved = x.iter().zip(&sys.x).any(|(a, b)| a != b);
    write_back(s, &sys.vars, &x);
    Ok(moved)
}

/// Copy the solved unknowns into the state.
fn write_back(s: &mut State, vars: &[Var], x: &[f64]) {
    for (i, var) in vars.iter().enumerate() {
        let (Var::Vertex((item, ci, k)) | Var::Bulge((item, ci, k))) = *var;
        let Some(crate::model::Shape::Path { contours }) = s.items.iter_mut().find(|it| it.id == item).map(|it| &mut it.shape) else { continue };
        let v = &mut contours[ci].vertices[k];
        match var {
            Var::Bulge(_) => v.bulge = x[i],
            _ if i > 0 && vars[i - 1] == *var => v.y = x[i],
            _ => v.x = x[i],
        }
    }
}

/// Constraints that share unknowns, grouped (indices into `cons`).
fn groups(cons: &[Compiled], n: usize) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for c in cons {
        for w in c.vars.windows(2) {
            let (a, b) = (root(&mut parent, w[0]), root(&mut parent, w[1]));
            parent[a] = b;
        }
    }
    let mut by_root: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, c) in cons.iter().enumerate() {
        let r = c.vars.first().map_or(usize::MAX - k, |v| root(&mut parent, *v));
        by_root.entry(r).or_default().push(k);
    }
    let mut out: Vec<Vec<usize>> = by_root.into_values().collect();
    out.sort();
    out
}

fn all_residuals(cons: &[Compiled], x: &[f64]) -> Vec<f64> {
    let mut r = Vec::new();
    for c in cons {
        residuals(c, x, &mut r);
    }
    r
}

fn norm2(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}

/// Sparse Jacobian rows: (unknown, derivative) per residual, by central differences.
fn jacobian(cons: &[Compiled], x: &[f64]) -> Vec<Vec<(usize, f64)>> {
    let mut rows = Vec::new();
    let mut xp = x.to_vec();
    let (mut plus, mut minus) = (Vec::new(), Vec::new());
    for c in cons {
        let start = rows.len();
        let mut base = Vec::new();
        residuals(c, x, &mut base);
        rows.extend(base.iter().map(|_| Vec::new()));
        for &k in &c.vars {
            let h = 1e-6 * (1.0 + x[k].abs());
            plus.clear();
            minus.clear();
            xp[k] = x[k] + h;
            residuals(c, &xp, &mut plus);
            xp[k] = x[k] - h;
            residuals(c, &xp, &mut minus);
            xp[k] = x[k];
            for (i, (p, m)) in plus.iter().zip(&minus).enumerate() {
                let d = (p - m) / (2.0 * h);
                if d != 0.0 {
                    rows[start + i].push((k, d));
                }
            }
        }
    }
    rows
}

/// Solve the group in place. False when it cannot be satisfied.
fn newton(cons: &[Compiled], x: &mut [f64], w: &[f64]) -> bool {
    let mut r = all_residuals(cons, x);
    let mut lambda = 1e-9;
    for _ in 0..MAX_STEPS {
        if r.iter().all(|v| v.abs() < TOL) {
            return true;
        }
        let rows = jacobian(cons, x);
        let m = r.len();
        let mut a = vec![0.0; m * m];
        for i in 0..m {
            for j in 0..=i {
                let mut v = 0.0;
                for (ki, ji) in &rows[i] {
                    for (kj, jj) in &rows[j] {
                        if ki == kj {
                            v += ji * jj / w[*ki];
                        }
                    }
                }
                a[i * m + j] = v;
                a[j * m + i] = v;
            }
        }
        // Raise the damping until a step lowers the residuals.
        loop {
            let mut damped = a.clone();
            for i in 0..m {
                damped[i * m + i] += lambda * a[i * m + i] + 1e-12;
            }
            if let Some(y) = cholesky_solve(&mut damped, &r, m) {
                let mut trial = x.to_vec();
                for (row, yi) in rows.iter().zip(&y) {
                    for (k, d) in row {
                        trial[*k] -= d * yi / w[*k];
                    }
                }
                let tr = all_residuals(cons, &trial);
                if norm2(&tr) < norm2(&r) {
                    x.copy_from_slice(&trial);
                    r = tr;
                    lambda = (lambda / 10.0).max(1e-12);
                    break;
                }
            }
            lambda *= 10.0;
            if lambda > 1e8 {
                return false;
            }
        }
    }
    r.iter().all(|v| v.abs() < TOL)
}

/// Solve A y = b for a symmetric positive definite A (overwritten). None when it is not.
fn cholesky_solve(a: &mut [f64], b: &[f64], n: usize) -> Option<Vec<f64>> {
    for j in 0..n {
        let mut d = a[j * n + j];
        for k in 0..j {
            d -= a[j * n + k] * a[j * n + k];
        }
        if d <= 0.0 || !d.is_finite() {
            return None;
        }
        let d = d.sqrt();
        a[j * n + j] = d;
        for i in j + 1..n {
            let mut v = a[i * n + j];
            for k in 0..j {
                v -= a[i * n + k] * a[j * n + k];
            }
            a[i * n + j] = v / d;
        }
    }
    let mut y = b.to_vec();
    for i in 0..n {
        for k in 0..i {
            y[i] -= a[i * n + k] * y[k];
        }
        y[i] /= a[i * n + i];
    }
    for i in (0..n).rev() {
        for k in i + 1..n {
            y[i] -= a[k * n + i] * y[k];
        }
        y[i] /= a[i * n + i];
    }
    Some(y)
}

/// The constraints of a failed group whose removal lets the rest be satisfied: the conflict.
fn conflict_message(s: &State, cons: &[Compiled], x0: &[f64], w: &[f64]) -> String {
    // One solve per constraint of the failed group; fine for the tens of constraints a part has.
    let culprits: Vec<u64> = (0..cons.len())
        .filter(|k| {
            let rest: Vec<Compiled> = cons.iter().enumerate().filter(|(i, _)| i != k).map(|(_, c)| c.clone()).collect();
            newton(&rest, &mut x0.to_vec(), w)
        })
        .map(|k| cons[k].id)
        .collect();
    let ids = if culprits.is_empty() { cons.iter().map(|c| c.id).collect() } else { culprits };
    let labels: Vec<String> = s.constraints.iter().filter(|c| ids.contains(&c.id)).map(Constraint::label).collect();
    format!("the constraints cannot all be satisfied; these conflict: {}", labels.join(", "))
}

/// Whether each constraint of `s` is satisfied now (by id).
pub fn satisfied(s: &State) -> HashMap<u64, bool> {
    let Ok(sys) = compile(s) else { return HashMap::new() };
    sys.cons
        .iter()
        .map(|c| {
            let mut r = Vec::new();
            residuals(c, &sys.x, &mut r);
            (c.id, r.iter().all(|v| v.abs() < 1e-6))
        })
        .collect()
}
