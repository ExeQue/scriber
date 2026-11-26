//! Text as real glyph outlines. usvg lays the text out with the system fonts and we take the glyph
//! paths, so the canvas, bounding boxes, SVG export and laser paths all use the same shapes.

use super::geom::{BBox, Contour, Point, rotate_point, union};
use resvg::usvg::{self, Node, tiny_skia_path::PathSegment};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// The font family new text gets; resolved to the system's sans-serif font.
pub const DEFAULT_FONT: &str = "sans-serif";

/// Curves are flattened to lines within this distance (mm).
/// Fine enough that text stays smooth even at the canvas's deepest zoom.
const TOLERANCE: f64 = 0.0025;

/// What a text looks like; everything that changes its outlines except position and rotation.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Style<'a> {
    pub text: &'a str,
    pub font: &'a str,
    /// Font size in mm, as bits so it can be hashed.
    pub size: u64,
    pub bold: bool,
    pub italic: bool,
}

/// Glyph outlines with the baseline starting at (0, 0), unrotated.
pub struct Glyphs {
    pub contours: Vec<Contour>,
    /// Bounding box of the outlines; None when nothing was drawn (empty text or no usable font).
    pub bbox: Option<BBox>,
}

/// SVG options with the system fonts loaded once (loading them is slow). PNG export uses them too.
pub fn usvg_options() -> &'static usvg::Options<'static> {
    static OPT: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPT.get_or_init(|| {
        let mut opt = usvg::Options::default();
        let db = opt.fontdb_mut();
        db.load_system_fonts();
        add_legacy_families(db);
        opt
    })
}

/// fontdb files every face under its typographic family ("Noto Sans"), so weights such as
/// "Noto Sans Light" or "Noto Sans Black" cannot be named. File those faces under their legacy
/// family name too, as fontconfig and other programs' font menus list them.
fn add_legacy_families(db: &mut usvg::fontdb::Database) {
    use skrifa::{FontRef, MetadataProvider, string::StringId};
    use usvg::fontdb::Language;
    let legacy = |data: &[u8], index: u32| {
        let font = FontRef::from_index(data, index).ok()?;
        Some(font.localized_strings(StringId::FAMILY_NAME).english_or_first()?.to_string())
    };
    let extra: Vec<_> = db
        .faces()
        .filter_map(|face| {
            let name = db.with_face_data(face.id, legacy)??;
            let new = !face.families.iter().any(|(n, _)| *n == name);
            new.then(|| usvg::fontdb::FaceInfo { families: vec![(name, Language::English_UnitedStates)], ..face.clone() })
        })
        .collect();
    for face in extra {
        db.push_face_info(face);
    }
}

/// Installed font families, sorted and without duplicates.
pub fn families() -> &'static [String] {
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut names: Vec<String> = usvg_options().fontdb.faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    })
}

/// Why `font` cannot be used (unknown family), with a suggestion when only the case differs.
pub fn font_error(font: &str) -> Option<String> {
    let known = |f: &str| ["serif", "sans-serif", "monospace", "cursive", "fantasy"].contains(&f) || families().iter().any(|n| n == f);
    if known(font) {
        return None;
    }
    let hint = families().iter().find(|n| n.eq_ignore_ascii_case(font)).map_or(String::new(), |n| format!(" Did you mean \"{n}\"?"));
    Some(format!("font \"{font}\" is not installed.{hint} Use list_fonts, or sans-serif, serif or monospace."))
}

/// The outlines for `style`, cached because the canvas asks every frame.
pub fn glyphs(style: &Style) -> Arc<Glyphs> {
    type Key = (String, String, u64, bool, bool);
    static CACHE: OnceLock<Mutex<HashMap<Key, Arc<Glyphs>>>> = OnceLock::new();
    let key = (style.text.to_owned(), style.font.to_owned(), style.size, style.bold, style.italic);
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    if let Some(g) = cache.get(&key) {
        return g.clone();
    }
    // ponytail: the cache is simply emptied when it grows large; an LRU if text editing ever feels slow.
    if cache.len() > 500 {
        cache.clear();
    }
    let g = Arc::new(layout(style));
    cache.insert(key, g.clone());
    g
}

/// Place unrotated outlines with their baseline start at `at`, turned `rotation` degrees clockwise.
pub fn place(contours: &[Contour], at: Point, rotation: f64) -> Vec<Contour> {
    let mut out = contours.to_vec();
    for v in out.iter_mut().flat_map(|c| c.vertices.iter_mut()) {
        [v.x, v.y] = rotate_point([v.x + at[0], v.y + at[1]], at, rotation);
    }
    out
}

fn layout(style: &Style) -> Glyphs {
    let size = f64::from_bits(style.size);
    let mut contours = Vec::new();
    if size > 0.0 && !style.text.trim().is_empty() {
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"><text x=\"0\" y=\"0\" font-size=\"{size}\" font-family=\"{}, sans-serif\" font-weight=\"{}\" font-style=\"{}\" xml:space=\"preserve\">{}</text></svg>",
            family_list(style.font),
            if style.bold { "bold" } else { "normal" },
            if style.italic { "italic" } else { "normal" },
            escape(style.text)
        );
        if let Ok(tree) = usvg::Tree::from_str(&svg, usvg_options()) {
            collect(tree.root(), &mut contours);
        }
    }
    let boxes: Vec<BBox> = contours.iter().map(Contour::bbox).collect();
    Glyphs { bbox: union(&boxes), contours }
}

/// A family name for the CSS font-family list: generic names bare, others quoted.
fn family_list(font: &str) -> String {
    match font {
        "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" => font.to_owned(),
        _ => format!("'{}'", font.replace('\'', "")),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn collect(group: &usvg::Group, out: &mut Vec<Contour>) {
    for node in group.children() {
        match node {
            Node::Group(g) => collect(g, out),
            Node::Text(t) => collect(t.flattened(), out),
            Node::Path(p) => {
                if let Some(data) = p.data().clone().transform(p.abs_transform()) {
                    contours_of(&data, out);
                }
            }
            Node::Image(_) => {}
        }
    }
}

/// Path segments to closed polylines, curves flattened within TOLERANCE.
fn contours_of(path: &usvg::tiny_skia_path::Path, out: &mut Vec<Contour>) {
    let mut pts: Vec<Point> = Vec::new();
    let mut flush = |pts: &mut Vec<Point>| {
        if pts.len() > 2 {
            if pts.first() == pts.last() {
                pts.pop();
            }
            out.push(Contour::polyline(pts, true));
        }
        pts.clear();
    };
    let p = |q: usvg::tiny_skia_path::Point| [q.x as f64, q.y as f64];
    for seg in path.segments() {
        let last = pts.last().copied().unwrap_or_default();
        match seg {
            PathSegment::MoveTo(a) => {
                flush(&mut pts);
                pts.push(p(a));
            }
            PathSegment::LineTo(a) => pts.push(p(a)),
            PathSegment::QuadTo(c, a) => {
                let (c, a) = (p(c), p(a));
                // Wang's formula: segments so the curve is within TOLERANCE of the chords.
                let n = steps(dist2(last, c, a) / (4.0 * TOLERANCE));
                pts.extend((1..=n).map(|k| {
                    let t = k as f64 / n as f64;
                    lerp3(lerp3(last, c, t), lerp3(c, a, t), t)
                }));
            }
            PathSegment::CubicTo(c1, c2, a) => {
                let (c1, c2, a) = (p(c1), p(c2), p(a));
                let n = steps(3.0 * dist2(last, c1, c2).max(dist2(c1, c2, a)) / (4.0 * TOLERANCE));
                pts.extend((1..=n).map(|k| {
                    let t = k as f64 / n as f64;
                    let (ab, bc, cd) = (lerp3(last, c1, t), lerp3(c1, c2, t), lerp3(c2, a, t));
                    lerp3(lerp3(ab, bc, t), lerp3(bc, cd, t), t)
                }));
            }
            PathSegment::Close => flush(&mut pts),
        }
    }
    flush(&mut pts);
}

fn steps(sq: f64) -> usize {
    (sq.sqrt().ceil() as usize).clamp(1, 100)
}

/// |a − 2b + c|: how far a curve bends.
fn dist2(a: Point, b: Point, c: Point) -> f64 {
    (a[0] - 2.0 * b[0] + c[0]).hypot(a[1] - 2.0 * b[1] + c[1])
}

fn lerp3(a: Point, b: Point, t: f64) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(text: &str, font: &str, size: f64) -> Style<'static> {
        let (text, font) = (text.to_owned().leak(), font.to_owned().leak());
        Style { text, font, size: size.to_bits(), bold: false, italic: false }
    }

    #[test]
    fn capital_h_is_about_cap_height_tall() {
        let Some(font) = families().first() else { return eprintln!("no fonts installed; skipped") };
        let g = glyphs(&style("H", font, 10.0));
        let Some(b) = g.bbox else { return eprintln!("{font} has no H; skipped") };
        assert!(!g.contours.is_empty() && g.contours.iter().all(|c| c.closed));
        // Cap height is 0.5–0.9 em; the H sits on the baseline (y = 0) and rises above it.
        assert!((5.0..9.0).contains(&b[3]), "height {}", b[3]);
        assert!((b[1] + b[3]).abs() < 0.2, "bottom at {}", b[1] + b[3]);
    }

    #[test]
    fn fonts_differ_and_empty_text_has_no_outline() {
        assert!(glyphs(&style("  ", "sans-serif", 10.0)).bbox.is_none());
        let fams = families();
        if fams.len() < 2 {
            return;
        }
        let w = |f: &str| glyphs(&style("Hello", f, 10.0)).bbox.map(|b| b[2]);
        let widths: Vec<_> = fams.iter().take(20).filter_map(|f| w(f)).collect();
        assert!(widths.windows(2).any(|p| (p[0] - p[1]).abs() > 0.01), "every font gave the same width");
    }

    #[test]
    fn place_moves_and_rotates() {
        let c = vec![Contour::polyline(&[[0.0, 0.0], [1.0, 0.0], [1.0, -1.0]], true)];
        let p = place(&c, [10.0, 5.0], 90.0);
        let v = &p[0].vertices[1];
        assert!((v.x - 10.0).abs() < 1e-9 && (v.y - 6.0).abs() < 1e-9, "{v:?}");
    }

    #[test]
    fn weights_are_listed_and_render() {
        // A "<family> Light" next to its "<family>" (e.g. Noto Sans Light): listed, and drawn
        // with the light face, not the regular one. Skipped on a system without such a font.
        let all = families();
        let outline = |font: &str| glyphs(&Style { text: "H", font, size: 10f64.to_bits(), bold: false, italic: false }).contours.clone();
        // Latin fonts only: a script font without an H draws a fallback font's H at any weight.
        let candidates = ["Noto Sans Light", "Noto Serif Light", "Source Sans Pro Light", "Open Sans Light", "Roboto Light", "Lato Light"];
        let light_face = |f: &str| usvg_options().fontdb.faces().any(|face| face.families[0].0 == f.trim_end_matches(" Light") && face.weight == usvg::fontdb::Weight::LIGHT);
        let Some(light) = candidates.into_iter().find(|f| light_face(f)) else { return eprintln!("no light Latin font; skipped") };
        assert!(all.iter().any(|n| n == light), "{light} is not listed");
        assert_ne!(outline(light), outline(light.trim_end_matches(" Light")), "{light}");
    }
}
