//! PNG export: renders the SVG export, so both always look the same.

use crate::model::State;
/// SVG options with the system fonts loaded once; shared with text outlines and SVG import.
pub(super) use crate::model::text::usvg_options as options;
use resvg::{tiny_skia, usvg};

/// Longest side of a PNG when no DPI is asked for: sharp, yet small enough for an agent to look at.
pub const DEFAULT_PX: f64 = 2048.0;
/// Longest side of an image returned inline (the Claude API refuses images over 8000 px).
const MAX_INLINE_PX: f64 = 8000.0;

/// The DPI to render at: `asked`, else the one that fits the material's longest side in
/// DEFAULT_PX pixels; an `inline` image (returned to the agent) is kept under MAX_INLINE_PX.
pub fn dpi(s: &State, asked: Option<f64>, inline: bool) -> f64 {
    let inches = s.width.max(s.height) / 25.4;
    let dpi = asked.unwrap_or(DEFAULT_PX / inches);
    if inline { dpi.min(MAX_INLINE_PX / inches) } else { dpi }
}

/// Render the document at `dpi` on a white background, every item in its layer's colour (as on the canvas).
pub fn png(s: &State, dpi: f64, annotations: bool) -> anyhow::Result<Vec<u8>> {
    // Laser hairlines would be sub-pixel; draw lines at least 1.5 px wide.
    let stroke = (1.5 * 25.4 / dpi).max(0.1);
    render(s, dpi, &super::svg::svg_with_stroke(s, annotations, stroke, true))
}

/// Render what the laser makes of the document (the Preview look) at `dpi`.
pub fn preview_png(s: &State, dpi: f64) -> anyhow::Result<Vec<u8>> {
    render(s, dpi, &super::preview::svg(s, 25.4 / dpi))
}

fn render(s: &State, dpi: f64, svg: &str) -> anyhow::Result<Vec<u8>> {
    let tree = usvg::Tree::from_str(svg, options())?;
    let px_w = (s.width / 25.4 * dpi).round().max(1.0) as u32;
    let px_h = (s.height / 25.4 * dpi).round().max(1.0) as u32;
    anyhow::ensure!(px_w as u64 * px_h as u64 <= 400_000_000, "image too large ({px_w}×{px_h} px); lower the DPI");
    let mut pixmap = tiny_skia::Pixmap::new(px_w, px_h).ok_or_else(|| anyhow::anyhow!("cannot allocate {px_w}×{px_h} px"))?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let scale = px_w as f32 / tree.size().width();
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}

#[cfg(test)]
mod tests {
    use crate::model::{Contour, Document, Op, Shape};

    #[test]
    fn renders_lines_fills_and_holes() {
        let mut d = Document::new(100.0, 50.0);
        d.add("hole", Op::Cut, Shape::path(Contour::circle(20.0, 20.0, 10.0)));
        let plate = Shape::Path { contours: vec![Contour::rect(70.0, 5.0, 20.0, 20.0, 0.0), Contour::circle(80.0, 15.0, 3.0)] };
        d.add("coaster", Op::Engrave, plate);
        let png = super::png(&d.s, 50.8, true).unwrap(); // 2 px per mm
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!((img.width(), img.height()), (200, 100));
        assert_eq!(img.get_pixel(146, 16).0, [0, 0, 0], "filled area is black");
        assert_eq!(img.get_pixel(160, 30).0, [255, 255, 255], "the hole stays empty");
        assert_eq!(img.get_pixel(100, 5).0, [255, 255, 255]);
        let red = img.get_pixel(20, 40).0;
        assert!(red[0] > 150 && red[1] < 120, "{red:?}");
    }

    #[test]
    fn every_engrave_layer_in_its_own_colour() {
        let mut d = Document::new(100.0, 50.0);
        d.add("medium", Op::Engrave, Shape::path(Contour::rect(10.0, 10.0, 20.0, 20.0, 0.0)));
        let light = d.add_layer("Engrave light", Op::Engrave);
        d.s.layer_mut(light).unwrap().color = Some([230, 140, 30]);
        d.add_to(light, "dot", Shape::path(Contour::circle(60.0, 20.0, 5.0)));
        d.add_to(light, "line", Shape::path(Contour::line([70.0, 40.0], [90.0, 40.0])));
        let img = image::load_from_memory(&super::png(&d.s, 50.8, true).unwrap()).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(40, 40).0, [0, 0, 0], "the first engrave layer, default colour");
        assert_eq!(img.get_pixel(120, 40).0, [230, 140, 30], "the second engrave layer's area");
        let line = img.get_pixel(160, 80).0; // anti-aliased edge of a 1.5 px line
        assert!(line[0] > line[2] + 60 && line[1] > line[2], "its open path, scored: {line:?}");
    }

    #[test]
    fn default_size_fills_the_cap() {
        let d = Document::new(100.0, 50.0);
        let dpi = super::dpi(&d.s, None, true);
        let img = image::load_from_memory(&super::preview_png(&d.s, dpi).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (2048, 1024), "{dpi} dpi");
        assert_eq!(super::dpi(&d.s, Some(100.0), true), 100.0, "an asked DPI is kept");
        assert!(super::dpi(&d.s, Some(10_000.0), true) * 100.0 / 25.4 <= 8000.0, "inline images stay under the API limit");
        assert_eq!(super::dpi(&d.s, Some(10_000.0), false), 10_000.0);
    }

    #[test]
    fn preview_shows_material_burn_pieces_and_holes() {
        let mut d = Document::new(100.0, 50.0);
        d.s.material = Some("basswood-plywood-3mm".into());
        d.add("plate", Op::Cut, Shape::Path { contours: vec![Contour::rect(10.0, 10.0, 30.0, 30.0, 0.0), Contour::circle(25.0, 25.0, 5.0)] });
        d.add("face", Op::Engrave, Shape::path(Contour::rect(60.0, 10.0, 10.0, 10.0, 0.0)));
        d.add("note", Op::NoOutput, Shape::path(Contour::rect(80.0, 10.0, 10.0, 10.0, 0.0)));
        let img = image::load_from_memory(&super::preview_png(&d.s, 50.8).unwrap()).unwrap().to_rgb8();
        let p = crate::model::preview::palette(&d.s, &crate::library::get());
        assert_eq!(img.get_pixel(190, 90).0, p.sheet, "bare material");
        assert_eq!(img.get_pixel(30, 30).0, p.piece, "the cut-out piece");
        assert_eq!(img.get_pixel(50, 50).0, p.bed, "its hole");
        assert_eq!(img.get_pixel(130, 30).0, p.burn, "engraved area");
        assert_eq!(img.get_pixel(170, 30).0, p.sheet, "no-output items are not lasered");
    }

    #[test]
    fn preview_burns_images_like_engraved_areas() {
        let mut d = Document::new(100.0, 50.0);
        d.s.material = Some("basswood-plywood-3mm".into());
        d.add("plate", Op::Cut, Shape::path(Contour::rect(5.0, 5.0, 60.0, 40.0, 0.0)));
        d.add("face", Op::Engrave, Shape::path(Contour::rect(70.0, 10.0, 10.0, 10.0, 0.0)));
        // Left half black, right half white: white must not paint a white box over the piece.
        let grey = image::GrayImage::from_fn(20, 10, |x, _| image::Luma([if x < 10 { 0 } else { 255 }]));
        let img = crate::export::image::to_grey(&crate::export::image::encode_png(&grey).unwrap()).unwrap();
        d.place_image(img, [10.0, 10.0], Some(40.0), "logo");
        let out = image::load_from_memory(&super::preview_png(&d.s, 50.8).unwrap()).unwrap().to_rgb8();
        let p = crate::model::preview::palette(&d.s, &crate::library::get());
        assert_eq!(out.get_pixel(150, 30).0, out.get_pixel(40, 30).0, "black pixels burn like the engraved area");
        assert_eq!(out.get_pixel(40, 30).0, p.burn);
        assert_eq!(out.get_pixel(80, 30).0, p.piece, "white pixels leave the cut-out piece as it is");
    }

    #[test]
    fn preview_burn_follows_the_layer_energy() {
        use crate::model::layer::Overrides;
        let mut d = Document::new(100.0, 50.0);
        let engrave = |d: &mut Document, name: &str, power: f64, x: f64| {
            let l = d.add_layer(name, Op::Engrave);
            let layer = d.s.layer_mut(l).unwrap();
            layer.settings = Overrides { power: Some(power), speed: Some(140.0), passes: Some(1) };
            layer.line_interval = Some(10.0 / 120.0);
            d.add_to(l, name, Shape::path(Contour::rect(x, 10.0, 10.0, 10.0, 0.0)));
        };
        engrave(&mut d, "Lys", 40.0, 10.0);
        engrave(&mut d, "Medium", 60.0, 30.0);
        engrave(&mut d, "Dyb", 80.0, 50.0);
        let img = image::load_from_memory(&super::preview_png(&d.s, 50.8).unwrap()).unwrap().to_rgb8();
        let lum = |x| img.get_pixel(x, 30).0.iter().map(|v| *v as u32).sum::<u32>();
        let (light, medium, deep) = (lum(30), lum(70), lum(110));
        assert!(light > medium + 30 && medium > deep + 30, "{light} > {medium} > {deep}");
    }
}
