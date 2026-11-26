//! What the laser makes of the document, for the Preview mode and the preview PNG: the same rules as
//! the SVG export (`Item::effective_op`), sorted into what to paint. Painting lives in the GUI and export.

use super::{Contour, Item, Op, Shape, State};

/// RGB colours of the preview.
pub struct Palette {
    pub sheet: [u8; 3],
    /// A piece that is cut free, a shade lighter so it reads as loose.
    pub piece: [u8; 3],
    /// What shows through a hole (the machine bed).
    pub bed: [u8; 3],
    pub burn: [u8; 3],
    pub cut: [u8; 3],
    pub score: [u8; 3],
}

/// Wood when a material is chosen, a neutral grey when not.
pub fn palette(s: &State) -> Palette {
    if s.material.is_some() {
        Palette { sheet: [214, 178, 128], piece: [224, 190, 142], bed: [40, 40, 44], burn: [74, 44, 22], cut: [40, 24, 12], score: [96, 60, 30] }
    } else {
        Palette { sheet: [222, 222, 218], piece: [234, 234, 230], bed: [40, 40, 44], burn: [62, 62, 62], cut: [30, 30, 30], score: [90, 90, 90] }
    }
}

pub struct Scene<'a> {
    /// Closed cut outlines, biggest first: a piece cut free, or a hole when it lies inside a piece.
    pub pieces: Vec<(Contour, bool)>,
    /// Engraved areas (paths and text) and images, in document order.
    pub engraved: Vec<&'a Item>,
    /// Laser lines with the op that makes them (cut or score), drawn over everything.
    pub lines: Vec<(Contour, Op)>,
}

pub fn scene(s: &State) -> Scene<'_> {
    let mut scene = Scene { pieces: Vec::new(), engraved: Vec::new(), lines: Vec::new() };
    for item in &s.items {
        let op = item.effective_op();
        match &item.shape {
            // Like the export: an image is engraved unless it is set to no output.
            Shape::Image { .. } if item.op != Op::NoOutput => scene.engraved.push(item),
            Shape::Path { .. } | Shape::Text { .. } if op != Op::NoOutput => {
                if item.engraves_area() {
                    scene.engraved.push(item);
                    continue;
                }
                for c in item.shape.laser_contours().iter() {
                    if op == Op::Cut && c.closed {
                        scene.pieces.push((c.clone(), false));
                    }
                    scene.lines.push((c.clone(), op));
                }
            }
            _ => {}
        }
    }
    // Even-odd over all cut outlines: inside an odd number of bigger ones is a hole.
    scene.pieces.sort_by(|a, b| b.0.area().total_cmp(&a.0.area()));
    let holes: Vec<bool> = (0..scene.pieces.len())
        .map(|i| {
            let c = &scene.pieces[i].0;
            let p = c.vertices.first().map(|v| [v.x, v.y]).unwrap_or_default();
            scene.pieces[..i].iter().filter(|(o, _)| o.contains(p)).count() % 2 == 1
        })
        .collect();
    scene.pieces.iter_mut().zip(holes).for_each(|(p, h)| p.1 = h);
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, Guide};

    #[test]
    fn pieces_holes_and_what_is_left_out() {
        let mut d = Document::new(100.0, 100.0);
        d.add("hole", Op::Cut, Shape::path(Contour::circle(30.0, 30.0, 5.0)));
        d.add("plate", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 40.0, 40.0, 0.0)));
        d.add("in hole", Op::Cut, Shape::path(Contour::circle(30.0, 30.0, 2.0)));
        d.add("face", Op::Engrave, Shape::path(Contour::rect(12.0, 12.0, 5.0, 5.0, 0.0)));
        d.add("groove", Op::Engrave, Shape::path(Contour::line([0.0, 0.0], [5.0, 5.0])));
        d.add("note", Op::NoOutput, Shape::path(Contour::rect(0.0, 0.0, 5.0, 5.0, 0.0)));
        d.add("guide", Op::Cut, Shape::Guide(Guide::Xline { x: 0.0, y: 0.0, angle: 0.0 }));
        let s = scene(&d.s);
        let holes: Vec<bool> = s.pieces.iter().map(|p| p.1).collect();
        assert_eq!(holes, [false, true, false], "plate, its hole, a piece inside the hole");
        assert_eq!(s.engraved.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["face"]);
        let ops: Vec<Op> = s.lines.iter().map(|l| l.1).collect();
        assert_eq!(ops, [Op::Cut, Op::Cut, Op::Cut, Op::Score], "the open engrave line is scored");
    }
}
