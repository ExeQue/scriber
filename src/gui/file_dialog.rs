//! File dialogs that do not freeze the window: the system dialog runs on its own thread and the
//! chosen path comes back over a channel the UI polls each frame. A blocking dialog on the UI
//! thread stops egui's event loop, and after a few seconds the desktop flags the window as not
//! responding. One dialog at a time, across the whole app: requests while one is open are ignored.

use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

static OPEN: AtomicBool = AtomicBool::new(false);

/// A dialog's answer still to come, and what it was asked for (`K`).
pub(super) struct Pending<K>(Option<(K, Receiver<Option<PathBuf>>)>);

impl<K> Default for Pending<K> {
    fn default() -> Self {
        Self(None)
    }
}

impl<K> Pending<K> {
    /// Show an open dialog for `kind`.
    pub fn pick(&mut self, ctx: &egui::Context, kind: K, d: rfd::FileDialog) {
        self.start(ctx, kind, move || d.pick_file());
    }

    /// Show a folder dialog for `kind`.
    pub fn pick_folder(&mut self, ctx: &egui::Context, kind: K, d: rfd::FileDialog) {
        self.start(ctx, kind, move || d.pick_folder());
    }

    /// Show a save dialog for `kind`.
    pub fn save(&mut self, ctx: &egui::Context, kind: K, d: rfd::FileDialog) {
        self.start(ctx, kind, move || d.save_file());
    }

    fn start(&mut self, ctx: &egui::Context, kind: K, ask: impl FnOnce() -> Option<PathBuf> + Send + 'static) {
        if OPEN.swap(true, Ordering::SeqCst) {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("file-dialog".into()).spawn(move || {
            let path = ask();
            OPEN.store(false, Ordering::SeqCst);
            let _ = tx.send(path);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.0 = Some((kind, rx)),
            Err(_) => OPEN.store(false, Ordering::SeqCst),
        }
    }

    /// The answer, once the person chose a file (None while open, or when cancelled).
    pub fn poll(&mut self) -> Option<(K, PathBuf)> {
        let (_, rx) = self.0.as_ref()?;
        match rx.try_recv() {
            Err(TryRecvError::Empty) => None,
            Ok(Some(path)) => self.0.take().map(|(k, _)| (k, path)),
            Ok(None) | Err(TryRecvError::Disconnected) => {
                self.0 = None;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_arrives_without_blocking_and_one_dialog_at_a_time() {
        let ctx = egui::Context::default();
        let (mut a, mut b) = (Pending::default(), Pending::default());
        let (go, wait) = channel::<()>();
        a.start(&ctx, 1, move || {
            wait.recv().unwrap();
            Some(PathBuf::from("/x.scrbr"))
        });
        b.start(&ctx, 2, || Some(PathBuf::from("/y")));
        assert!(b.0.is_none(), "ignored while another dialog is open");
        assert_eq!(a.poll(), None, "still open: the UI goes on");
        go.send(()).unwrap();
        let t = std::time::Instant::now();
        let got = loop {
            if let Some(r) = a.poll() {
                break r;
            }
            assert!(t.elapsed().as_secs() < 5);
            std::thread::yield_now();
        };
        assert_eq!(got, (1, PathBuf::from("/x.scrbr")));
    }
}
