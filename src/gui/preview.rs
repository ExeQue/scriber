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
        let tol = (0.5 / self.view.zoom as f64).clamp(0.002, 1.0);
        let mut preview = self.preview.take().unwrap_or_default();
        let key = (rev, tol.log2().floor() as i32);
        if preview.key != Some(key) {
            let sc = scene(&state);
            preview.pieces = sc.pieces.iter().map(|(c, hole)| (tessellate(&[c.flatten(tol)]), *hole)).collect();
            preview.lines = sc.lines.iter().map(|(c, op)| (c.flatten(tol), *op)).collect();
            preview.engraved = state.items.iter().enumerate().filter(|(_, i)| sc.engraved.iter().any(|e| e.id == i.id)).map(|(k, _)| k).collect();
            preview.key = Some(key);
        }
        self.paint_preview(ui.ctx(), &painter, &state, &preview, &p, tol);
        self.preview = Some(preview);
        banner(&painter, rect);
    }

    fn paint_preview(&mut self, ctx: &egui::Context, painter: &egui::Painter, s: &crate::model::State, preview: &Preview, p: &Palette, tol: f64) {
        let sheet = Rect::from_min_max(self.w2s([0.0, 0.0]), self.w2s([s.width, s.height]));
        painter.rect_filled(sheet.translate(vec2(3.0, 3.0)), 2.0, Color32::from_black_alpha(60));
        painter.rect_filled(sheet, 0.0, rgb(p.sheet));
        for (mesh, hole) in &preview.pieces {
            let Some(mesh) = mesh else { continue };
            if *hole {
                self.fill(painter, mesh, rgb(p.bed), 0.0);
            } else {
                self.fill(painter, mesh, Color32::from_black_alpha(90), 2.0);
                self.fill(painter, mesh, rgb(p.piece), 0.0);
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
            } else if let Some(fill) = self.geometry.get(item, tol).fill.clone() {
                self.fill(painter, &fill, rgb(p.burn), 0.0);
            }
        }
        for (line, op) in &preview.lines {
            let color = if *op == Op::Cut { p.cut } else { p.score };
            painter.add(egui::Shape::line(line.iter().map(|q| self.w2s(*q)).collect(), Stroke::new(1.0, rgb(color))));
        }
    }

    /// A triangle mesh in mm, filled with `color` and moved `shift` pixels right and down.
    fn fill(&self, painter: &egui::Painter, (verts, indices): &Mesh, color: Color32, shift: f32) {
        let (offset, zoom) = (self.canvas_rect.min + self.view.offset + vec2(shift, shift), self.view.zoom);
        let vertices = verts.iter().map(|v| egui::epaint::Vertex { pos: offset + vec2(v[0], v[1]) * zoom, uv: egui::epaint::WHITE_UV, color }).collect();
        painter.add(egui::Mesh { indices: indices.clone(), vertices, texture_id: egui::TextureId::default() });
    }
}

fn banner(painter: &egui::Painter, rect: Rect) {
    let pos = rect.center_top() + vec2(0.0, 12.0);
    let galley = painter.layout_no_wrap(format!("Preview — press {} or click to edit", super::shortcuts::preview_key()), FontId::proportional(14.0), Color32::WHITE);
    let r = Align2::CENTER_TOP.anchor_size(pos, galley.size());
    painter.rect_filled(r.expand2(vec2(10.0, 5.0)), 6.0, Color32::from_rgba_unmultiplied(0, 90, 140, 230));
    painter.galley(r.min, galley, Color32::WHITE);
}
