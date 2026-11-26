//! The drawing area: input for the drawing tools, the drafting aids (OSNAP, ORTHO, POLAR, grid)
//! and painting the document. Selection and grip dragging live in select.rs.

use super::App;
use super::labels::op_color;
use super::snap::{self, Aid};
use super::tracking;
use crate::model::guides::guide_segments;

use super::tools::Tool;
use crate::model::geom::{distance, segment_distance};
use crate::model::{Document, Img, Item, Op, Point, Shape};
use eframe::egui::{self, Align2, Color32, FontId, Key, PointerButton, Pos2, Rect, Sense, Stroke, pos2, vec2};
use std::path::PathBuf;

/// Half the length of the crosshair arms, in pixels.
const CROSSHAIR: f32 = 20.0;
const SELECTED: Color32 = Color32::from_rgb(255, 150, 0);
/// Half the size of the pick box, in pixels; also how close a click must be to a line.
const PICKBOX: f32 = 5.0;
/// How close (pixels) the pointer must be for an object snap, polar ray or grip.
pub(super) const APERTURE: f32 = 10.0;

impl App {
    pub(super) fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas_rect = rect;
        let ctx = ui.ctx().clone();
        self.zoom_extents_on_wheel_double_click(&response);
        if self.preview.is_some() {
            self.fit_if_needed(rect);
            self.pan_and_zoom(ui, &response);
            self.settle();
            return self.preview_canvas(ui, rect, &response);
        }
        if response.secondary_clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            self.right_click(at);
        }
        response.context_menu(|ui| self.canvas_menu(ui));
        self.fit_if_needed(rect);
        self.pan_and_zoom(ui, &response);
        let space_pan = self.space_pan(ui, &response);
        if response.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }

        self.aids.hit = None;
        self.aids.tracking = None;
        self.shift = ui.input(|i| i.modifiers.shift);
        let raw = response.hover_pos().map(|p| self.s2w(p));
        let pointer = if space_pan {
            raw
        } else if self.tool == Tool::Select {
            self.select_input(ui, &response, raw)
        } else {
            let from = self.draft.last().copied();
            let pointer = raw.map(|r| self.aim_at_doc(r, from));
            if let Some(p) = pointer {
                self.draw_input(ui, &response, p);
            }
            pointer
        };
        self.pointer_mm = pointer;
        // Keep drawing while the pointer rests: the acquire ring fills and the point is taken in time.
        if self.tracker.waiting() {
            ctx.request_repaint();
        }
        self.dropped_files(&ctx, pointer);
        self.dimension_input(&response);
        self.settle();
        self.paint(ui, rect, pointer);
    }

    // ---------- drafting aids ----------

    pub(super) fn aid_on(&self, a: Aid) -> bool {
        match a {
            Aid::Grid => self.aids.grid,
            Aid::Snap => self.snap,
            Aid::Ortho => self.aids.ortho,
            Aid::Osnap => self.aids.osnap,
            Aid::Polar => self.aids.polar,
            Aid::Otrack => self.aids.otrack,
            Aid::Infer => self.aids.infer,
        }
    }

    /// Flip a status-bar toggle. ORTHO and POLAR exclude each other, as in AutoCAD.
    pub(super) fn toggle(&mut self, a: Aid) {
        let on = !self.aid_on(a);
        match a {
            Aid::Grid => self.aids.grid = on,
            Aid::Snap => self.snap = on,
            Aid::Osnap => self.aids.osnap = on,
            Aid::Infer => self.aids.infer = on,
            Aid::Otrack => self.aids.otrack = on,
            Aid::Ortho => {
                self.aids.ortho = on;
                self.aids.polar &= !on;
            }
            Aid::Polar => {
                self.aids.polar = on;
                self.aids.ortho &= !on;
            }
        }
        self.message = format!("<{} {}>", a.label(), if on { "on" } else { "off" });
    }

    /// F3/F7/F8/F9/F10 toggle the aids (also while the command line has focus); Esc cancels a window or grip drag.
    /// Runs before `App::keys`, which consumes Esc.
    pub(super) fn aid_keys(&mut self, ctx: &egui::Context) {
        for a in Aid::ALL {
            if let Some(k) = a.key()
                && ctx.input(|i| i.key_pressed(k))
            {
                self.toggle(a);
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.cancel_drag();
        }
    }

    /// Where a point lands: a one-shot snap override when one waits; else an object snap, a
    /// tracking path (POLAR, OTRACK, Extension, Parallel) or the nearest point on a shape; else the
    /// grid, with ORTHO from `from`.
    pub(super) fn aim(&mut self, items: &[Item], raw: Point, from: Option<Point>) -> Point {
        self.aim_at_time(items, raw, from, std::time::Instant::now())
    }

    /// `aim` at time `now` (OTRACK acquires what the pointer rested on long enough).
    pub(super) fn aim_at_time(&mut self, items: &[Item], raw: Point, from: Option<Point>, now: std::time::Instant) -> Point {
        let p = self.aim_point(items, raw, from, now);
        self.override_point(p)
    }

    fn aim_point(&mut self, items: &[Item], raw: Point, from: Option<Point>, now: std::time::Instant) -> Point {
        let tol = (APERTURE / self.view.zoom) as f64;
        let guides = self.guides_in_view(items);
        if let Some(kind) = self.snap_once() {
            self.aids.hit = snap::find_only(items, &guides, raw, tol, from, kind);
            return self.aids.hit.map_or(raw, |s| s.p);
        }
        let hit = if self.aids.osnap { snap::find(items, &guides, raw, tol, from) } else { None };
        self.acquire(items, &guides, hit, now);

        if let Some(s) = hit.filter(|s| s.kind != snap::Kind::Nearest) {
            self.aids.hit = Some(s);
            return s.p;
        }
        let ortho = self.ortho_now();
        let angles = self.aids.polar_angles();
        let polar = (self.aids.polar && !ortho).then_some(angles.as_slice());
        let tracks = self.tracker.tracks(from, polar, ortho, self.aids.otrack, self.aids.osnap);
        if let Some(l) = tracking::lock(&tracks, raw, tol) {
            let p = l.p;
            self.aids.tracking = Some(l);
            return p;
        }
        if let Some(s) = hit {
            self.aids.hit = Some(s);
            return s.p;
        }
        let p = self.snap(raw);
        match from {
            Some(f) if ortho => snap::ortho(f, p),
            _ => p,
        }
    }

    /// The shown guide lines, cut to a box three times the view (so the cut ends are off screen).
    pub(super) fn guides_in_view(&self, items: &[Item]) -> Vec<[Point; 2]> {
        if !self.show_guides {
            return Vec::new();
        }
        let v = self.view_box();
        guide_segments(items.iter().filter(|i| self.visible(i)), [v[0] - v[2], v[1] - v[3], 3.0 * v[2], 3.0 * v[3]])
    }

    fn aim_at_doc(&mut self, raw: Point, from: Option<Point>) -> Point {
        let doc = self.doc.clone();
        let d = doc.lock().unwrap();
        self.aim(&d.s.items, raw, from)
    }

    /// Show the whole material, once at start and after "Show whole material".
    fn fit_if_needed(&mut self, rect: Rect) {
        // On the first frames the panels are still settling; wait for a real canvas size.
        if !self.view.auto_fit || rect.size() == self.view.fit_size || rect.width() < 100.0 || rect.height() < 100.0 {
            return;
        }
        let (w, h) = {
            let d = self.doc.lock().unwrap();
            (d.s.width as f32, d.s.height as f32)
        };
        let z = ((rect.width() - 60.0) / w).min((rect.height() - 60.0) / h).max(0.05);
        self.view.zoom = z;
        self.view.offset = (rect.size() - vec2(w, h) * z) / 2.0;
        self.view.fit_size = rect.size();
    }

    /// Double-click with the wheel: show the whole material (AutoCAD's zoom extents).
    fn zoom_extents_on_wheel_double_click(&mut self, response: &egui::Response) {
        if response.double_clicked_by(PointerButton::Middle) {
            self.view.zoom_extents();
        }
    }

    /// Middle drag pans, and so does a right drag (egui only reports a drag once the pointer moved,
    /// so a plain right click stays free); the wheel (or pinch) zooms around the pointer.
    fn pan_and_zoom(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if response.drag_started_by(PointerButton::Middle) || response.drag_started_by(PointerButton::Secondary) {
            self.view.remember();
        }
        if response.dragged_by(PointerButton::Middle) || response.dragged_by(PointerButton::Secondary) {
            self.view.offset += response.drag_delta();
            self.view.auto_fit = false;
        }
        let Some(hover) = response.hover_pos() else { return };
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        let factor = pinch * (scroll.y * 0.0025).exp();
        let wheeling = (factor - 1.0).abs() > 1e-4;
        if wheeling && !std::mem::replace(&mut self.view.wheeling, true) {
            self.view.remember();
        }
        self.view.wheeling = wheeling;
        if wheeling {
            let before = self.s2w(hover);
            self.view.zoom = (self.view.zoom * factor).clamp(0.05, 400.0);
            self.view.offset += hover - self.w2s(before);
            self.view.auto_fit = false;
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
                let start = self.aim_at_doc(self.s2w(o), None);
                self.note_snap(start);
                self.draft.push(start);
            }
            if response.drag_stopped_by(PointerButton::Primary) && self.draft.len() == 1 && self.px(distance(self.draft[0], p)) > 4.0 {
                self.place_point(p);
            }
        }
        if response.double_clicked_by(PointerButton::Primary) && tool == Tool::Polyline {
            // The double click's first click already placed this point.
            self.finish_draft(false);
        } else if response.clicked_by(PointerButton::Primary) {
            self.place_point(p);
        }
    }

    /// Images and SVG drawings are imported where they are dropped; .scrbr files are opened.
    fn dropped_files(&mut self, ctx: &egui::Context, pointer: Option<Point>) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        for path in dropped {
            if path.extension().is_some_and(|e| e == "scrbr") {
                self.open_path(&path);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz")) {
                self.import_svg_at(&path, pointer.unwrap_or([10.0, 10.0]));
            } else {
                self.import_image(&path, pointer.unwrap_or([10.0, 10.0]));
            }
        }
    }

    /// Millimetres on screen, in pixels.
    pub(super) fn px(&self, mm: f64) -> f32 {
        mm as f32 * self.view.zoom
    }

    /// The item a click at `p` picks: the candidate chosen with Shift+Space, else the front one.
    pub(super) fn hit(&self, d: &Document, p: Point) -> Option<u64> {
        self.cycle.pick(&self.hits(d, p))
    }

    /// The items under `p` in the order a click picks them: lines within a few pixels (front
    /// first), then the areas containing `p`, smallest first.
    pub(super) fn hits(&self, d: &Document, p: Point) -> Vec<u64> {
        let tol = (PICKBOX / self.view.zoom) as f64;
        let (mut lines, mut inside) = (Vec::new(), Vec::new());
        for item in d.s.items.iter().rev().filter(|i| self.visible(i)) {
            if item.shape.outline(0.05).iter().any(|line| line.windows(2).any(|s| segment_distance(p, s[0], s[1]) <= tol)) {
                lines.push(item.id);
            } else if item.shape.contains(p) {
                inside.push((item.shape.metrics().area.unwrap_or(f64::MAX), item.id));
            }
        }
        inside.sort_by(|a, b| a.0.total_cmp(&b.0));
        lines.extend(inside.into_iter().map(|(_, id)| id));
        lines
    }

    pub(super) fn texture(&mut self, ctx: &egui::Context, id: u64, img: &Img) -> egui::TextureId {
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
        // Model space: always dark around the light material.
        painter.rect_filled(rect, 0.0, Color32::from_gray(33));

        let (state, selection) = {
            let d = self.doc.lock().unwrap();
            (d.s.clone(), d.selection.clone())
        };
        self.paint_material(&painter, state.width, state.height);
        let tol = (0.5 / self.view.zoom as f64).clamp(0.002, 1.0);
        let ids: Vec<u64> = state.items.iter().map(|i| i.id).collect();
        self.geometry.retain(&ids);
        for item in &state.items {
            if !self.visible(item) {
                continue;
            }
            let selected = selection.contains(&item.id);
            if let Shape::Guide(g) = &item.shape {
                self.paint_guide(&painter, g, selected);
                continue;
            }
            self.paint_item(ui.ctx(), &painter, &state.images, item, selected, tol);
        }
        self.paint_dimensions(&painter, &state);
        self.paint_bars(&painter, &state);
        self.paint_selection(&painter, &state.items, &selection, pointer);
        self.paint_hover(&painter, &state, pointer);
        self.paint_hover_measure(&painter, &state.items, pointer);
        self.paint_draft(&painter, pointer, tol);
        self.paint_aids(&painter, rect, pointer);
    }

    /// Tracking ray, snap marker with its name, and the crosshair with the pick box.
    fn paint_aids(&self, painter: &egui::Painter, rect: Rect, pointer: Option<Point>) {
        let Some(p) = pointer else { return };
        let at = self.w2s(p);
        self.paint_tracking(painter, rect);
        self.paint_override(painter, p);
        if let Some(s) = self.aids.hit {
            snap::paint_marker(painter, at, s.kind);
            tooltip(painter, at, s.kind.label());
        }
        // A short crosshair, like AutoCAD's default size, rather than lines across the whole canvas.
        let cross = Stroke::new(1.0, Color32::from_gray(150));
        painter.line_segment([at - vec2(CROSSHAIR, 0.0), at + vec2(CROSSHAIR, 0.0)], cross);
        painter.line_segment([at - vec2(0.0, CROSSHAIR), at + vec2(0.0, CROSSHAIR)], cross);
        if self.tool == Tool::Select {
            let pick = Rect::from_center_size(at, vec2(PICKBOX * 2.0, PICKBOX * 2.0));
            painter.rect_filled(pick, 0.0, Color32::from_gray(33));
            painter.rect_stroke(pick, 0.0, Stroke::new(1.0, Color32::from_gray(220)), egui::StrokeKind::Middle);
        }
    }

    fn paint_material(&self, painter: &egui::Painter, w: f64, h: f64) {
        let mat = Rect::from_min_max(self.w2s([0.0, 0.0]), self.w2s([w, h]));
        painter.rect_filled(mat.translate(vec2(3.0, 3.0)), 2.0, Color32::from_black_alpha(60));
        painter.rect_filled(mat, 0.0, Color32::WHITE);
        // 1 mm and 10 mm grid lines (GRID, F7), hidden when they would be too dense.
        for (step, color) in [(1.0, Color32::from_gray(242)), (10.0, Color32::from_gray(222))] {
            if !self.aids.grid || self.px(step) < 6.0 {
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
        let op = item.effective_op();
        let color = op_color(op);
        let (offset, zoom) = (self.canvas_rect.min + self.view.offset, self.view.zoom);
        let to_screen = move |p: &Point| offset + vec2(p[0] as f32, p[1] as f32) * zoom;
        let geometry = self.geometry.get(item, tol);
        if selected {
            for line in &geometry.outline {
                painter.add(egui::Shape::line(line.iter().map(to_screen).collect(), Stroke::new(6.0, SELECTED.gamma_multiply(0.45))));
            }
        }
        match &item.shape {
            // Text is drawn from its glyph outlines, the same shapes the laser gets.
            Shape::Path { .. } | Shape::Text { .. } => {
                if let Some((verts, indices)) = &geometry.fill {
                    let fill = Color32::from_gray(60);
                    let vertices = verts.iter().map(|v| egui::epaint::Vertex { pos: offset + vec2(v[0], v[1]) * zoom, uv: egui::epaint::WHITE_UV, color: fill }).collect();
                    painter.add(egui::Mesh { indices: indices.clone(), vertices, texture_id: egui::TextureId::default() });
                }
                for line in &geometry.outline {
                    let pts: Vec<Pos2> = line.iter().map(to_screen).collect();
                    if op == Op::NoOutput {
                        painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, color), 4.0, 3.0));
                    } else {
                        painter.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
                    }
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
            Shape::Dimension { .. } => self.paint_dimension(painter, &item.shape, color),
            Shape::Guide(_) => {}
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

    /// The shape being drawn, with its live size, plus the measuring line.
    fn paint_draft(&self, painter: &egui::Painter, pointer: Option<Point>, tol: f64) {
        if let Some((a, b)) = self.measured {
            measure_overlay(painter, self.w2s(a), self.w2s(b), a, b);
        }
        let Some(p) = pointer else { return };
        // FILLET/CHAMFER: what a click here would do.
        let preview = Stroke::new(2.0, Color32::from_rgb(255, 150, 0));
        for shape in self.op_preview(p) {
            for line in shape.outline(tol) {
                let pts: Vec<Pos2> = line.iter().map(|q| self.w2s(*q)).collect();
                painter.extend(egui::Shape::dashed_line(&pts, preview, 6.0, 3.0));
            }
        }
        if self.tool == Tool::Select {
            return;
        }
        if self.draft.is_empty() {
            return;
        }
        if self.tool == Tool::Measure {
            measure_overlay(painter, self.w2s(self.draft[0]), self.w2s(p), self.draft[0], p);
            return;
        }
        // Typed measurements on the command card win over the mouse.
        let p = self.dyn_point(p).unwrap_or(p);
        let mut pts = self.draft.clone();
        pts.push(p);
        let Some((_, shape)) = self.draft_shape(&pts) else { return };
        let op = if self.tool == Tool::Dimension { Op::NoOutput } else { self.new_op };
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
        // Dynamic input shows the same measurements next to the cursor; avoid two boxes on top of each other.
        if !self.has_dyn_fields() {
    painter.text(self.w2s(p) + vec2(14.0, 14.0), Align2::LEFT_TOP, label, FontId::proportional(13.0), Color32::from_gray(30));
        }
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

pub(super) fn arrow_head(painter: &egui::Painter, tip: Pos2, from: Pos2, color: Color32) {
    let dir = (tip - from).normalized() * 7.0;
    let side = vec2(-dir.y, dir.x) * 0.4;
    painter.add(egui::Shape::convex_polygon(vec![tip, tip - dir + side, tip - dir - side], color, Stroke::NONE));
}

/// A small dark label next to the cursor.
pub(super) fn tooltip(painter: &egui::Painter, at: Pos2, text: &str) {
    // Above-right of the cursor: dynamic input's fields sit below-right.
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(12.0), Color32::WHITE);
    let pos = at + vec2(16.0, -16.0 - galley.size().y);
    painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(3.0), 3.0, Color32::from_black_alpha(200));
    painter.galley(pos, galley, Color32::WHITE);
}
