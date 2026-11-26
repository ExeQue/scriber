//! Parametric constraints, as in AutoCAD: geometric ones (coincident, parallel, tangent, …) bind
//! geometry together; dimensional ones (d1 = 40) bind it to a named parameter whose value can be an
//! expression of other parameters. The solver keeps them all satisfied after every edit.
//!
//! - `expr`: the parameter expressions;
//! - `geo`: constraints compiled into unknowns (vertex coordinates, arc bulges);
//! - `residual`: how far each constraint is from satisfied, and what dimensions measure;
//! - `solve`: the damped Gauss–Newton solver;
//! - `infer`: AUTOCONSTRAIN;
//! - `attach`: constraints inferred while drawing (INFER);
//! - `doc`: the `Document` commands, and `settle`, which runs the solver after every change.

pub mod attach;
mod doc;
pub mod expr;
mod geo;
mod infer;
pub mod pick;
mod remap;
mod residual;
pub mod selection;
mod solve;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod attach_tests;
#[cfg(test)]
mod trim_tests;

pub(crate) use doc::{distinct, freeze_orphans};
pub use solve::parameter_values;

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

/// A piece of geometry a constraint points at: always in a path item (`contour` defaults to 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ref {
    /// A vertex: a line's end, a polyline corner, an arc's end.
    Point {
        item: u64,
        #[serde(default)]
        contour: usize,
        vertex: usize,
    },
    /// The segment from vertex `segment` to the next one: a line, or an arc (its circle). For a
    /// circle, segment 0 is the whole circle.
    Segment {
        item: u64,
        #[serde(default)]
        contour: usize,
        segment: usize,
    },
    /// The centre of the arc or circle `segment`, used as a point.
    Centre {
        item: u64,
        #[serde(default)]
        contour: usize,
        segment: usize,
    },
    /// The whole item (every vertex), for fix.
    Item { item: u64 },
}

impl Ref {
    pub fn item(&self) -> u64 {
        match *self {
            Ref::Point { item, .. } | Ref::Segment { item, .. } | Ref::Centre { item, .. } | Ref::Item { item } => item,
        }
    }

    /// The contour it is on (None for a whole item).
    pub fn contour(&self) -> Option<usize> {
        match *self {
            Ref::Point { contour, .. } | Ref::Segment { contour, .. } | Ref::Centre { contour, .. } => Some(contour),
            Ref::Item { .. } => None,
        }
    }
}

/// What a constraint keeps true. The geometric kinds are AutoCAD's GEOMCONSTRAINT, the `dim_*`
/// kinds its DIMCONSTRAINT.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Two points on top of each other, or a point on a line (its extension) or a circle/arc.
    Coincident,
    /// Two lines on one infinite line.
    Collinear,
    /// Two arcs/circles with the same centre.
    Concentric,
    /// A point, segment, centre or whole item stays where it is.
    Fix,
    Parallel,
    Perpendicular,
    /// A line, or two points, level.
    Horizontal,
    /// A line, or two points, plumb.
    Vertical,
    /// A line and an arc/circle, or two arcs/circles, touching without crossing.
    Tangent,
    /// Two segments joined end to end with no kink (G1); two arcs also get the same radius (G2).
    Smooth,
    /// Two points, lines or arcs/circles mirrored over a symmetry line (the third reference).
    Symmetric,
    /// Two lines the same length, or two arcs/circles the same radius.
    Equal,
    /// A closed straight-sided item (one contour) stays a regular polygon: equal sides, equal corners.
    Regular,
    /// Horizontal distance between two points (or a line's ends).
    DimHorizontal,
    /// Vertical distance between two points (or a line's ends).
    DimVertical,
    /// Straight distance between two points, a line's length, or a point's distance to a line.
    DimAligned,
    /// Angle between two lines, in degrees.
    DimAngular,
    DimRadial,
    DimDiameter,
}

impl Kind {
    pub const GEOMETRIC: [Kind; 12] = [
        Kind::Coincident,
        Kind::Collinear,
        Kind::Concentric,
        Kind::Fix,
        Kind::Parallel,
        Kind::Perpendicular,
        Kind::Horizontal,
        Kind::Vertical,
        Kind::Tangent,
        Kind::Smooth,
        Kind::Symmetric,
        Kind::Equal,
    ];

    pub fn is_dimension(self) -> bool {
        matches!(self, Kind::DimHorizontal | Kind::DimVertical | Kind::DimAligned | Kind::DimAngular | Kind::DimRadial | Kind::DimDiameter)
    }

    /// The name used in JSON and messages (`parallel`, `dim_aligned`).
    pub fn name(self) -> String {
        serde_json::to_value(self).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
    }

    /// How many references it takes.
    pub fn arity(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Kind::Fix | Kind::Regular | Kind::DimRadial | Kind::DimDiameter => 1..=1,
            Kind::Horizontal | Kind::Vertical | Kind::DimHorizontal | Kind::DimVertical | Kind::DimAligned => 1..=2,
            Kind::Symmetric => 3..=3,
            _ => 2..=2,
        }
    }

    /// Default parameter name prefix: d1, ang1, rad1, dia1.
    pub fn prefix(self) -> &'static str {
        match self {
            Kind::DimAngular => "ang",
            Kind::DimRadial => "rad",
            Kind::DimDiameter => "dia",
            _ => "d",
        }
    }
}

/// One constraint. Dimensional ones are also a named parameter (`name = expr`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Constraint {
    pub id: u64,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub refs: Vec<Ref>,
    /// Dimensional: the parameter name (d1, ang1, rad1, dia1).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Dimensional: the value, a number or an expression of other parameters (`d1 / 2`). mm or degrees.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expr: String,
    /// Dimensional: where the dimension is drawn: its distance from the measured points (linear,
    /// aligned), the radius of its arc (angular), or the angle of its leader in degrees (radius, diameter).
    #[serde(default)]
    pub offset: f64,
}

impl Constraint {
    /// For messages: `parallel #7` or `d1 = 40 (#9)`.
    pub fn label(&self) -> String {
        if self.kind.is_dimension() { format!("{} = {} (#{})", self.name, self.expr, self.id) } else { format!("{} #{}", self.kind.name(), self.id) }
    }
}
