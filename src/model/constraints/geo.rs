//! Constraints compiled against a state: the unknowns the solver may change (vertex coordinates and
//! arc bulges) and, for every constraint, where its geometry sits among them.

use super::{Constraint, Kind, Ref};
use crate::model::{Contour, Point, Shape, State};
use std::collections::HashMap;

/// (item, contour, vertex or segment index).
pub type Key = (u64, usize, usize);

/// What one unknown is: a vertex's x (its y is the next unknown) or a segment's bulge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Var {
    Vertex(Key),
    Bulge(Key),
}

/// Where a bulge comes from: an unknown, or fixed (a full circle's halves stay half circles).
#[derive(Clone, Copy, Debug)]
pub enum Bulge {
    Var(usize),
    Fixed(f64),
}

/// A reference resolved to unknowns (indices into the unknown vector; a point uses i and i + 1).
#[derive(Clone, Debug)]
pub enum G {
    Point(usize),
    Line(usize, usize),
    Arc(usize, usize, Bulge),
    Centre(usize, usize, Bulge),
    Item(Vec<usize>),
}

/// One constraint, ready for the solver.
#[derive(Clone, Debug)]
pub struct Compiled {
    pub id: u64,
    pub kind: Kind,
    pub g: Vec<G>,
    /// Dimensional: the parameter's value (mm or degrees).
    pub value: f64,
    /// Every unknown it depends on.
    pub vars: Vec<usize>,
    /// Fix: the value each of `vars` must keep.
    pub target: Vec<f64>,
}

#[derive(Default)]
pub struct System {
    pub x: Vec<f64>,
    pub vars: Vec<Var>,
    index: HashMap<Var, usize>,
    pub cons: Vec<Compiled>,
}

pub fn path_contours(s: &State, item: u64) -> Result<&[Contour], String> {
    match s.items.iter().find(|i| i.id == item).map(|i| &i.shape) {
        Some(Shape::Path { contours }) => Ok(contours),
        Some(_) => Err(format!("item {item} is not a path; only paths can be constrained")),
        None => Err(format!("there is no item {item}")),
    }
}

impl System {
    fn unknown(&mut self, var: Var, init: &[f64]) -> usize {
        if let Some(i) = self.index.get(&var) {
            return *i;
        }
        let i = self.x.len();
        self.x.extend_from_slice(init);
        self.vars.extend(std::iter::repeat_n(var, init.len()));
        self.index.insert(var, i);
        i
    }

    fn vertex(&mut self, c: &Contour, key: Key) -> usize {
        let v = c.vertices[key.2];
        self.unknown(Var::Vertex(key), &[v.x, v.y])
    }

    /// Resolve a reference, adding the unknowns it needs.
    pub fn resolve(&mut self, s: &State, r: &Ref) -> Result<G, String> {
        let item = r.item();
        let contours = path_contours(s, item)?;
        let Some(ci) = r.contour() else {
            let mut all = Vec::new();
            for (ci, c) in contours.iter().enumerate() {
                for vi in 0..c.vertices.len() {
                    all.push(self.vertex(c, (item, ci, vi)));
                }
            }
            return Ok(G::Item(all));
        };
        let c = contours.get(ci).ok_or_else(|| format!("item {item} has no contour {ci}"))?;
        let n = c.vertices.len();
        match *r {
            Ref::Point { vertex, .. } if vertex < n => Ok(G::Point(self.vertex(c, (item, ci, vertex)))),
            Ref::Point { vertex, .. } => Err(format!("item {item} contour {ci} has no vertex {vertex}")),
            Ref::Segment { segment, .. } | Ref::Centre { segment, .. } => {
                let segments = if c.closed { n } else { n.saturating_sub(1) };
                if segment >= segments {
                    return Err(format!("item {item} contour {ci} has no segment {segment}"));
                }
                let a = self.vertex(c, (item, ci, segment));
                let b = self.vertex(c, (item, ci, (segment + 1) % n));
                let bulge = c.vertices[segment].bulge;
                if bulge.abs() < 1e-12 {
                    return match r {
                        Ref::Centre { .. } => Err(format!("segment {segment} of item {item} is straight; it has no centre")),
                        _ => Ok(G::Line(a, b)),
                    };
                }
                let bulge = if c.as_circle().is_some() { Bulge::Fixed(bulge) } else { Bulge::Var(self.unknown(Var::Bulge((item, ci, segment)), &[bulge])) };
                Ok(if matches!(r, Ref::Centre { .. }) { G::Centre(a, b, bulge) } else { G::Arc(a, b, bulge) })
            }
            Ref::Item { .. } => unreachable!(),
        }
    }

    /// Compile one constraint; `value` is its parameter's value (dimensional ones).
    pub fn add(&mut self, s: &State, c: &Constraint, value: f64) -> Result<(), String> {
        if !c.kind.arity().contains(&c.refs.len()) {
            return Err(format!("{} takes {:?} references, not {}", c.kind.name(), c.kind.arity(), c.refs.len()));
        }
        if c.kind == Kind::Regular {
            regular_shape(s, &c.refs[0])?;
        }
        let g = c.refs.iter().map(|r| self.resolve(s, r)).collect::<Result<Vec<_>, _>>()?;
        let mut vars: Vec<usize> = g.iter().flat_map(G::vars).collect();
        if c.kind != Kind::Fix {
            vars.sort_unstable();
            vars.dedup();
        }
        let target = vars.iter().map(|i| self.x[*i]).collect();
        let compiled = Compiled { id: c.id, kind: c.kind, g, value, vars, target };
        if !super::residual::residuals(&compiled, &self.x, &mut Vec::new()) {
            return Err(format!("{} does not apply to this geometry ({})", c.kind.name(), describe(&compiled.g)));
        }
        self.cons.push(compiled);
        Ok(())
    }
}

/// Regular applies to a whole item that is one closed contour of 3 or more straight sides.
fn regular_shape(s: &State, r: &Ref) -> Result<(), String> {
    let Ref::Item { item } = *r else { return Err("regular takes a whole item".into()) };
    match path_contours(s, item)? {
        [c] if c.closed && c.vertices.len() >= 3 && c.vertices.iter().all(|v| v.bulge == 0.0) => Ok(()),
        _ => Err(format!("item {item} is not one closed shape with straight sides")),
    }
}

fn describe(g: &[G]) -> String {
    let names: Vec<&str> = g
        .iter()
        .map(|g| match g {
            G::Point(_) => "point",
            G::Line(..) => "line",
            G::Arc(..) => "arc/circle",
            G::Centre(..) => "centre",
            G::Item(_) => "item",
        })
        .collect();
    names.join(", ")
}

fn pt(x: &[f64], i: usize) -> Point {
    [x[i], x[i + 1]]
}

impl G {
    pub fn vars(&self) -> Vec<usize> {
        let b = |bulge: &Bulge| if let Bulge::Var(i) = bulge { vec![*i] } else { vec![] };
        match self {
            G::Point(i) => vec![*i, i + 1],
            G::Line(a, c) => vec![*a, a + 1, *c, c + 1],
            G::Arc(a, c, bl) | G::Centre(a, c, bl) => [vec![*a, a + 1, *c, c + 1], b(bl)].concat(),
            G::Item(pts) => pts.iter().flat_map(|i| [*i, i + 1]).collect(),
        }
    }

    /// A point: a vertex or an arc's centre.
    pub fn point(&self, x: &[f64]) -> Option<Point> {
        match self {
            G::Point(i) => Some(pt(x, *i)),
            G::Centre(..) => self.circle(x).map(|c| c.0),
            _ => None,
        }
    }

    pub fn line(&self, x: &[f64]) -> Option<(Point, Point)> {
        match self {
            G::Line(a, b) => Some((pt(x, *a), pt(x, *b))),
            _ => None,
        }
    }

    /// Centre and radius of an arc or circle.
    pub fn circle(&self, x: &[f64]) -> Option<(Point, f64)> {
        match self {
            G::Arc(a, b, bl) | G::Centre(a, b, bl) => {
                let bulge = match bl {
                    Bulge::Var(i) => x[*i],
                    Bulge::Fixed(v) => *v,
                };
                let (c, r, _, _) = crate::model::geom::arc_params(pt(x, *a), pt(x, *b), bulge);
                Some((c, r))
            }
            _ => None,
        }
    }

    /// The two ends of a line or arc.
    pub fn ends(&self, x: &[f64]) -> Option<[Point; 2]> {
        match self {
            G::Line(a, b) | G::Arc(a, b, _) => Some([pt(x, *a), pt(x, *b)]),
            _ => None,
        }
    }
}
