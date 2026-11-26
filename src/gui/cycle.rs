//! Selection cycling, as in AutoCAD: where shapes overlap, Shift+Space steps through the ones
//! under the cursor; the candidate is highlighted and is what the next click picks.

use super::App;
use eframe::egui::{self, Event, Key, Modifiers};

/// The shapes under the cursor (front first) and which of them a click picks.
#[derive(Default)]
pub struct Cycle {
    under: Vec<u64>,
    index: usize,
}

impl Cycle {
    /// The shapes under the cursor now; a different set starts again at the front one.
    pub fn update(&mut self, under: Vec<u64>) {
        if self.under != under {
            self.under = under;
            self.index = 0;
        }
    }

    /// The one a click on `under` picks: the cycled candidate when it is the same set, else the front one.
    pub fn pick(&self, under: &[u64]) -> Option<u64> {
        let i = if under == self.under.as_slice() { self.index } else { 0 };
        under.get(i).copied()
    }

    /// The candidate, if any.
    pub fn current(&self) -> Option<u64> {
        self.under.get(self.index).copied()
    }

    /// Step to the next candidate. False when there is nothing to cycle.
    pub fn next(&mut self) -> bool {
        if self.under.len() < 2 {
            return false;
        }
        self.index = (self.index + 1) % self.under.len();
        true
    }

    pub fn count(&self) -> usize {
        self.under.len()
    }
}

impl App {
    /// Shift+Space: the next shape under the cursor. Its release is then not Enter.
    pub(super) fn cycle_key(&mut self, ctx: &egui::Context) -> bool {
        if self.takes_text() || !ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Space)) {
            return false;
        }
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(t) if t == " ")));
        self.space_panned = true;
        self.cycle.next();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycling_order() {
        let mut c = Cycle::default();
        c.update(vec![3, 1, 2]);
        assert_eq!(c.pick(&[3, 1, 2]), Some(3));
        assert!(c.next());
        assert_eq!(c.pick(&[3, 1, 2]), Some(1));
        c.next();
        c.next();
        assert_eq!(c.current(), Some(3), "wraps around");
        c.next();
        // The same shapes again keep the choice; others start at the front.
        c.update(vec![3, 1, 2]);
        assert_eq!(c.current(), Some(1));
        assert_eq!(c.pick(&[5, 1]), Some(5));
        c.update(vec![7]);
        assert!(!c.next());
        assert_eq!(c.current(), Some(7));
    }
}
