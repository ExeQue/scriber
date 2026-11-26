//! XLINE, RAY, CENTERLINE and CENTERMARK: construction lines that are never lasered. Each command
//! keeps placing guides until Enter; the whole run is one undo step.

use super::parse::{Entry, is_option};
use super::ux::GuideMode;
use super::{App, Cmd};
use crate::model::geom::{distance, segment_distance};
use crate::model::{Guide, Op, Point, Shape};
use eframe::egui::{self, DragValue, RichText};

/// Angle of the direction from `a` to `b` in model degrees (clockwise on screen).
fn angle(a: Point, b: Point) -> f64 {
    (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees()
}

impl App {
    fn add_guide(&mut self, name: &str, g: Guide) -> Result<(), String> {
        self.apply_ids(&[], |d, _| {
            let id = d.add(name, Op::NoOutput, Shape::Guide(g));
            d.selection = vec![id];
            Ok(())
        })
    }

    pub(super) fn guide_step(&mut self, cmd: Cmd, entry: Entry, pointer: Option<Point>) -> Result<(), String> {
        if entry == Entry::Empty {
            self.end();
            return Ok(());
        }
        if cmd == Cmd::Xline {
            match &entry {
                Entry::Word(w) => {
                    let mode = GuideMode::ALL.into_iter().find(|m| is_option(w, m.word())).ok_or(format!("\"{w}\" is not an option here."))?;
                    self.set_guide_mode(mode);
                    return Ok(());
                }
                Entry::Number(n) if self.cmdline.ux.guide == GuideMode::Ang => {
                    self.cmdline.ux.angle = *n;
                    return Ok(());
                }
                Entry::Number(n) if self.cmdline.ux.guide == GuideMode::Offset && *n > 0.0 => {
                    self.cmdline.ux.offset = *n;
                    return Ok(());
                }
                _ => {}
            }
        }
        let p = self.point_of(&entry, pointer).ok_or("Specify a point.")?;
        self.cmdline.last_point = Some(p);
        match cmd {
            Cmd::Centerline => {
                let id = self.item_at(p)?;
                match self.cmdline.ux.pick.take() {
                    None => {
                        self.cmdline.ux.pick = Some((id, p));
                        self.mark_picked(id, p);
                    }
                    Some((first, _)) if first == id => return Err("Pick a second, different line.".into()),
                    Some((first, _)) => {
                        let ok = {
                            let d = self.doc.lock().unwrap();
                            let contour = |id: u64| d.item(id).and_then(|i| i.shape.contours().first().cloned());
                            matches!((contour(first), contour(id)), (Some(a), Some(b)) if Guide::centerline(&a, &b).is_some())
                        };
                        if !ok {
                            return Err("Pick two lines.".into());
                        }
                        // The real ends are filled in from the two lines (the centre line follows them).
                        self.add_guide("centre line", Guide::Centerline { x1: 0.0, y1: 0.0, x2: 0.0, y2: 0.0, of: vec![first, id] })?;
                    }
                }
            }
            Cmd::Centermark => {
                let id = self.item_at(p)?;
                let found = {
                    let d = self.doc.lock().unwrap();
                    d.item(id).and_then(|i| i.shape.contours().first().and_then(Guide::circle_of))
                };
                let ([cx, cy], r) = found.ok_or("Pick a circle or an arc.")?;
                self.add_guide("centre mark", Guide::Centermark { cx, cy, r, of: Some(id) })?;
            }
            Cmd::Ray => match self.cmdline.ux.pts.first().copied() {
                None => {
                    self.cmdline.ux.pts.push(p);
                    self.draft = vec![p];
                }
                Some(a) if distance(a, p) < 1e-9 => return Err("Pick a point away from the start.".into()),
                Some(a) => self.add_guide("ray", Guide::Ray { x: a[0], y: a[1], angle: angle(a, p) })?,
            },
            _ => self.xline_point(p)?,
        }
        Ok(())
    }

    fn set_guide_mode(&mut self, mode: GuideMode) {
        let u = &mut self.cmdline.ux;
        u.guide = mode;
        u.pts.clear();
        u.pick = None;
        self.draft.clear();
    }

    fn xline_point(&mut self, p: Point) -> Result<(), String> {
        let xline = |q: Point, angle: f64| Guide::Xline { x: q[0], y: q[1], angle };
        let u = &self.cmdline.ux;
        match (u.guide, u.pts.as_slice()) {
            (GuideMode::Hor, _) => self.add_guide("guide line", xline(p, 0.0)),
            (GuideMode::Ver, _) => self.add_guide("guide line", xline(p, 90.0)),
            // Typed angles are counter-clockwise, as everywhere on the command line.
            (GuideMode::Ang, _) => self.add_guide("guide line", xline(p, -u.angle)),
            (GuideMode::Points, [a]) if distance(*a, p) > 1e-9 => self.add_guide("guide line", xline(*a, angle(*a, p))),
            (GuideMode::Bisect, [v, a]) => {
                let (ua, ub) = (angle(*v, *a).to_radians(), angle(*v, p).to_radians());
                let dir = [ua.cos() + ub.cos(), ua.sin() + ub.sin()];
                // Opposite directions: the bisector is square to them.
                let bisector = if dir[0].hypot(dir[1]) < 1e-9 { ua.to_degrees() + 90.0 } else { dir[1].atan2(dir[0]).to_degrees() };
                let v = *v;
                self.cmdline.ux.pts.truncate(1);
                self.draft = vec![v];
                self.add_guide("guide line", xline(v, bisector))
            }
            (GuideMode::Offset, _) => self.xline_offset(p),
            (GuideMode::Points | GuideMode::Bisect, _) => {
                self.cmdline.ux.pts.push(p);
                self.draft = vec![self.cmdline.ux.pts[0]];
                Ok(())
            }
        }
    }

    /// XLINE Offset: first click picks the line, the second the side.
    fn xline_offset(&mut self, p: Point) -> Result<(), String> {
        let Some((id, at)) = self.cmdline.ux.pick.take() else {
            let id = self.item_at(p)?;
            self.cmdline.ux.pick = Some((id, p));
            self.mark_picked(id, p);
            return Ok(());
        };
        let seg = {
            let d = self.doc.lock().unwrap();
            let outline = d.item(id).map(|i| i.shape.outline(0.05)).unwrap_or_default();
            outline.iter().flat_map(|l| l.windows(2).map(|s| [s[0], s[1]]).collect::<Vec<_>>()).min_by(|a, b| segment_distance(at, a[0], a[1]).total_cmp(&segment_distance(at, b[0], b[1])))
        };
        let [a, b] = seg.ok_or("Pick a line.")?;
        let len = distance(a, b);
        if len < 1e-9 {
            return Err("Pick a line.".into());
        }
        let (u, n) = ([(b[0] - a[0]) / len, (b[1] - a[1]) / len], [-(b[1] - a[1]) / len, (b[0] - a[0]) / len]);
        let side = if (p[0] - a[0]) * n[0] + (p[1] - a[1]) * n[1] < 0.0 { -1.0 } else { 1.0 };
        let off = self.cmdline.ux.offset * side;
        let angle = u[1].atan2(u[0]).to_degrees();
        self.add_guide("guide line", Guide::Xline { x: a[0] + n[0] * off, y: a[1] + n[1] * off, angle })
    }

    pub(super) fn guide_prompt(&self, cmd: Cmd) -> String {
        let u = &self.cmdline.ux;
        match cmd {
            Cmd::Ray if u.pts.is_empty() => "Specify start point:",
            Cmd::Ray => "Specify through point (Enter when done):",
            Cmd::Centerline if u.pick.is_none() => "Select first line:",
            Cmd::Centerline => "Select second line:",
            Cmd::Centermark => "Select circle or arc (Enter when done):",
            _ => match (u.guide, u.pts.len(), u.pick.is_some()) {
                (GuideMode::Points, 0, _) => "Specify a point or [Hor/Ver/Ang/Bisect/Offset]:",
                (GuideMode::Points, _, _) => "Specify through point (Enter when done):",
                (GuideMode::Hor | GuideMode::Ver | GuideMode::Ang, _, _) => "Specify through point (Enter when done):",
                (GuideMode::Bisect, 0, _) => "Specify angle vertex point:",
                (GuideMode::Bisect, 1, _) => "Specify angle start point:",
                (GuideMode::Bisect, _, _) => "Specify angle end point:",
                (GuideMode::Offset, _, false) => "Select a line object:",
                (GuideMode::Offset, _, true) => "Specify side to offset:",
            },
        }
        .into()
    }

    pub(super) fn xline_fields(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for mode in GuideMode::ALL {
                if ui.selectable_label(self.cmdline.ux.guide == mode, mode.label()).clicked() {
                    self.set_guide_mode(mode);
                }
            }
        });
        let u = &mut self.cmdline.ux;
        match u.guide {
            GuideMode::Ang => _ = ui.horizontal(|ui| {
                ui.label("Angle");
                ui.add(DragValue::new(&mut u.angle).speed(1.0).suffix("°"));
            }),
            GuideMode::Offset => _ = ui.horizontal(|ui| {
                ui.label("Distance");
                ui.add(DragValue::new(&mut u.offset).speed(0.1).range(0.01..=10000.0).suffix(" mm"));
            }),
            _ => {}
        }
        ui.label(RichText::new("Guide lines help you draw; they are never cut or engraved.").small().weak());
    }
}
