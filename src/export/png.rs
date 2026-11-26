//! PNG export: renders the SVG export, so both always look the same.

use crate::model::State;
/// SVG options with the system fonts loaded once; shared with text outlines and SVG import.
pub(super) use crate::model::text::usvg_options as options;
use resvg::{tiny_skia, usvg};

/// Render the document at `dpi` on a white background.
pub fn png(s: &State, dpi: f64, annotations: bool) -> anyhow::Result<Vec<u8>> {
    // Laser hairlines would be sub-pixel; draw lines at least 1.5 px wide.
    let stroke = (1.5 * 25.4 / dpi).max(0.1);
    render(s, dpi, &super::svg::svg_with_stroke(s, annotations, stroke))
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
    fn preview_shows_material_burn_pieces_and_holes() {
        let mut d = Document::new(100.0, 50.0);
        d.s.material = Some("basswood-plywood-3mm".into());
        d.add("plate", Op::Cut, Shape::Path { contours: vec![Contour::rect(10.0, 10.0, 30.0, 30.0, 0.0), Contour::circle(25.0, 25.0, 5.0)] });
        d.add("face", Op::Engrave, Shape::path(Contour::rect(60.0, 10.0, 10.0, 10.0, 0.0)));
        d.add("note", Op::NoOutput, Shape::path(Contour::rect(80.0, 10.0, 10.0, 10.0, 0.0)));
        let img = image::load_from_memory(&super::preview_png(&d.s, 50.8).unwrap()).unwrap().to_rgb8();
        let p = crate::model::preview::palette(&d.s);
        assert_eq!(img.get_pixel(190, 90).0, p.sheet, "bare material");
        assert_eq!(img.get_pixel(30, 30).0, p.piece, "the cut-out piece");
        assert_eq!(img.get_pixel(50, 50).0, p.bed, "its hole");
        assert_eq!(img.get_pixel(130, 30).0, p.burn, "engraved area");
        assert_eq!(img.get_pixel(170, 30).0, p.sheet, "no-output items are not lasered");
    }
}
