//! Small windows (Check, Parameters, …) open where they first appear every time: once closed, a
//! window forgets where it was dragged. Each opening gets a fresh egui id, so no old position is found.

use eframe::egui::Id;
use std::collections::HashMap;

#[derive(Default)]
pub struct Popouts {
    /// Per window: whether it was shown last frame, and how often it has been opened.
    seen: HashMap<&'static str, (bool, u32)>,
}

impl Popouts {
    /// Note which windows are shown this frame; call once per frame before drawing them.
    pub fn track(&mut self, windows: &[(&'static str, bool)]) {
        for &(name, shown) in windows {
            let (was, n) = self.seen.entry(name).or_default();
            if shown && !*was {
                *n += 1;
            }
            *was = shown;
        }
    }

    /// The window's id for this opening.
    pub fn id(&self, name: &'static str) -> Id {
        Id::NULL.with((name, self.seen.get(name).map_or(0, |s| s.1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopening_gives_a_new_id() {
        let mut p = Popouts::default();
        p.track(&[("check", true)]);
        let first = p.id("check");
        p.track(&[("check", true)]);
        assert_eq!(p.id("check"), first, "same while open");
        p.track(&[("check", false)]);
        p.track(&[("check", true)]);
        assert_ne!(p.id("check"), first);
    }
}
