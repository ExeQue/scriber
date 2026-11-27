//! Bringing pictures in: any common format becomes a greyscale PNG.

use crate::model::Img;

/// Imports are capped at 4000 px on the long side.
const MAX_IMAGE_PX: u32 = 4000;

pub fn encode_png(img: &image::GrayImage) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    image::DynamicImage::ImageLuma8(img.clone()).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    Ok(out)
}

/// Decode an image, convert it to greyscale (transparent = white, so it is not burned) and re-encode it as PNG.
pub fn to_grey(bytes: &[u8]) -> anyhow::Result<Img> {
    let mut img = image::load_from_memory(bytes)?;
    if img.width().max(img.height()) > MAX_IMAGE_PX {
        img = img.resize(MAX_IMAGE_PX, MAX_IMAGE_PX, image::imageops::FilterType::Lanczos3);
    }
    let rgba = img.to_rgba8();
    let grey = image::GrayImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let l = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
        let a = a as f64 / 255.0;
        image::Luma([(l * a + 255.0 * (1.0 - a)).round() as u8])
    });
    Ok(Img { png: encode_png(&grey)?, w: grey.width(), h: grey.height() })
}

/// What the laser burns of a stored picture, as `rgb` with alpha = darkness: black is opaque, white is
/// see-through. Painted over the material it shows only what gets burned, with no white box around it.
pub fn burn_mask(png: &[u8], rgb: [u8; 3]) -> Option<image::RgbaImage> {
    let grey = image::load_from_memory(png).ok()?.to_luma8();
    let [r, g, b] = rgb;
    Some(image::RgbaImage::from_fn(grey.width(), grey.height(), |x, y| image::Rgba([r, g, b, 255 - grey.get_pixel(x, y).0[0]])))
}

#[cfg(test)]
mod tests {
    #[test]
    fn burn_mask_makes_white_see_through() {
        let grey = image::GrayImage::from_fn(3, 1, |x, _| image::Luma([[0, 128, 255][x as usize]]));
        let mask = super::burn_mask(&super::encode_png(&grey).unwrap(), [74, 44, 22]).unwrap();
        assert_eq!(mask.get_pixel(0, 0).0, [74, 44, 22, 255], "black burns fully");
        assert_eq!(mask.get_pixel(1, 0).0, [74, 44, 22, 127], "grey burns half");
        assert_eq!(mask.get_pixel(2, 0).0, [74, 44, 22, 0], "white is not burned");
    }

    #[test]
    fn colour_and_transparency_become_grey() {
        let rgba = image::RgbaImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 0, 0]) });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(rgba).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let img = super::to_grey(&png).unwrap();
        let back = image::load_from_memory(&img.png).unwrap();
        assert_eq!(back.color(), image::ColorType::L8);
        let g = back.to_luma8();
        assert_eq!(g.get_pixel(0, 0).0, [76]);
        assert_eq!(g.get_pixel(1, 0).0, [255]);
    }
}
