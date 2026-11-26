//! Drawing geometry per item, kept between frames: the outline flattened to points and the fill
//! as a triangle mesh (even-odd, so holes stay open). Both are in millimetres and only rebuilt when
//! the shape changes or the zoom needs a finer or coarser curve; each frame just maps the points
//! to the screen.

use crate::model::{Item, Point, Shape};
use lyon_tessellation::geom::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers};
use std::collections::HashMap;

pub struct Geometry {
    shape: Shape,
    tol: f64,
    pub outline: Vec<Vec<Point>>,
    /// Triangle mesh of the filled area: vertices in mm and indices.
    pub fill: Option<(Vec<[f32; 2]>, Vec<u32>)>,
}

#[derive(Default)]
pub struct GeometryCache {
    items: HashMap<u64, Geometry>,
}

/// Round the curve tolerance down to a power of two, so zooming a little reuses the cache.
fn bucket(tol: f64) -> f64 {
    2f64.powi(tol.log2().floor() as i32)
}

impl GeometryCache {
    /// The geometry for `item` at tolerance `tol` (mm), rebuilt only when needed; `fill` when it
    /// engraves an area.
    pub fn get(&mut self, item: &Item, fill: bool, tol: f64) -> &Geometry {
        let tol = bucket(tol);
        let fresh = self.items.get(&item.id).is_some_and(|g| g.tol == tol && g.shape == item.shape);
        if !fresh {
            let outline = item.shape.outline(tol);
            let fill = fill.then(|| tessellate(&outline)).flatten();
            self.items.insert(item.id, Geometry { shape: item.shape.clone(), tol, outline, fill });
        }
        let g = self.items.get_mut(&item.id).unwrap();
        // The op can change without the shape changing.
        if fill && g.fill.is_none() {
            g.fill = tessellate(&g.outline);
        } else if !fill {
            g.fill = None;
        }
        g
    }

    /// Forget items that are gone.
    pub fn retain(&mut self, ids: &[u64]) {
        self.items.retain(|id, _| ids.contains(id));
    }
}

/// Even-odd fill of closed polylines as triangles.
pub(super) fn tessellate(polys: &[Vec<Point>]) -> Option<(Vec<[f32; 2]>, Vec<u32>)> {
    let mut builder = Path::builder();
    for poly in polys.iter().filter(|p| p.len() >= 3) {
        builder.begin(point(poly[0][0] as f32, poly[0][1] as f32));
        for p in &poly[1..] {
            builder.line_to(point(p[0] as f32, p[1] as f32));
        }
        builder.end(true);
    }
    let path = builder.build();
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let options = FillOptions::default().with_fill_rule(FillRule::EvenOdd);
    FillTessellator::new()
        .tessellate_path(&path, &options, &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position().to_array()))
        .ok()?;
    (!buffers.indices.is_empty()).then_some((buffers.vertices, buffers.indices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Contour;

    #[test]
    fn fill_keeps_holes_and_cache_is_reused() {
        let shape = Shape::Path { contours: vec![Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0), Contour::rect(3.0, 3.0, 4.0, 4.0, 0.0)] };
        let item = Item { id: 1, name: String::new(), layer: 1, shape, note: String::new(), rotation: 0.0 };
        let mut cache = GeometryCache::default();
        let (verts, idx) = cache.get(&item, true, 0.1).fill.clone().unwrap();
        // Area of the triangles = outer square minus the hole.
        let area: f32 = idx
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]];
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum();
        assert!((area - 84.0).abs() < 1e-3, "{area}");
        let first = cache.get(&item, true, 0.1).outline.as_ptr();
        assert_eq!(cache.get(&item, true, 0.11).outline.as_ptr(), first, "same bucket reuses the cache");
    }
}
