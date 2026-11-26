//! The drawing commands: LINE, CIRCLE, ARC, RECTANG, POLYGON, PLINE, TEXT, DIMLINEAR and DIST.

use super::parse::{Entry, is_option, toward};
use super::{App, Cmd, Step};
use crate::gui::tools::Tool;
use crate::model::geom::distance;
use crate::model::{Op, Point, Shape};

pub(super) fn prompt(cmd: Cmd, placed: usize) -> &'static str {
    match (cmd, placed) {
        (Cmd::Line, 0) => "Specify first point:",
        (Cmd::Line, _) => "Specify next point or <exit>:",
        (Cmd::Circle, 0) => "Specify center point for circle:",
        (Cmd::Circle, _) => "Specify radius of circle or [Diameter]:",
        (Cmd::Arc, 0) => "Specify center point of arc:",
        (Cmd::Arc, 1) => "Specify start point of arc:",
        (Cmd::Arc, _) => "Specify end point of arc (clockwise):",
        (Cmd::Rectang, 0) => "Specify first corner point or [Fillet]:",
        (Cmd::Rectang, _) => "Specify other corner point:",
        (Cmd::Polygon, 0) => "Specify center of polygon:",
        (Cmd::Polygon, _) => "Specify radius of polygon:",
        (Cmd::Pline, 0) => "Specify start point:",
        (Cmd::Pline, 1 | 2) => "Specify next point or [Undo] <exit>:",
        (Cmd::Pline, _) => "Specify next point or [Close/Undo] <exit>:",
        (Cmd::Text, _) => "Specify start point of text:",
        (Cmd::DimLinear, 0) => "Specify first extension line origin:",
        (Cmd::DimLinear, _) => "Specify second extension line origin:",
        (Cmd::Dist, 0) => "Specify first point:",
        (_, _) => "Specify second point:",
    }
}

impl App {
    pub(super) fn draw_step(&mut self, cmd: Cmd, step: Step, entry: Entry, pointer: Option<Point>) -> Result<(), String> {
        match (step, entry) {
            (Step::Sides, Entry::Empty) => self.set_step(Step::Draw),
            (Step::Sides, Entry::Number(n)) if (3.0..=64.0).contains(&n) && n.fract() == 0.0 => {
                self.sides = n as u32;
                self.set_step(Step::Draw);
            }
            (Step::Sides, _) => return Err("Enter a whole number from 3 to 64.".into()),
            (Step::Fillet, Entry::Empty) => self.set_step(Step::Draw),
            (Step::Fillet, Entry::Number(r)) if r >= 0.0 => {
                self.cmdline.fillet = r;
                self.set_step(Step::Draw);
            }
            (Step::Fillet, _) => return Err("Enter a radius of 0 or more.".into()),
            (Step::Height, Entry::Empty) => self.set_step(Step::Content),
            (Step::Height, Entry::Number(h)) if h > 0.0 => {
                self.cmdline.text_height = h;
                self.set_step(Step::Content);
            }
            (Step::Height, _) => return Err("Enter a height above 0.".into()),
            (Step::Diameter, entry) => {
                let centre = self.draft[0];
                let (r, toward_p) = match entry {
                    Entry::Number(d) if d > 0.0 => (d / 2.0, pointer),
                    Entry::Point(p) => (distance(centre, p) / 2.0, Some(p)),
                    _ => return Err("Enter a diameter or click a point.".into()),
                };
                self.set_step(Step::Draw);
                self.draw_point(toward(centre, toward_p, r));
            }
            (_, Entry::Number(n)) if self.curve_entry(cmd, &Entry::Number(n)) => {
                let p = self.curve_number(cmd, n, pointer).ok_or("Specify a point.")?;
                self.draw_point(p);
            }
            (_, Entry::Empty) => self.enter_draw(cmd),
            (_, Entry::Word(w)) => self.draw_option(cmd, &w)?,
            (_, entry) => {
                let p = self.point_of(&entry, pointer).ok_or("Specify a point.")?;
                self.draw_point(p);
            }
        }
        Ok(())
    }

    fn draw_option(&mut self, cmd: Cmd, word: &str) -> Result<(), String> {
        let n = self.draft.len();
        if self.curve_option(cmd, word) {
            return Ok(());
        }
        match cmd {
            Cmd::Circle if n == 1 && self.cmdline.circle_mode == super::curves::CircleMode::CenterRadius && is_option(word, "Diameter") => self.set_step(Step::Diameter),
            Cmd::Rectang if n == 0 && is_option(word, "Fillet") => self.set_step(Step::Fillet),
            Cmd::Pline if n >= 3 && is_option(word, "Close") => {
                self.draft.push(self.draft[0]);
                self.finish_draft(true);
            }
            Cmd::Pline if n >= 1 && is_option(word, "Undo") => _ = self.draft.pop(),
            _ => return Err(format!("\"{word}\" is not an option here.")),
        }
        Ok(())
    }

    /// Enter while drawing: a polyline is finished, everything else ends.
    fn enter_draw(&mut self, cmd: Cmd) {
        if cmd == Cmd::Pline && self.draft.len() >= 2 {
            self.finish_draft(false);
        }
        self.end();
    }

    fn draw_point(&mut self, p: Point) {
        self.cmdline.last_point = Some(p);
        if self.tool == Tool::Text {
            self.draft = vec![p];
            self.set_step(Step::Height);
            return;
        }
        // Placing the first point again closes a polyline.
        if self.tool == Tool::Polyline
            && self.draft.len() >= 3
            && distance(self.draft[0], p) * (self.view.zoom as f64) < 8.0
        {
            self.draft.push(self.draft[0]);
            self.finish_draft(true);
            return;
        }
        self.note_snap(p);
        self.draft.push(p);
        let needed = self.active_cmd().and_then(|c| self.curve_points(c)).unwrap_or(self.tool.points());
        if needed > 0 && self.draft.len() >= needed {
            self.finish_draft(false);
        }
    }

    /// TEXT's last step: the text itself.
    pub(super) fn enter_text(&mut self, text: String) {
        if !text.is_empty()
            && let Some(at) = self.draft.first().copied()
        {
            // Text is engraved unless another op than the default (cut) was picked.
            let op = if self.new_op == Op::Cut { Op::Engrave } else { self.new_op };
            let shape = Shape::Text { x: at[0], y: at[1], size: self.cmdline.text_height, text, rotation: 0.0, font: self.cmdline.text_font.clone(), bold: false, italic: false };
            self.create("text", op, shape);
        }
        self.end();
    }

    /// Turn the placed points into an item (or a measurement). LINE goes on from the end point;
    /// the other commands end with their shape.
    pub(in crate::gui) fn finish_draft(&mut self, close: bool) {
        let pts = std::mem::take(&mut self.draft);
        if self.tool == Tool::Measure {
            if let [a, b, ..] = pts[..] {
                self.measured = Some((a, b));
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let angle = (-dy).atan2(dx).to_degrees();
                self.cmdline.log(format!("Distance = {:.3}, Angle = {angle:.1}°, Delta X = {dx:.3}, Delta Y = {dy:.3}", dx.hypot(dy)));
            }
            return self.end();
        }
        let Some((name, mut shape)) = self.draft_shape(&pts) else { return };
        if close && let Shape::Path { contours } = &mut shape {
            contours[0].vertices.pop();
            contours[0].closed = true;
        }
        // Ignore zero-size shapes from accidental double clicks: drop the doubled point.
        if shape.metrics().length < 0.05 {
            self.draft = pts[..pts.len() - 1].to_vec();
            return;
        }
        // A dimension snapped onto two points of the drawing drives them; otherwise it is a note.
        if let (Tool::Dimension, [a, b, ..]) = (self.tool, &pts[..])
            && self.associative_dimension(*a, *b)
        {
            return self.end();
        }
        let op = if self.tool == Tool::Dimension { Op::NoOutput } else { self.new_op };
        let id = self.create(name, op, shape);
        self.infer_drawn(id, &pts);
        match self.tool {
            Tool::Line => self.chain_from(id, pts[pts.len() - 1]),
            _ => self.end(),
        }
    }
}
