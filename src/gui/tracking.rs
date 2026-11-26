//! Object snap tracking (OTRACK) and the tracking snaps, as in AutoCAD. Resting the pointer on a
//! snap point for a moment acquires it (a small +); dashed alignment paths then run from it
//! horizontally and vertically (and at POLAR's angles), and the cursor snaps to a path and to where
//! two paths cross. Extension tracks along a line or arc beyond the end it was acquired at,
//! Parallel along the direction of a line pointed at earlier, and POLAR from the last point is a
//! path too, so its crossings with the others snap as well.

use super::App;
use super::select::Drag;
use super::snap::{self, Kind, Seg, Snap, circle_circle};
use crate::model::geom::distance;
use crate::model::{Item, Point};
use eframe::egui::{self, Color32, Stroke, vec2};
use std::time::{Duration, Instant};

/// How long the pointer rests on a snap point before it is acquired.
pub const ACQUIRE: Duration = Duration::from_millis(500);
/// At most this many acquired points (the oldest goes), as in AutoCAD.
const MAX_POINTS: usize = 7;
const MAX_LINES: usize = 3;
const TRACK: Color32 = Color32::from_rgb(120, 200, 120);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Path {
    /// The endless line through `o` at `ang` degrees (counter-clockwise on screen, 0 = right).
    Line { o: Point, ang: f64 },
    /// The full circle of an arc (Extension on an arc).
    Circle { c: Point, r: f64 },
}

/// Why a path is there; it names the path in the cursor tooltip.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    Polar,
    Ortho,
    Track,
    Extension,
    Parallel,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Track {
    pub path: Path,
    pub source: Source,
}

/// Where the pointer locked: the point and the one or two paths through it.
#[derive(Clone, PartialEq, Debug)]
pub struct Locked {
    pub p: Point,
    pub tracks: Vec<Track>,
}

/// The direction of `ang` degrees in model space (y points down).
fn dir(ang: f64) -> Point {
    let a = ang.to_radians();
    [a.cos(), -a.sin()]
}

/// The angle (degrees, counter-clockwise on screen) from `a` to `b`.
pub fn angle(a: Point, b: Point) -> f64 {
    (a[1] - b[1]).atan2(b[0] - a[0]).to_degrees()
}

impl Path {
    fn nearest(self, p: Point) -> Point {
        match self {
            Path::Line { o, ang } => {
                let d = dir(ang);
                let t = (p[0] - o[0]) * d[0] + (p[1] - o[1]) * d[1];
                [o[0] + t * d[0], o[1] + t * d[1]]
            }
            Path::Circle { c, r } => {
                let d = distance(p, c);
                if d < 1e-12 { [c[0] + r, c[1]] } else { [c[0] + (p[0] - c[0]) * r / d, c[1] + (p[1] - c[1]) * r / d] }
            }
        }
    }
}

/// Where two paths cross.
pub fn crossings(a: Path, b: Path) -> Vec<Point> {
    match (a, b) {
        (Path::Line { o: o1, ang: a1 }, Path::Line { o: o2, ang: a2 }) => {
            let (r, s) = (dir(a1), dir(a2));
            let den = r[0] * s[1] - r[1] * s[0];
            if den.abs() < 1e-9 {
                return Vec::new();
            }
            let t = ((o2[0] - o1[0]) * s[1] - (o2[1] - o1[1]) * s[0]) / den;
            vec![[o1[0] + t * r[0], o1[1] + t * r[1]]]
        }
        (line @ Path::Line { ang, .. }, Path::Circle { c, r }) | (Path::Circle { c, r }, line @ Path::Line { ang, .. }) => {
            let f = line.nearest(c);
            let h2 = r * r - distance(f, c).powi(2);
            if h2 < 0.0 {
                return Vec::new();
            }
            let (d, h) = (dir(ang), h2.sqrt());
            vec![[f[0] - h * d[0], f[1] - h * d[1]], [f[0] + h * d[0], f[1] + h * d[1]]]
        }
        (Path::Circle { c: c1, r: r1 }, Path::Circle { c: c2, r: r2 }) => circle_circle(c1, r1, c2, r2),
    }
}

/// Where pointer `p` locks: the closest crossing of two paths within `tol` mm, else the closest
/// point on a path within `tol`.
pub fn lock(tracks: &[Track], p: Point, tol: f64) -> Option<Locked> {
    let mut best: Option<(f64, Locked)> = None;
    for (i, a) in tracks.iter().enumerate() {
        for b in &tracks[i + 1..] {
            for q in crossings(a.path, b.path) {
                let d = distance(p, q);
                if d <= tol && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, Locked { p: q, tracks: vec![*a, *b] }));
                }
            }
        }
    }
    if let Some((_, l)) = best {
        return Some(l);
    }
    tracks
        .iter()
        .map(|t| (t, t.path.nearest(p)))
        .map(|(t, q)| (distance(p, q), t, q))
        .filter(|(d, _, _)| *d <= tol)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t, q)| Locked { p: q, tracks: vec![*t] })
}

/// The path Extension follows from an end of `seg`: the line it lies on, or its arc's circle.
pub fn extension(seg: Seg) -> Path {
    match seg {
        Seg::Line(a, b) => Path::Line { o: b, ang: angle(a, b) },
        Seg::Arc { c, r, .. } => Path::Circle { c, r },
    }
}

/// Angles as line directions (an endless line at 0° is the one at 180°), without repeats.
fn line_angles(angles: &[f64]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for a in angles.iter().map(|a| a.rem_euclid(180.0)) {
        if !out.iter().any(|b| (a - b).abs() < 1e-9) {
            out.push(a);
        }
    }
    out
}

/// What was acquired by resting the pointer on it, until the command ends.
#[derive(Default)]
pub struct Tracker {
    pub points: Vec<Point>,
    pub extensions: Vec<Path>,
    /// Directions (degrees) of lines pointed at for Parallel.
    pub parallels: Vec<f64>,
    /// What the pointer rests on (a snap point, or a line's ends), since when, and whether it was used.
    rest: Option<([f64; 4], Instant, bool)>,
}

impl Tracker {
    /// The pointer rests on `key` now. True once, when it has rested there for ACQUIRE.
    pub fn rested(&mut self, key: Option<[f64; 4]>, now: Instant) -> bool {
        match (&mut self.rest, key) {
            (Some((k, since, done)), Some(n)) if *k == n => {
                if !*done && now.duration_since(*since) >= ACQUIRE {
                    *done = true;
                    return true;
                }
                false
            }
            (_, key) => {
                self.rest = key.map(|k| (k, now, false));
                false
            }
        }
    }

    /// Something is about to be acquired (the canvas must repaint once the time is up).
    pub fn waiting(&self) -> bool {
        self.rest.is_some_and(|(_, _, done)| !done)
    }

    /// Acquire point `p` with its extension paths; an acquired point is dropped again instead.
    pub fn toggle_point(&mut self, p: Point, extensions: Vec<Path>) {
        if let Some(i) = self.points.iter().position(|q| distance(*q, p) < 1e-9) {
            self.points.remove(i);
            self.extensions.retain(|e| !extensions.contains(e));
            return;
        }
        self.points.push(p);
        if self.points.len() > MAX_POINTS {
            self.points.remove(0);
        }
        self.extensions.extend(extensions);
        let extra = self.extensions.len().saturating_sub(MAX_LINES);
        self.extensions.drain(..extra);
    }

    pub fn add_parallel(&mut self, ang: f64) {
        let a = ang.rem_euclid(180.0);
        if !self.parallels.iter().any(|b| (a - b).abs() < 1e-9) {
            self.parallels.push(a);
            let extra = self.parallels.len().saturating_sub(MAX_LINES);
            self.parallels.drain(..extra);
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The paths the pointer can lock to. `from` is the last placed point; `polar` POLAR's angles
    /// when it is on (OTRACK also uses them, else it tracks horizontally and vertically).
    pub fn tracks(&self, from: Option<Point>, polar: Option<&[f64]>, ortho: bool, otrack: bool, osnap: bool) -> Vec<Track> {
        let mut out = Vec::new();
        let mut add = |o: Point, angles: &[f64], source: Source| out.extend(line_angles(angles).into_iter().map(|ang| Track { path: Path::Line { o, ang }, source }));
        if let Some(f) = from {
            if ortho {
                add(f, &[0.0, 90.0], Source::Ortho);
            } else if let Some(angles) = polar {
                add(f, angles, Source::Polar);
            }
            if osnap {
                add(f, &self.parallels, Source::Parallel);
            }
        }
        if otrack {
            for &p in &self.points {
                add(p, polar.unwrap_or(&[0.0, 90.0]), Source::Track);
            }
        }
        if osnap {
            out.extend(self.extensions.iter().map(|&path| Track { path, source: Source::Extension }));
        }
        out
    }
}

impl App {
    /// OTRACK, Extension and Parallel: acquire what the pointer rests on (while a point is asked for).
    pub(super) fn acquire(&mut self, items: &[Item], guides: &[[Point; 2]], hit: Option<Snap>, now: Instant) {
        let asking = self.active_cmd().is_some() || matches!(self.drag, Some(Drag::Grip { .. }));
        let hit = hit.filter(|_| asking && (self.aids.otrack || self.aids.osnap));
        let on = |p: Point| snap::pieces_near(items, guides, p, 1e-6);
        let key = match hit {
            Some(Snap { kind: Kind::Nearest, p, .. }) => on(p).into_iter().find_map(|s| if let Seg::Line(a, b) = s { Some([a[0], a[1], b[0], b[1]]) } else { None }),
            Some(Snap { p, .. }) => Some([p[0], p[1], f64::INFINITY, f64::INFINITY]),
            None => None,
        };
        if !self.tracker.rested(key, now) {
            return;
        }
        let (Some(s), Some(k)) = (hit, key) else { return };
        if s.kind == Kind::Nearest {
            if self.aids.osnap {
                self.tracker.add_parallel(angle([k[0], k[1]], [k[2], k[3]]));
            }
            return;
        }
        let ends_here = |seg: &Seg| seg.ends().iter().any(|e| distance(*e, s.p) < 1e-6);
        let ext = if self.aids.osnap && s.kind == Kind::Endpoint { on(s.p).into_iter().filter(ends_here).map(extension).collect() } else { Vec::new() };
        self.tracker.toggle_point(s.p, ext);
    }

    /// Acquired points (+), and the dashed path(s) the pointer is locked to with what they are.
    pub(super) fn paint_tracking(&self, painter: &egui::Painter, rect: egui::Rect) {
        let plus = Stroke::new(1.5, Color32::from_rgb(255, 170, 0));
        if self.aids.otrack {
            for p in &self.tracker.points {
                let at = self.w2s(*p);
                painter.line_segment([at - vec2(4.0, 0.0), at + vec2(4.0, 0.0)], plus);
                painter.line_segment([at - vec2(0.0, 4.0), at + vec2(0.0, 4.0)], plus);
            }
        }
        let Some(l) = &self.aids.tracking else { return };
        let stroke = Stroke::new(1.0, TRACK);
        let far = rect.width() + rect.height();
        let at = self.w2s(l.p);
        for t in &l.tracks {
            match t.path {
                Path::Line { o, .. } => {
                    let a = self.w2s(o);
                    let d = if distance(o, l.p) > 1e-9 { (at - a).normalized() } else { vec2(1.0, 0.0) };
                    painter.extend(egui::Shape::dashed_line(&[a, at + d * far], stroke, 4.0, 4.0));
                }
                Path::Circle { c, r } => {
                    let pts: Vec<_> = (0..=96).map(|k| self.w2s(snap::polar_point(c, r, k as f64 / 96.0 * std::f64::consts::TAU))).collect();
                    painter.extend(egui::Shape::dashed_line(&pts, stroke, 4.0, 4.0));
                }
            }
        }
        let kind = match l.tracks.as_slice() {
            [_, _] => Kind::Intersection,
            [Track { source: Source::Extension, .. }] => Kind::Extension,
            [Track { source: Source::Parallel, .. }] => Kind::Parallel,
            _ => return super::canvas::tooltip(painter, at, &describe(&l.tracks[0], l.p)),
        };
        snap::paint_marker(painter, at, kind);
        let text = l.tracks.iter().map(|t| describe(t, l.p)).collect::<Vec<_>>().join("  ·  ");
        super::canvas::tooltip(painter, at, &text);
    }
}

impl App {
    /// POLAR's settings: the increment angle and extra angles to track.
    pub(super) fn polar_menu(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Track every").small().weak());
        for step in [90.0, 45.0, 30.0, 22.5, 18.0, 15.0, 10.0, 5.0] {
            if ui.radio(self.aids.polar_step == step, format!("{step}°")).clicked() {
                self.aids.polar_step = step;
                self.aids.polar = true;
                self.aids.ortho = false;
            }
        }
        ui.separator();
        ui.label("Also track these angles");
        let r = ui.add(egui::TextEdit::singleline(&mut self.polar_extra_text).hint_text("e.g. 7.5, 63").desired_width(120.0));
        if r.changed() {
            self.aids.polar_extra = self.polar_extra_text.split([',', ' ', ';']).filter_map(|t| t.trim().parse().ok()).collect();
        }
    }
}

/// "Polar: 23.00 < 45°" and the like.
fn describe(t: &Track, p: Point) -> String {
    let name = match t.source {
        Source::Polar => "Polar",
        Source::Ortho => "Ortho",
        Source::Track => "Track",
        Source::Extension => "Extension",
        Source::Parallel => "Parallel",
    };
    match t.path {
        Path::Line { o, .. } => format!("{name}: {:.2} < {:.0}°", distance(o, p), angle(o, p)),
        Path::Circle { c, r } => format!("{name}: arc R {r:.2}, {:.0}°", angle(c, p)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        distance(a, b) < 1e-6
    }

    fn track(o: Point, ang: f64) -> Track {
        Track { path: Path::Line { o, ang }, source: Source::Track }
    }

    #[test]
    fn crossing_of_two_paths_wins() {
        // Vertical through (10, 0) and horizontal through (0, 20) cross at (10, 20).
        let tracks = [track([10.0, 0.0], 90.0), track([0.0, 20.0], 0.0)];
        let l = lock(&tracks, [10.4, 19.7], 1.0).unwrap();
        assert!(close(l.p, [10.0, 20.0]) && l.tracks.len() == 2);
        // Away from the crossing: onto the closest path.
        let l = lock(&tracks, [10.3, 50.0], 1.0).unwrap();
        assert!(close(l.p, [10.0, 50.0]) && l.tracks == vec![tracks[0]]);
        assert!(lock(&tracks, [30.0, 50.0], 1.0).is_none());
    }

    #[test]
    fn path_crossings() {
        // 45° up-right from the origin (y down) meets the vertical at x = 5 at (5, -5).
        let q = crossings(Path::Line { o: [0.0, 0.0], ang: 45.0 }, Path::Line { o: [5.0, 0.0], ang: 90.0 });
        assert!(close(q[0], [5.0, -5.0]), "{q:?}");
        assert!(crossings(Path::Line { o: [0.0, 0.0], ang: 0.0 }, Path::Line { o: [0.0, 1.0], ang: 180.0 }).is_empty());
        let q = crossings(Path::Line { o: [0.0, 0.0], ang: 0.0 }, Path::Circle { c: [10.0, 0.0], r: 3.0 });
        assert!(close(q[0], [7.0, 0.0]) && close(q[1], [13.0, 0.0]), "{q:?}");
    }

    #[test]
    fn tracks_from_acquired_points_and_the_last_point() {
        let mut t = Tracker::default();
        t.toggle_point([10.0, 10.0], vec![Path::Line { o: [10.0, 10.0], ang: 0.0 }]);
        t.add_parallel(210.0);
        // OTRACK alone: horizontal and vertical; the extension; nothing from `from` without POLAR.
        let tr = t.tracks(Some([0.0, 0.0]), None, false, true, true);
        assert_eq!(tr.iter().filter(|x| x.source == Source::Track).count(), 2);
        assert_eq!(tr.iter().filter(|x| x.source == Source::Extension).count(), 1);
        assert!(tr.iter().any(|x| x.source == Source::Parallel && x.path == Path::Line { o: [0.0, 0.0], ang: 30.0 }));
        // POLAR at 45°: four line directions from both the last point and the acquired one.
        let polar = [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
        let tr = t.tracks(Some([0.0, 0.0]), Some(&polar), false, true, false);
        assert_eq!(tr.iter().filter(|x| x.source == Source::Polar).count(), 4);
        assert_eq!(tr.iter().filter(|x| x.source == Source::Track).count(), 4);
        // Acquiring the same point again drops it with its extension.
        t.toggle_point([10.0, 10.0], vec![Path::Line { o: [10.0, 10.0], ang: 0.0 }]);
        assert!(t.points.is_empty() && t.extensions.is_empty());
    }

    #[test]
    fn resting_acquires_once() {
        let mut t = Tracker::default();
        let now = Instant::now();
        let k = Some([1.0, 2.0, 0.0, 0.0]);
        assert!(!t.rested(k, now) && t.waiting());
        assert!(t.rested(k, now + ACQUIRE));
        assert!(!t.rested(k, now + ACQUIRE * 2) && !t.waiting());
        assert!(!t.rested(None, now));
    }

    /// The real input path: `aim` as the canvas calls it each frame, with the pointer resting and moving.
    #[test]
    fn otrack_works_with_polar_off() {
        use crate::gui::command::Cmd;
        use crate::model::{Contour, Document, Op, Shape};
        use std::sync::{Arc, Mutex};
        let doc = Arc::new(Mutex::new(Document::new(200.0, 200.0)));
        let mut app = App::new(doc.clone(), Err(String::new()));
        doc.lock().unwrap().add("", Op::Cut, Shape::path(Contour::line([0.0, 0.0], [40.0, 0.0])));
        let items = doc.lock().unwrap().s.items.clone();
        app.aids.polar = false;
        app.start(Cmd::Line);
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let aim = |app: &mut App, p: Point, from: Option<Point>, t: Instant| {
            app.aids.hit = None;
            app.aids.tracking = None;
            app.aim_at_time(&items, p, from, t)
        };
        aim(&mut app, [40.2, 0.1], None, ms(0));
        aim(&mut app, [40.2, 0.1], None, ms(600));
        assert_eq!(app.tracker.points, vec![[40.0, 0.0]]);
        // Straight below the acquired point, with and without a placed first point.
        let p = aim(&mut app, [40.5, 30.0], None, ms(700));
        assert!(close(p, [40.0, 30.0]), "no from: {p:?}");
        let p = aim(&mut app, [40.5, 30.0], Some([10.0, 50.0]), ms(800));
        assert!(close(p, [40.0, 30.0]), "with from: {p:?}");
    }

    #[test]
    fn otrack_acquires_on_rest_and_snaps_to_crossing_guides() {
        use crate::gui::command::Cmd;
        use crate::model::{Contour, Document, Op, Shape};
        use std::sync::{Arc, Mutex};
        let doc = Arc::new(Mutex::new(Document::new(200.0, 200.0)));
        let mut app = App::new(doc.clone(), Err(String::new()));
        {
            let mut d = doc.lock().unwrap();
            d.add("", Op::Cut, Shape::path(Contour::line([0.0, 0.0], [40.0, 0.0])));
            d.add("", Op::Cut, Shape::path(Contour::line([100.0, 60.0], [100.0, 80.0])));
        }
        let items = doc.lock().unwrap().s.items.clone();
        app.start(Cmd::Line);
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let aim = |app: &mut App, p: Point, t: Instant| {
            app.aids.hit = None;
            app.aids.tracking = None;
            app.aim_at_time(&items, p, None, t)
        };
        // Resting on the end of the first line: acquired once the time is up, not before.
        aim(&mut app, [40.2, 0.1], ms(0));
        assert!(app.tracker.points.is_empty() && app.tracker.waiting());
        aim(&mut app, [40.3, 0.2], ms(300));
        assert!(app.tracker.points.is_empty());
        aim(&mut app, [40.2, 0.1], ms(550));
        assert_eq!(app.tracker.points, vec![[40.0, 0.0]]);
        // A second point, acquired the same way.
        aim(&mut app, [100.1, 60.2], ms(700));
        aim(&mut app, [100.1, 60.2], ms(1300));
        assert_eq!(app.tracker.points, vec![[40.0, 0.0], [100.0, 60.0]]);
        // Below the first and level with the second: the guides from both cross there, and the
        // first point of the line goes exactly there.
        let p = aim(&mut app, [40.6, 59.5], ms(1400));
        assert!(close(p, [40.0, 60.0]), "{p:?}");
        assert_eq!(app.aids.tracking.as_ref().map(|l| l.tracks.len()), Some(2));
        // Along one guide only.
        let p = aim(&mut app, [40.5, 30.0], ms(1500));
        assert!(close(p, [40.0, 30.0]), "{p:?}");
        // Resting on an acquired point again drops it.
        aim(&mut app, [100.1, 60.2], ms(1600));
        aim(&mut app, [100.1, 60.2], ms(2200));
        assert_eq!(app.tracker.points, vec![[40.0, 0.0]]);
        // The points stay while the command runs and go when it ends.
        app.place_point(p);
        assert_eq!(app.tracker.points, vec![[40.0, 0.0]]);
        app.escape();
        assert!(app.tracker.points.is_empty());
    }

    #[test]
    fn extension_of_a_line_runs_past_its_end() {
        let p = extension(Seg::Line([0.0, 0.0], [10.0, 0.0]));
        let l = lock(&[Track { path: p, source: Source::Extension }], [25.0, 0.4], 1.0).unwrap();
        assert!(close(l.p, [25.0, 0.0]));
    }
}
