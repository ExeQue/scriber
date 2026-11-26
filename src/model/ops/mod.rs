//! CAD operations as pure functions on contours. `Document` applies them to items.

mod array;
mod boolean;
mod chamfer;
mod corner;
mod fillet;
mod offset;
mod trim;

pub use array::{grid_offsets, polar_angles};
pub use boolean::{BoolOp, boolean};
pub use chamfer::chamfer;
pub use corner::join_ends;
pub use fillet::fillet;
pub use offset::offset;
pub use trim::trim;

use super::geom::{Contour, Vertex};
use cavalier_contours::polyline::{PlineOrientation, PlineSource, PlineSourceMut, Polyline};

/// Distances below this (mm) count as zero.
const EPS: f64 = 1e-9;

fn to_contour(p: &impl PlineSource<Num = f64>) -> Contour {
    Contour { vertices: p.iter_vertexes().map(|v| Vertex::new(v.x, v.y, v.bulge)).collect(), closed: p.is_closed() }
}

/// The closed contour as a polyline wound the given way (cavalier's counter-clockwise = positive area).
fn wound(c: &Contour, ccw: bool) -> Polyline<f64> {
    let mut p = c.pline();
    if (p.orientation() == PlineOrientation::CounterClockwise) != ccw {
        p.invert_direction_mut();
    }
    p
}

/// How many of `all` contain `c` (closed ones only): even = outline, odd = hole (even-odd rule).
fn depth(c: &Contour, all: &[Contour]) -> usize {
    let p = c.vertices.first().map(|v| [v.x, v.y]).unwrap_or_default();
    all.iter().filter(|o| !std::ptr::eq(*o, c) && o.contains(p)).count()
}
