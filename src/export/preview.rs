//! The laser preview as SVG (rendered to PNG): the material, burned areas, lines and loose pieces.

use crate::model::preview::{mark, palette, scene};
use crate::model::{Op, Shape, State};
use base64::Engine;
use std::fmt::Write;

pub(super) fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `px` is one pixel in mm: lines are a pixel wide and the pieces' shadow two pixels.
pub(super) fn svg(s: &State, px: f64) -> String {
    let (w, h) = (s.width, s.height);
    let lib = crate::library::get();
    let p = palette(s, &lib);
    let sc = scene(s);
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n");
    writeln!(out, "<rect width=\"{w}\" height=\"{h}\" fill=\"{}\"/>", hex(p.sheet)).unwrap();
    let shadow = 2.0 * px;
    for (c, hole) in &sc.pieces {
        let d = super::svg::path_data(c);
        if *hole {
            writeln!(out, "<path d=\"{d}\" fill=\"{}\"/>", hex(p.bed)).unwrap();
        } else {
            writeln!(out, "<path d=\"{d}\" fill=\"#000\" fill-opacity=\"0.35\" transform=\"translate({shadow} {shadow})\"/>").unwrap();
            writeln!(out, "<path d=\"{d}\" fill=\"{}\"/>", hex(p.piece)).unwrap();
        }
    }
    for item in &sc.engraved {
        if let Shape::Image { image, x, y, w, h, rotation } = &item.shape {
            let Some(img) = s.images.get(image) else { continue };
            let b64 = base64::engine::general_purpose::STANDARD.encode(&img.png);
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            // Multiply: white leaves the material as it is, black burns it.
            writeln!(out, "<image x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\" style=\"mix-blend-mode:multiply\" transform=\"rotate({rotation} {cx} {cy})\" href=\"data:image/png;base64,{b64}\"/>").unwrap();
            continue;
        }
        let d: Vec<String> = item.shape.laser_contours().iter().map(super::svg::path_data).collect();
        writeln!(out, "<path d=\"{}\" fill=\"{}\" fill-rule=\"evenodd\"/>", d.join(" "), hex(mark(s, &lib, &p, item, p.burn))).unwrap();
    }
    for (c, op, item) in &sc.lines {
        let color = if *op == Op::Cut { p.cut } else { mark(s, &lib, &p, item, p.score) };
        writeln!(out, "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{px}\"/>", super::svg::path_data(c), hex(color)).unwrap();
    }
    out.push_str("</svg>\n");
    out
}
