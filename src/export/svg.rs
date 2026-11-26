//! SVG in millimetres, coloured the way laser software expects: red cut, blue score, black engrave.

use crate::model::geom::arc_params;
use crate::model::{Contour, Item, Op, Shape, State};
use base64::Engine;
use std::fmt::Write;

const HAIRLINE: f64 = 0.1; // laser software treats thin strokes as vector lines
const BLACK: &str = "#000000";

/// One group per laser step, in the order a job runs: engrave first, cut last.
#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Images,
    Fill,
    Engrave,
    Score,
    Cut,
    Annotate,
}

const LAYERS: [(Layer, &str); 6] = [
    (Layer::Images, "images"),
    (Layer::Fill, "fill"),
    (Layer::Engrave, "engrave"),
    (Layer::Score, "score"),
    (Layer::Cut, "cut"),
    (Layer::Annotate, "annotate"),
];

/// The document as SVG. Annotations (notes, dimensions) are only included when asked for.
pub fn svg(s: &State, annotations: bool) -> String {
    svg_with_stroke(s, annotations, HAIRLINE)
}

/// Same, with lines `stroke` mm wide (PNG rendering needs them at least a pixel wide to be visible).
pub(super) fn svg_with_stroke(s: &State, annotations: bool, stroke: f64) -> String {
    let (w, h) = (s.width, s.height);
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n"
    );
    for (layer, name) in LAYERS {
        if layer == Layer::Annotate && !annotations {
            continue;
        }
        let body: String = s.items.iter().filter_map(|i| element(s, i, layer, stroke)).collect();
        if !body.is_empty() {
            writeln!(out, "<g id=\"{name}\">\n{body}</g>").unwrap();
        }
    }
    out.push_str("</svg>\n");
    out
}

/// The item's part in `layer`, if it has one.
fn element(s: &State, item: &Item, layer: Layer, stroke: f64) -> Option<String> {
    let id = item.id;
    let line_layer = match item.op {
        Op::Cut => Some(Layer::Cut),
        Op::Score => Some(Layer::Score),
        Op::Engrave => Some(Layer::Engrave),
        Op::Annotate => Some(Layer::Annotate),
        Op::NoLine => None,
    };
    match &item.shape {
        Shape::Image { image, x, y, w, h, rotation } => {
            if layer != Layer::Images || item.op == Op::Annotate {
                return None;
            }
            let img = s.images.get(image)?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&img.png);
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            Some(format!(
                "<image id=\"i{id}\" x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" transform=\"rotate({rotation} {cx} {cy})\" href=\"data:image/png;base64,{b64}\"/>\n"
            ))
        }
        Shape::Path { contours } => {
            let d: String = contours.iter().map(path_data).collect::<Vec<_>>().join(" ");
            if layer == Layer::Fill && item.engraves_area() {
                return Some(format!("<path id=\"i{id}-fill\" d=\"{d}\" fill=\"{BLACK}\" fill-rule=\"evenodd\" stroke=\"none\"/>\n"));
            }
            (Some(layer) == line_layer).then(|| {
                format!("<path id=\"i{id}\" d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{stroke}\"/>\n", item.op.hex())
            })
        }
        Shape::Text { x, y, size, text, rotation } => {
            // ponytail: text stays <text>; convert it to outlines (usvg can) when laser software needs paths.
            let paint = if layer == Layer::Fill && item.engraves_area() {
                format!("fill=\"{BLACK}\" stroke=\"none\"")
            } else if Some(layer) == line_layer {
                format!("fill=\"none\" stroke=\"{}\" stroke-width=\"{stroke}\"", item.op.hex())
            } else {
                return None;
            };
            Some(format!(
                "<text id=\"i{id}\" x=\"{x}\" y=\"{y}\" font-size=\"{size}\" font-family=\"sans-serif\" {paint} transform=\"rotate({rotation} {x} {y})\">{}</text>\n",
                escape(text)
            ))
        }
        Shape::Dimension { .. } => {
            if layer != Layer::Annotate {
                return None;
            }
            let g = item.shape.dimension()?;
            let color = Op::Annotate.hex();
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

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// SVG path data for one contour; arcs stay real arcs.
fn path_data(c: &Contour) -> String {
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
        d.add("hole", Op::Cut, false, Shape::path(Contour::circle(20.0, 20.0, 10.0)));
        d.add("arc", Op::Score, false, Shape::path(Contour::arc(60.0, 25.0, 10.0, 0.0, 270.0)));
        d.add("t", Op::NoLine, true, Shape::Text { x: 5.0, y: 45.0, size: 5.0, text: "A&B".into(), rotation: 0.0 });
        d.add("dim", Op::Annotate, false, Shape::Dimension { x1: 10.0, y1: 40.0, x2: 30.0, y2: 40.0, offset: 5.0 });
        let plate = Shape::Path { contours: vec![Contour::rect(70.0, 5.0, 20.0, 20.0, 0.0), Contour::circle(80.0, 15.0, 3.0)] };
        d.add("coaster", Op::Cut, true, plate);

        let s = svg(&d.s, false);
        assert!(s.contains("width=\"100mm\""));
        assert!(s.contains("A10 10 0 0 1 30 20"));
        assert!(s.contains("A10 10 0 1 1"), "270° arc needs the large-arc flag: {s}");
        assert!(s.contains("A&amp;B"));
        assert!(!s.contains("20.00 mm"));
        assert!(svg(&d.s, true).contains("20.00 mm"));
        // The coaster is both a filled compound path and a cut line; fill comes before cut.
        let fill = s.find("i5-fill").unwrap();
        assert!(s[fill..].starts_with("i5-fill\" d=\"M70 5") && s.contains("fill-rule=\"evenodd\""));
        assert!(fill < s.find("<g id=\"cut\"").unwrap());
        assert!(s.find("<g id=\"cut\"").unwrap() > s.find("<g id=\"score\"").unwrap());
    }
}
