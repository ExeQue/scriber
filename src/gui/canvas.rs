//! The drawing area: mouse and keyboard input, picking shapes, and painting the document.

use super::labels::op_color;
use super::tools::Tool;
use super::{App, Drag};
use crate::model::geom::{distance, segment_distance, union};
use crate::model::{BBox, Document, Img, Item, Op, Point, Shape, Transform};
use eframe::egui::{self, Align2, Color32, FontId, PointerButton, Pos2, Rect, Sense, Stroke, pos2, vec2};
use std::path::PathBuf;

const SELECTED: Color32 = Color32::from_rgb(255, 150, 0);

impl App {
    pub(super) fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas_rect = rect;
        let ctx = ui.ctx().clone();
        self.fit_if_needed(rect);
        self.pan_and_zoom(ui, &response);

        let raw = response.hover_pos().map(|p| self.s2w(p));
        let pointer = raw.map(|p| self.snap(p));
        if self.tool == Tool::Select {
            self.select_input(ui, &response, raw);
        } else if let Some(p) = pointer {
            self.draw_input(ui, &response, p);
        }

        if !ctx.egui_wants_keyboard_input() {
            self.keys(&ctx);
        }
        self.dropped_files(&ctx, pointer);
        self.paint(ui, rect, pointer);
    }

    /// Show the whole material, once at start and after "Show whole material".
    fn fit_if_needed(&mut self, rect: Rect) {
        if self.view.fitted || rect.width() < 10.0 {
            return;
        }
        let (w, h) = {
            let d = self.doc.lock().unwrap();
            (d.s.width as f32, d.s.height as f32)
        };
        let z = ((rect.width() - 60.0) / w).min((rect.height() - 60.0) / h).max(0.05);
        self.view.zoom = z;
        self.view.offset = (rect.size() - vec2(w, h) * z) / 2.0;
        self.view.fitted = true;
    }

    /// Middle/right drag pans; the wheel (or pinch) zooms around the pointer.
    fn pan_and_zoom(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if response.dragged_by(PointerButton::Middle) || response.dragged_by(PointerButton::Secondary) {
            self.view.offset += response.drag_delta();
        }
        let Some(hover) = response.hover_pos() else { return };
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        let factor = pinch * (scroll.y * 0.0025).exp();
        if (factor - 1.0).abs() > 1e-4 {
            let before = self.s2w(hover);
            self.view.zoom = (self.view.zoom * factor).clamp(0.05, 400.0);
            self.view.offset += hover - self.w2s(before);
        }
    }

    /// Drawing tools: click point by point, or press-drag-release for two-point shapes.
    fn draw_input(&mut self, ui: &egui::Ui, response: &egui::Response, p: Point) {
        let tool = self.tool;
        if !tool.clicks_only() {
            if response.drag_started_by(PointerButton::Primary)
                && self.draft.is_empty()
                && let Some(o) = ui.input(|i| i.pointer.press_origin())
            {
                self.draft.push(self.snap(self.s2w(o)));
            }
            if response.drag_stopped_by(PointerButton::Primary) && self.draft.len() == 1 && self.px(distance(self.draft[0], p)) > 4.0 {
                self.draft.push(p);
                self.finish_draft(false);
            }
        }
        if response.double_clicked_by(PointerButton::Primary) && tool == Tool::Polyline {
            // The double click's first click already placed this point.
            self.finish_draft(false);
        } else if response.clicked_by(PointerButton::Primary) {
            self.place_point(p);
        }
    }

    /// Select tool: click to pick, drag a shape to move it, drag on empty space to box-select.
    fn select_input(&mut self, ui: &egui::Ui, response: &egui::Response, raw: Option<Point>) {
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let origin = self.s2w(o);
            let mut d = self.doc.lock().unwrap();
            self.drag = Some(match self.hit(&d, origin) {
                Some(id) => {
                    if !d.selection.contains(&id) {
                        if !add {
                            d.selection.clear();
                        }
                        d.selection.push(id);
                    }
                    d.checkpoint();
                    Drag::Move { origin, applied: [0.0, 0.0] }
                }
                None => Drag::Box { start: origin },
            });
        }
        if let (Some(Drag::Move { origin, applied }), Some(p)) = (&mut self.drag, raw) {
            let mut want = [p[0] - origin[0], p[1] - origin[1]];
            if self.snap {
                want = [want[0].round(), want[1].round()];
            }
            let (dx, dy) = (want[0] - applied[0], want[1] - applied[1]);
            if dx != 0.0 || dy != 0.0 {
                *applied = want;
                let mut d = self.doc.lock().unwrap();
                let ids = d.selection.clone();
                d.transform(&ids, &Transform::translate(dx, dy));
            }
        }
        if response.drag_stopped_by(PointerButton::Primary) {
            if let (Some(Drag::Box { start }), Some(p)) = (&self.drag, raw) {
                let area = [start[0].min(p[0]), start[1].min(p[1]), (p[0] - start[0]).abs(), (p[1] - start[1]).abs()];
                let mut d = self.doc.lock().unwrap();
                let inside: Vec<u64> = d.s.items.iter().filter(|i| contains_box(area, i.shape.metrics().bbox)).map(|i| i.id).collect();
                if !add {
                    d.selection.clear();
                }
                for id in inside {
                    if !d.selection.contains(&id) {
                        d.selection.push(id);
                    }
                }
            }
            self.drag = None;
        }
        if response.clicked_by(PointerButton::Primary)
            && let Some(p) = raw
        {
            let mut d = self.doc.lock().unwrap();
            match (self.hit(&d, p), add) {
                (Some(id), true) => match d.selection.iter().position(|s| *s == id) {
                    Some(i) => _ = d.selection.remove(i),
                    None => d.selection.push(id),
                },
                (Some(id), false) => d.selection = vec![id],
                (None, false) => d.selection.clear(),
                (None, true) => {}
            }
        }
    }

    /// Images are imported where they are dropped; .scriber files are opened.
    fn dropped_files(&mut self, ctx: &egui::Context, pointer: Option<Point>) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        for path in dropped {
            if path.extension().is_some_and(|e| e == "scriber") {
                self.open_path(&path);
            } else {
                self.import_image(&path, pointer.unwrap_or([10.0, 10.0]));
            }
        }
    }

    /// Millimetres on screen, in pixels.
    fn px(&self, mm: f64) -> f32 {
        mm as f32 * self.view.zoom
    }

    /// The item under `p`: a line within a few pixels wins; otherwise the smallest area containing `p`.
    fn hit(&self, d: &Document, p: Point) -> Option<u64> {
        let tol = 6.0 / self.view.zoom as f64;
        let mut inside: Option<(f64, u64)> = None;
        for item in d.s.items.iter().rev() {
            let near_line = item.shape.outline(0.05).iter().any(|line| line.windows(2).any(|s| segment_distance(p, s[0], s[1]) <= tol));
            if near_line {
                return Some(item.id);
            }
            if item.shape.contains(p) {
                let area = item.shape.metrics().area.unwrap_or(f64::MAX);
                if inside.is_none_or(|(a, _)| area < a) {
                    inside = Some((area, item.id));
                }
            }
        }
        inside.map(|(_, id)| id)
    }

    fn texture(&mut self, ctx: &egui::Context, id: u64, img: &Img) -> egui::TextureId {
        self.textures
            .entry(id)
            .or_insert_with(|| {
                let grey = image::load_from_memory(&img.png).map(|i| i.to_luma8()).unwrap_or_default();
                let size = [grey.width() as usize, grey.height() as usize];
                ctx.load_texture(format!("img{id}"), egui::ColorImage::from_gray(size, grey.as_raw()), egui::TextureOptions::LINEAR)
            })
            .id()
    }

    // ---------- painting ----------

    fn paint(&mut self, ui: &egui::Ui, rect: Rect, pointer: Option<Point>) {
        let painter = ui.painter_at(rect);
        let background = if ui.visuals().dark_mode { Color32::from_gray(38) } else { Color32::from_gray(200) };
        painter.rect_filled(rect, 0.0, background);

        let (state, selection) = {
            let d = self.doc.lock().unwrap();
            (d.s.clone(), d.selection.clone())
        };
        self.paint_material(&painter, state.width, state.height);
        let tol = (0.5 / self.view.zoom as f64).clamp(0.002, 1.0);
        for item in &state.items {
            let selected = selection.contains(&item.id);
            self.paint_item(ui.ctx(), &painter, &state.images, item, selected, tol);
        }
        self.paint_selection(&painter, &state.items, &selection, pointer);
        self.paint_draft(&painter, pointer, tol);
    }

    fn paint_material(&self, painter: &egui::Painter, w: f64, h: f64) {
        let mat = Rect::from_min_max(self.w2s([0.0, 0.0]), self.w2s([w, h]));
        painter.rect_filled(mat.translate(vec2(3.0, 3.0)), 2.0, Color32::from_black_alpha(60));
        painter.rect_filled(mat, 0.0, Color32::WHITE);
        // 1 mm and 10 mm grid lines, hidden when they would be too dense.
        for (step, color) in [(1.0, Color32::from_gray(242)), (10.0, Color32::from_gray(222))] {
            if self.px(step) < 6.0 {
                continue;
            }
            let stroke = Stroke::new(1.0, color);
            for i in 0..=(w / step) as usize {
                let x = i as f64 * step;
                painter.line_segment([self.w2s([x, 0.0]), self.w2s([x, h])], stroke);
            }
            for i in 0..=(h / step) as usize {
                let y = i as f64 * step;
                painter.line_segment([self.w2s([0.0, y]), self.w2s([w, y])], stroke);
            }
        }
    }

    fn paint_item(&mut self, ctx: &egui::Context, painter: &egui::Painter, images: &std::collections::BTreeMap<u64, std::sync::Arc<Img>>, item: &Item, selected: bool, tol: f64) {
        let color = op_color(item.op);
        let screen = |line: &Vec<Point>| line.iter().map(|p| self.w2s(*p)).collect::<Vec<Pos2>>();
        if selected {
            for line in item.shape.outline(tol) {
                painter.add(egui::Shape::line(screen(&line), Stroke::new(6.0, SELECTED.gamma_multiply(0.45))));
            }
        }
        match &item.shape {
            Shape::Path { .. } => {
                let outline = item.shape.outline(tol);
                if item.fill {
                    let fill = if item.op == Op::Annotate { op_color(Op::Annotate).gamma_multiply(0.25) } else { Color32::from_gray(60) };
                    fill_even_odd(painter, &outline.iter().map(screen).collect::<Vec<_>>(), fill);
                }
                let stroke = if item.op == Op::NoLine { Stroke::new(1.0, color.gamma_multiply(0.6)) } else { Stroke::new(1.5, color) };
                for line in &outline {
                    painter.add(egui::Shape::line(screen(line), stroke));
                }
            }
            Shape::Image { image, x, y, w, h, rotation } => {
                let Some(img) = images.get(image) else { return };
                let tex = self.texture(ctx, *image, img);
                let r = Rect::from_min_max(self.w2s([*x, *y]), self.w2s([x + w, y + h]));
                let mut mesh = egui::Mesh::with_texture(tex);
                mesh.add_rect_with_uv(r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                mesh.rotate(egui::emath::Rot2::from_angle(rotation.to_radians() as f32), r.center());
                painter.add(mesh);
            }
            Shape::Text { x, y, size, text, rotation } => {
                let px = self.px(*size).max(1.0);
                let ink = if item.fill && item.op != Op::Annotate { Color32::from_gray(30) } else { color };
                let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(px), ink);
                let angle = rotation.to_radians() as f32;
                // (x, y) is the baseline start; egui places text by its top-left corner.
                let top_left = self.w2s([*x, *y]) + egui::emath::Rot2::from_angle(angle) * vec2(0.0, -px * 0.8);
                painter.add(egui::epaint::TextShape::new(top_left, galley, ink).with_angle(angle));
            }
            Shape::Dimension { .. } => self.paint_dimension(painter, &item.shape, color),
        }
    }

    fn paint_dimension(&self, painter: &egui::Painter, shape: &Shape, color: Color32) {
        let Some(g) = shape.dimension() else { return };
        for (a, b) in [(g.a, g.a2), (g.b, g.b2), (g.a2, g.b2)] {
            painter.line_segment([self.w2s(a), self.w2s(b)], Stroke::new(1.0, color));
        }
        arrow_head(painter, self.w2s(g.a2), self.w2s(g.b2), color);
        arrow_head(painter, self.w2s(g.b2), self.w2s(g.a2), color);
        // Keep the label upright.
        let mut angle = g.angle as f32;
        if !(-90.0..=90.0).contains(&angle) {
            angle += 180.0;
        }
        let galley = painter.layout_no_wrap(format!("{:.2} mm", g.length), FontId::proportional(13.0), color);
        let rot = egui::emath::Rot2::from_angle(angle.to_radians());
        let pos = self.w2s(g.mid) - rot * vec2(galley.size().x / 2.0, galley.size().y + 2.0);
        painter.add(egui::epaint::TextShape::new(pos, galley, color).with_angle(angle.to_radians()));
    }

    fn paint_selection(&self, painter: &egui::Painter, items: &[Item], selection: &[u64], pointer: Option<Point>) {
        let boxes: Vec<BBox> = items.iter().filter(|i| selection.contains(&i.id)).map(|i| i.shape.metrics().bbox).collect();
        if let Some(b) = union(&boxes) {
            let r = Rect::from_min_max(self.w2s([b[0], b[1]]), self.w2s([b[0] + b[2], b[1] + b[3]])).expand(4.0);
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, SELECTED), egui::StrokeKind::Outside);
        }
        if let (Some(Drag::Box { start }), Some(p)) = (&self.drag, pointer) {
            let blue = Color32::from_rgb(80, 140, 255);
            painter.rect(Rect::from_two_pos(self.w2s(*start), self.w2s(p)), 0.0, blue.gamma_multiply(0.12), Stroke::new(1.0, blue), egui::StrokeKind::Inside);
        }
    }

    /// The shape being drawn, with its live size, plus the measuring line.
    fn paint_draft(&self, painter: &egui::Painter, pointer: Option<Point>, tol: f64) {
        if let Some((a, b)) = self.measured {
            measure_overlay(painter, self.w2s(a), self.w2s(b), a, b);
        }
        let Some(p) = pointer else { return };
        if self.tool == Tool::Select {
            return;
        }
        painter.circle_stroke(self.w2s(p), 4.0, Stroke::new(1.5, Color32::from_rgb(255, 120, 0)));
        if self.draft.is_empty() {
            return;
        }
        if self.tool == Tool::Measure {
            measure_overlay(painter, self.w2s(self.draft[0]), self.w2s(p), self.draft[0], p);
            return;
        }
        let mut pts = self.draft.clone();
        pts.push(p);
        let Some((_, shape)) = self.tool.shape(&pts, self.sides) else { return };
        let op = if self.tool == Tool::Dimension { Op::Annotate } else { self.new_op };
        let stroke = Stroke::new(1.5, op_color(op).gamma_multiply(0.7));
        for line in shape.outline(tol) {
            let pts: Vec<Pos2> = line.iter().map(|q| self.w2s(*q)).collect();
            painter.extend(egui::Shape::dashed_line(&pts, stroke, 6.0, 4.0));
        }
        let m = shape.metrics();
        let label = match self.tool {
            Tool::Rect => format!("{:.1} × {:.1} mm", m.bbox[2], m.bbox[3]),
            Tool::Circle => format!("Ø {:.1} mm", m.bbox[2]),
            _ => format!("{:.1} mm", m.length),
        };
        painter.text(self.w2s(p) + vec2(14.0, 14.0), Align2::LEFT_TOP, label, FontId::proportional(13.0), Color32::from_gray(30));
    }
}

fn contains_box(outer: BBox, inner: BBox) -> bool {
    inner[0] >= outer[0] && inner[1] >= outer[1] && inner[0] + inner[2] <= outer[0] + outer[2] && inner[1] + inner[3] <= outer[1] + outer[3]
}

/// Fill polygons with the even-odd rule using horizontal spans, so holes stay empty.
/// egui only fills convex shapes itself.
fn fill_even_odd(painter: &egui::Painter, polys: &[Vec<Pos2>], color: Color32) {
    let clip = painter.clip_rect();
    let (mut top, mut bottom) = (f32::MAX, f32::MIN);
    for p in polys.iter().flatten() {
        top = top.min(p.y);
        bottom = bottom.max(p.y);
    }
    let (top, bottom) = (top.max(clip.top()), bottom.min(clip.bottom()));
    let mut y = top.floor() + 0.5;
    let mut xs = Vec::new();
    while y < bottom {
        xs.clear();
        for poly in polys {
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(f32::total_cmp);
        for span in xs.as_chunks::<2>().0 {
            painter.line_segment([pos2(span[0], y), pos2(span[1], y)], Stroke::new(1.0, color));
        }
        y += 1.0;
    }
}

fn measure_overlay(painter: &egui::Painter, a: Pos2, b: Pos2, wa: Point, wb: Point) {
    let c = Color32::from_rgb(230, 0, 160);
    painter.line_segment([a, b], Stroke::new(1.5, c));
    painter.circle_filled(a, 3.0, c);
    painter.circle_filled(b, 3.0, c);
    let (dx, dy) = (wb[0] - wa[0], wb[1] - wa[1]);
    let text = format!("{:.2} mm   Δx {:.2}  Δy {:.2}   {:.1}°", dx.hypot(dy), dx, dy, (-dy).atan2(dx).to_degrees());
    let pos = b + vec2(12.0, -22.0);
    let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), Color32::WHITE);
    painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(4.0), 4.0, c);
    painter.galley(pos, galley, Color32::WHITE);
}

fn arrow_head(painter: &egui::Painter, tip: Pos2, from: Pos2, color: Color32) {
    let dir = (tip - from).normalized() * 7.0;
    let side = vec2(-dir.y, dir.x) * 0.4;
    painter.add(egui::Shape::convex_polygon(vec![tip, tip - dir + side, tip - dir - side], color, Stroke::NONE));
}
