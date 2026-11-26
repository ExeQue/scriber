//! SVG in millimetres, coloured the way laser software expects: red cut, blue score, black engrave.

use crate::model::geom::arc_params;
use crate::model::{Contour, Item, Op, Shape, State};
use base64::Engine;
use std::fmt::Write;

const HAIRLINE: f64 = 0.1; // laser software treats thin strokes as vector lines
const BLACK: &str = "#000000";

/// Laser order of the layer groups: engraving first, cut last, no output after everything.
fn rank(op: Op) -> u8 {
    match op {
        Op::Engrave => 0,
        Op::Score => 1,
        Op::Cut => 2,
        Op::NoOutput => 3,
    }
}

/// The document as SVG. No-output items (notes, dimensions) are only included when asked for.
pub fn svg(s: &State, annotations: bool) -> String {
    svg_with_stroke(s, annotations, HAIRLINE)
}

/// Same, with lines `stroke` mm wide (PNG rendering needs them at least a pixel wide to be visible).
/// One group per shown layer (an Inkscape layer with its name), in laser order; images first in each.
pub(super) fn svg_with_stroke(s: &State, annotations: bool, stroke: f64) -> String {
    let (w, h) = (s.width, s.height);
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n"
    );
    let mut layers: Vec<_> = s.layers.iter().filter(|l| l.visible && (annotations || l.op != Op::NoOutput)).collect();
    layers.sort_by_key(|l| rank(l.op));
    for layer in layers {
        let on: Vec<&Item> = s.items.iter().filter(|i| i.layer == layer.id).collect();
        let (images, rest): (Vec<&Item>, Vec<&Item>) = on.into_iter().partition(|i| matches!(i.shape, Shape::Image { .. }));
        let body: String = images.into_iter().chain(rest).filter_map(|i| element(s, i, stroke)).collect();
        if !body.is_empty() {
            let name = escape(&layer.name);
            writeln!(out, "<g id=\"layer{}\" inkscape:groupmode=\"layer\" inkscape:label=\"{name}\">\n{body}</g>", layer.id).unwrap();
        }
    }
    out.push_str("</svg>\n");
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The item as SVG, if it goes out at all.
fn element(s: &State, item: &Item, stroke: f64) -> Option<String> {
    let id = item.id;
    let op = s.effective_op(item);
    match &item.shape {
        Shape::Image { image, x, y, w, h, rotation } => {
            if op == Op::NoOutput {
                return None;
            }
            let img = s.images.get(image)?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&img.png);
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            Some(format!(
                "<image id=\"i{id}\" x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" transform=\"rotate({rotation} {cx} {cy})\" href=\"data:image/png;base64,{b64}\"/>\n"
            ))
        }
        // Text goes out as its glyph outlines, so laser software does not need the font.
        Shape::Path { .. } | Shape::Text { .. } => {
            let d: String = item.shape.laser_contours().iter().map(path_data).collect::<Vec<_>>().join(" ");
            if d.is_empty() {
                return None;
            }
            if s.engraves_area(item) {
                return Some(format!("<path id=\"i{id}\" d=\"{d}\" fill=\"{BLACK}\" fill-rule=\"evenodd\" stroke=\"none\"/>\n"));
            }
            Some(format!("<path id=\"i{id}\" d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{stroke}\"/>\n", op.hex()))
        }
        // Guide lines are construction geometry for people: never in a laser file.
        Shape::Guide(_) => None,
        Shape::Dimension { .. } => {
            let g = item.shape.dimension()?;
            let color = op.hex();
            let mut angle = g.angle;
            if !(-90.0..=90.0).contains(&angle) {
                angle += 180.0;
            }
            let [(ax, ay), (a2x, a2y), (bx, by), (b2x, b2y), (mx, my)] = [g.a, g.a2, g.b, g.b2, g.mid].map(|p| (p[0], p[1]));
            Some(format!(
                "<g id=\"i{id}\" stroke=\"{color}\" stroke-width=\"{stroke}\" fill=\"none\"><path d=\"M{ax} {ay} L{a2x} {a2y} M{bx} {by} L{b2x} {b2y} M{a2x} {a2y} L{b2x} {b2y}\"/><text x=\"{mx}\" y=\"{}\" font-size=\"3\" font-family=\"sans-serif\" text-anchor=\"middle\" stroke=\"none\" fill=\"{color}\" transform=\"rotate({angle} {mx} {my})\">{:.2} mm</text></g>\n",
                my - 1.0,
                g.length
            ))
        }
    }
}

/// SVG path data for one contour; arcs stay real arcs.
pub(super) fn path_data(c: &Contour) -> String {
    let v = &c.vertices;
    let n = v.len();
    let Some(first) = v.first() else { return String::new() };
    let mut d = format!("M{} {}", first.x, first.y);
    let segs = if c.closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let (a, b) = (v[i], v[(i + 1) % n]);
        if a.bulge.abs() < 1e-12 {
            write!(d, " L{} {}", b.x, b.y).unwrap();
        } else {
            let (_, r, _, sweep) = arc_params([a.x, a.y], [b.x, b.y], a.bulge);
            let large = (sweep.abs() > std::f64::consts::PI) as u8;
            write!(d, " A{r} {r} 0 {large} {} {} {}", (sweep > 0.0) as u8, b.x, b.y).unwrap();
        }
    }
    if c.closed {
        d.push_str(" Z");
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Document;

    #[test]
    fn layers_colours_and_arcs() {
        let mut d = Document::new(100.0, 50.0);
        d.add("hole", Op::Cut, Shape::path(Contour::circle(20.0, 20.0, 10.0)));
        d.add("arc", Op::Score, Shape::path(Contour::arc(60.0, 25.0, 10.0, 0.0, 270.0)));
        let text = Shape::Text { x: 5.0, y: 45.0, size: 5.0, text: "A&B".into(), rotation: 0.0, font: "sans-serif".into(), bold: false, italic: false };
        let has_font = !text.laser_contours().is_empty();
        d.add("t", Op::Engrave, text);
        d.add("dim", Op::NoOutput, Shape::Dimension { x1: 10.0, y1: 40.0, x2: 30.0, y2: 40.0, offset: 5.0 });
        let plate = Shape::Path { contours: vec![Contour::rect(70.0, 5.0, 20.0, 20.0, 0.0), Contour::circle(80.0, 15.0, 3.0)] };
        d.add("face", Op::Engrave, plate.clone());
        d.add("coaster", Op::Cut, plate);
        d.add("groove", Op::Engrave, Shape::path(Contour::line([5.0, 5.0], [15.0, 5.0])));
        d.add("helper", Op::NoOutput, Shape::path(Contour::line([5.0, 8.0], [15.0, 8.0])));

        let s = svg(&d.s, false);
        assert!(s.contains("width=\"100mm\""));
        assert!(s.contains("A10 10 0 0 1 30 20"));
        assert!(s.contains("A10 10 0 1 1"), "270° arc needs the large-arc flag: {s}");
        // Text is exported as filled glyph outlines, never as <text>.
        assert!(!s.contains("<text") && !s.contains("A&"));
        assert_eq!(s.contains("<path id=\"i7\" d=\"M"), has_font, "{s}");
        // No-output items (dimension, helper line) only with annotations.
        assert!(!s.contains("20.00 mm") && !s.contains("id=\"i12\"") && !s.contains("No output"));
        let all = svg(&d.s, true);
        assert!(all.contains("20.00 mm") && all.contains("<path id=\"i12\" d=\"M5 8 L15 8\" fill=\"none\" stroke=\"#969696\""), "{all}");
        // Engrave: a closed shape is a filled area (even-odd); an open line is scored (blue, score layer).
        assert!(s.contains("<path id=\"i9\" d=\"M70 5") && s.contains("fill-rule=\"evenodd\""));
        assert!(s.contains("<path id=\"i11\" d=\"M5 5 L15 5\" fill=\"none\" stroke=\"#0000ff\""), "{s}");
        // One Inkscape layer per document layer, in laser order: engrave, score, cut.
        let at = |name: &str| s.find(&format!("inkscape:label=\"{name}\"")).unwrap();
        assert!(at("Engrave") < at("Score") && at("Score") < at("Cut"), "{s}");
        assert!(s.find("id=\"i9\"").unwrap() < at("Score") && s.find("id=\"i10\"").unwrap() > at("Cut"));
        let i11 = s.find("id=\"i11\"").unwrap();
        assert!(at("Engrave") < i11 && i11 < at("Score"), "the open engrave line stays on its layer");
        // Hidden layers are not exported.
        let cut = d.layer_for(Op::Cut);
        d.s.layer_mut(cut).unwrap().visible = false;
        assert!(!svg(&d.s, false).contains("inkscape:label=\"Cut\""));
    }

    #[test]
    fn guide_lines_are_never_exported() {
        let mut d = Document::new(100.0, 50.0);
        d.add("xl", Op::Cut, Shape::Guide(crate::model::Guide::Xline { x: 10.0, y: 10.0, angle: 0.0 }));
        let mark = crate::model::Guide::Centermark { cx: 50.0, cy: 25.0, r: 5.0, of: None };
        d.add("cm", Op::Cut, Shape::Guide(mark));
        for annotations in [false, true] {
            assert!(!svg(&d.s, annotations).contains("<path"), "{}", svg(&d.s, annotations));
        }
    }
}
