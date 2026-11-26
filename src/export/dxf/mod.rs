//! DXF out and in, through the `dxf` crate (ASCII and binary, R12 to 2018). DXF counts y upwards;
//! Scriber counts it downwards from the top of the material, so y is flipped around the material's
//! height both ways: the DXF origin is the bottom-left corner of the material, and arcs turn the
//! other way round (bulges change sign).

pub mod import;

use crate::model::{Op, Shape, State};
use dxf::entities::{Circle, Entity, EntityType, LwPolyline};
use dxf::LwPolylineVertex;
use dxf::enums::{AcadVersion, Units};
use dxf::tables::Layer;
use dxf::{Color, Drawing, Point};
use std::collections::HashSet;

/// What `dxf` wrote.
pub struct DxfOut {
    pub bytes: Vec<u8>,
    /// DXF layers with something on them.
    pub layers: usize,
    pub entities: usize,
    /// Items left out because DXF has no place for them (images).
    pub skipped: usize,
}

/// The document as ASCII DXF (AutoCAD 2004, in mm): each shown layer becomes a DXF layer with its
/// name and colour, contours become LWPOLYLINEs with arcs (full circles CIRCLEs), text its glyph
/// outlines. No-output layers only with `include_no_output`; guides, dimensions and images never.
pub fn dxf(s: &State, include_no_output: bool) -> Result<DxfOut, String> {
    let mut d = Drawing::new();
    // 2004 rather than 2000: entities can carry a true colour (group 420) for layer colours
    // that are not in AutoCAD's palette.
    d.header.version = AcadVersion::R2004;
    d.header.default_drawing_units = Units::Millimeters;
    let flip = |x: f64, y: f64| (x, s.height - y);
    let (mut layers, mut entities, mut skipped) = (0, 0, 0);
    let mut names = HashSet::new();
    for layer in s.layers.iter().filter(|l| l.visible && (include_no_output || l.op != Op::NoOutput)) {
        let name = unique(&mut names, &layer_name(&layer.name));
        let rgb = layer.rgb();
        let aci = nearest_aci(rgb);
        let mut n = 0;
        for item in s.items.iter().filter(|i| i.layer == layer.id) {
            match item.shape {
                Shape::Path { .. } | Shape::Text { .. } => {}
                Shape::Image { .. } => {
                    skipped += 1;
                    continue;
                }
                Shape::Guide(_) | Shape::Dimension { .. } => continue,
            }
            for c in item.shape.laser_contours().iter() {
                let specific = match c.as_circle() {
                    Some((centre, r)) => {
                        let (x, y) = flip(centre[0], centre[1]);
                        EntityType::Circle(Circle::new(Point::new(x, y, 0.0), r))
                    }
                    None => {
                        let vertices = c
                            .vertices
                            .iter()
                            .map(|v| {
                                let (x, y) = flip(v.x, v.y);
                                LwPolylineVertex { x, y, bulge: -v.bulge, ..Default::default() }
                            })
                            .collect();
                        EntityType::LwPolyline(LwPolyline { flags: c.closed as i32, vertices, ..Default::default() })
                    }
                };
                let mut e = Entity::new(specific);
                e.common.layer = name.clone();
                if aci_rgb(aci) != rgb {
                    e.common.color_24_bit = i32::from_be_bytes([0, rgb[0], rgb[1], rgb[2]]);
                }
                d.add_entity(e);
                n += 1;
            }
        }
        if n > 0 {
            d.add_layer(Layer { name, color: Color::from_index(aci), ..Default::default() });
            layers += 1;
            entities += n;
        }
    }
    let mut bytes = Vec::new();
    d.save(&mut bytes).map_err(|e| format!("cannot write the DXF: {e}"))?;
    Ok(DxfOut { bytes, layers, entities, skipped })
}

/// A layer name DXF accepts: none of < > / \ " : ; ? * | = , `.
fn layer_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if "<>/\\\":;?*|=,`".contains(c) || c.is_control() { '_' } else { c }).collect();
    if clean.trim().is_empty() { "Layer".into() } else { clean }
}

/// `name`, or `name 2`, `name 3`, … when a layer already has it (DXF names ignore case).
fn unique(taken: &mut HashSet<String>, name: &str) -> String {
    let name = (1..).map(|k| if k == 1 { name.to_string() } else { format!("{name} {k}") }).find(|n| !taken.contains(&n.to_lowercase())).unwrap();
    taken.insert(name.to_lowercase());
    name
}

/// The RGB of an AutoCAD colour index (1–255). 7 is "white", which CAD programs show black on a
/// light background.
pub fn aci_rgb(i: u8) -> [u8; 3] {
    match i {
        1 => [255, 0, 0],
        2 => [255, 255, 0],
        3 => [0, 255, 0],
        4 => [0, 255, 255],
        5 => [0, 0, 255],
        6 => [255, 0, 255],
        0 | 7 => [255, 255, 255],
        8 => [128, 128, 128],
        9 => [192, 192, 192],
        250..=255 => [[51; 3], [91; 3], [132; 3], [173; 3], [214; 3], [255; 3]][(i - 250) as usize],
        _ => {
            // 10–249: 24 hues 15° apart, each in five shades, full and half saturation.
            let k = (i - 10) % 10;
            let hue = ((i - 10) / 10) as f64 * 15.0;
            let v = [1.0, 0.8, 0.6, 0.5, 0.3][k as usize / 2];
            let s = if k % 2 == 0 { 1.0 } else { 0.5 };
            hsv(hue, s, v)
        }
    }
}

fn hsv(h: f64, s: f64, v: f64) -> [u8; 3] {
    let f = |n: f64| {
        let k = (n + h / 60.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)].map(|c| (c * 255.0).round() as u8)
}

/// The AutoCAD colour index nearest to `rgb`; black is 7 (black or white, after the background).
pub fn nearest_aci(rgb: [u8; 3]) -> u8 {
    if rgb == [0, 0, 0] {
        return 7;
    }
    let dist = |c: [u8; 3]| c.iter().zip(rgb).map(|(a, b)| (*a as i32 - b as i32).pow(2)).sum::<i32>();
    (1..=255u8).min_by_key(|i| dist(aci_rgb(*i))).unwrap()
}

#[cfg(test)]
mod tests;
