//! Preview mode: the canvas shows only what the laser makes (see `model::preview`), on the material's
//! colour, with loose pieces shadowed and holes open. Nothing can be edited until it is turned off.

use super::App;
use super::geometry_cache::tessellate;
use crate::model::preview::{Palette, palette, scene};
use crate::model::{Op, Point, Shape};
use eframe::egui::{self, Align2, Color32, FontId, PointerButton, Pos2, Rect, Stroke, vec2};

type Mesh = (Vec<[f32; 2]>, Vec<u32>);

/// The preview's geometry in mm, rebuilt when the document changes or the zoom needs other curves.
#[derive(Default)]
pub(super) struct Preview {
    key: Option<(u64, i32)>,
    /// Fill of each cut-out outline, and whether it is a hole.
    pieces: Vec<(Option<Mesh>, bool)>,
    lines: Vec<(Vec<Point>, Op)>,
    /// Indices of the engraved items and images in the document.
    engraved: Vec<usize>,
}

fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Preview {
    /// Rebuild the meshes when the document changed or the zoom needs curves of another fineness
    /// (`tol` in mm, rounded down to a power of two so small zoom steps reuse them).
    fn update(&mut self, state: &crate::model::State, rev: u64, tol: f64) {
        let bucket = tol.log2().floor() as i32;
        if self.key == Some((rev, bucket)) {
            return;
        }
        let tol = 2f64.powi(bucket);
        let sc = scene(state);
        self.pieces = sc.pieces.iter().map(|(c, hole)| (tessellate(&[c.flatten(tol)]), *hole)).collect();
        self.lines = sc.lines.iter().map(|(c, op)| (c.flatten(tol), *op)).collect();
        self.engraved = state.items.iter().enumerate().filter(|(_, i)| sc.engraved.iter().any(|e| e.id == i.id)).map(|(k, _)| k).collect();
        self.key = Some((rev, bucket));
    }
}

impl App {
    pub(super) fn toggle_preview(&mut self) {
        if self.preview.take().is_none() {
            self.cancel();
            self.preview = Some(Preview::default());
        }
    }

    /// The canvas in preview mode; pan and zoom still work, a click goes back to editing.
    pub(super) fn preview_canvas(&mut self, ui: &egui::Ui, rect: Rect, response: &egui::Response) {
        if response.clicked_by(PointerButton::Primary) {
            self.preview = None;
            return;
        }
        let (state, rev) = {
            let d = self.doc.lock().unwrap();
            (d.s.clone(), d.rev)
        };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(33));
        let p = palette(&state);
        let tol = super::canvas::curve_tol(self.view.zoom, ui.ctx().pixels_per_point());
        let mut preview = self.preview.take().unwrap_or_default();
        preview.update(&state, rev, tol);
        self.paint_preview(ui.ctx(), &painter, &state, &preview, &p, tol);
        self.preview = Some(preview);
        banner(&painter, rect);
    }

    fn paint_preview(&mut self, ctx: &egui::Context, painter: &egui::Painter, s: &crate::model::State, preview: &Preview, p: &Palette, tol: f64) {
        let sheet = Rect::from_min_max(self.w2s([0.0, 0.0]), self.w2s([s.width, s.height]));
        painter.rect_filled(sheet.translate(vec2(3.0, 3.0)), 2.0, Color32::from_black_alpha(60));
        painter.rect_filled(sheet, 0.0, rgb(p.sheet));
        let (origin, zoom) = (self.canvas_rect.min + self.view.offset, self.view.zoom);
        let to_screen = move |q: &crate::model::Point| origin + vec2(q[0] as f32, q[1] as f32) * zoom;
        for (mesh, hole) in &preview.pieces {
            let Some(mesh) = mesh else { continue };
            if *hole {
                fill(painter, mesh, rgb(p.bed), origin, zoom);
            } else {
                fill(painter, mesh, Color32::from_black_alpha(90), origin + vec2(2.0, 2.0), zoom);
                fill(painter, mesh, rgb(p.piece), origin, zoom);
            }
        }
        for item in preview.engraved.iter().filter_map(|k| s.items.get(*k)) {
            if let Shape::Image { image, x, y, w, h, rotation } = &item.shape {
                let Some(img) = s.images.get(image) else { continue };
                let tex = self.texture(ctx, *image, img);
                let r = Rect::from_min_max(self.w2s([*x, *y]), self.w2s([x + w, y + h]));
                let mut mesh = egui::Mesh::with_texture(tex);
                // The tint multiplies: white pixels take the material's colour, black ones stay black.
                mesh.add_rect_with_uv(r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), rgb(p.sheet));
                mesh.rotate(egui::emath::Rot2::from_angle(rotation.to_radians() as f32), r.center());
                painter.add(mesh);
            } else {
                let g = self.geometry.get(item, true, tol);
                if let Some(mesh) = &g.fill {
                    fill(painter, mesh, rgb(p.burn), origin, zoom);
                }
                // Meshes have hard, stair-stepped edges; a hairline along the outline antialiases them.
                let edge = Stroke::new(1.0 / ctx.pixels_per_point(), rgb(p.burn));
                for line in &g.outline {
                    painter.add(egui::Shape::line(line.iter().map(to_screen).collect(), edge));
                }
            }
        }
        for (line, op) in &preview.lines {
            let color = if *op == Op::Cut { p.cut } else { p.score };
            painter.add(egui::Shape::line(line.iter().map(to_screen).collect(), Stroke::new(1.0, rgb(color))));
        }
    }
}

/// A triangle mesh in mm, filled with `color`, with mm 0,0 at `origin` on screen.
fn fill(painter: &egui::Painter, (verts, indices): &Mesh, color: Color32, origin: Pos2, zoom: f32) {
    let vertices = verts.iter().map(|v| egui::epaint::Vertex { pos: origin + vec2(v[0], v[1]) * zoom, uv: egui::epaint::WHITE_UV, color }).collect();
    painter.add(egui::Mesh { indices: indices.clone(), vertices, texture_id: egui::TextureId::default() });
}

fn banner(painter: &egui::Painter, rect: Rect) {
    let pos = rect.center_top() + vec2(0.0, 12.0);
    let galley = painter.layout_no_wrap(format!("Preview — press {} or click to edit", super::shortcuts::preview_key()), FontId::proportional(14.0), Color32::WHITE);
    let r = Align2::CENTER_TOP.anchor_size(pos, galley.size());
    painter.rect_filled(r.expand2(vec2(10.0, 5.0)), 6.0, Color32::from_rgba_unmultiplied(0, 90, 140, 230));
    painter.galley(r.min, galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::canvas::curve_tol;
    use crate::model::{Contour, Document};

    #[test]
    fn zooming_in_gives_finer_curves() {
        let mut d = Document::new(100.0, 100.0);
        d.add("disc", Op::Cut, Shape::path(Contour::circle(50.0, 50.0, 40.0)));
        let verts = |zoom: f32, ppp: f32| {
            let mut p = Preview::default();
            p.update(&d.s, 1, curve_tol(zoom, ppp));
            (p.pieces[0].0.as_ref().unwrap().0.len(), p.lines[0].0.len())
        };
        let (far, near) = (verts(2.0, 1.0), verts(50.0, 1.0));
        assert!(near.0 > far.0 && near.1 > far.1, "{far:?} vs {near:?}");
        assert!(verts(50.0, 2.0).1 > near.1, "a HiDPI screen gets finer curves too");
    }
}
