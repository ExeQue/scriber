//! The ways a drawing tool can draw, for the ribbon's split buttons: their names, which one is
//! used now, and starting the tool in one of them.

use super::curves::{ArcMode, CircleMode};
use super::rects::RectMode;
use super::{App, Cmd};

impl App {
    /// The ways `cmd` can draw and the one used now; empty for tools with only one way.
    pub(in crate::gui) fn variants(&self, cmd: Cmd) -> (Vec<&'static str>, usize) {
        let c = &self.cmdline;
        let find = |names: Vec<&'static str>, now: &'static str| {
            let n = names.iter().position(|x| *x == now).unwrap_or(0);
            (names, n)
        };
        match cmd {
            Cmd::Circle => find(CircleMode::ALL.map(CircleMode::label).to_vec(), c.circle_mode.label()),
            Cmd::Arc => find(ArcMode::ALL.map(ArcMode::label).to_vec(), c.arc_mode.label()),
            Cmd::Rectang => find(RectMode::ALL.map(RectMode::label).to_vec(), c.rect_mode.label()),
            Cmd::Polygon => (vec!["Size to a corner", "Size to a side"], c.polygon_flat as usize),
            _ => (Vec::new(), 0),
        }
    }

    /// Start `cmd` drawing in its `n`th way (and remember that way).
    pub(in crate::gui) fn start_variant(&mut self, cmd: Cmd, n: usize) {
        let c = &mut self.cmdline;
        match cmd {
            Cmd::Circle => c.circle_mode = CircleMode::ALL[n.min(2)],
            Cmd::Arc => c.arc_mode = ArcMode::ALL[n.min(3)],
            Cmd::Rectang => c.rect_mode = RectMode::ALL[n.min(2)],
            Cmd::Polygon => c.polygon_flat = n == 1,
            _ => {}
        }
        self.start(cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_starts_in_that_way() {
        let doc = std::sync::Arc::new(std::sync::Mutex::new(crate::model::Document::new(100.0, 100.0)));
        let mut app = App::new(doc, Err(String::new()));
        app.start_variant(Cmd::Arc, 2);
        assert_eq!(app.cmdline.arc_mode, ArcMode::CenterStartEnd);
        assert_eq!(app.variants(Cmd::Arc).1, 2);
        assert_eq!(app.active_cmd(), Some(Cmd::Arc));
        app.start_variant(Cmd::Polygon, 1);
        assert!(app.cmdline.polygon_flat);
    }
}
