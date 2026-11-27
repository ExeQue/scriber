//! The open documents (the GUI's tabs): an ordered list, each with an id that stays the same for
//! the session, and the active one. Each document keeps its own undo, selection, layers, comments
//! and laser setup; its file path and unsaved flag live on the `Document`. Shared between the GUI
//! and the MCP server, which can address any open document.
//!
//! Lock order: the workspace lock is never held while a document is locked here, so code holding
//! a document may call into the workspace.

use super::{Document, Shared};
use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::sync::Arc;

/// How many recently used files File → Open recent keeps.
pub const RECENT_MAX: usize = 10;

#[derive(Clone)]
pub struct Workspace(Arc<Mutex<Inner>>);

struct Inner {
    docs: Vec<(u64, Shared)>,
    active: u64,
    next: u64,
    recent: Vec<PathBuf>,
}

/// The same file, also when one path is relative or goes through a link.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// Nothing in it worth keeping: no file, no changes, no items.
fn pristine(d: &Document) -> bool {
    d.path.is_none() && !d.dirty && d.s.items.is_empty() && d.annotations.list.is_empty()
}

impl Workspace {
    pub fn new(doc: Document) -> Self {
        Self::from_shared(Arc::new(Mutex::new(doc)))
    }

    /// A workspace whose one document is `doc`.
    pub fn from_shared(doc: Shared) -> Self {
        Self(Arc::new(Mutex::new(Inner { docs: vec![(1, doc)], active: 1, next: 2, recent: Vec::new() })))
    }

    fn inner(&self) -> parking_lot::MutexGuard<'_, Inner> {
        self.0.lock()
    }

    /// The open documents in tab order.
    pub fn docs(&self) -> Vec<(u64, Shared)> {
        self.inner().docs.clone()
    }

    pub fn get(&self, id: u64) -> Option<Shared> {
        self.inner().docs.iter().find(|(i, _)| *i == id).map(|(_, d)| d.clone())
    }

    pub fn active_id(&self) -> u64 {
        self.inner().active
    }

    /// The active document (there always is one).
    pub fn active(&self) -> (u64, Shared) {
        let w = self.inner();
        w.docs.iter().find(|(i, _)| *i == w.active).cloned().expect("the active document is open")
    }

    /// Add a document as a new tab after the others; returns its id.
    pub fn add(&self, doc: Document, activate: bool) -> u64 {
        let mut w = self.inner();
        let id = w.next;
        w.next += 1;
        w.docs.push((id, Arc::new(Mutex::new(doc))));
        if activate {
            w.active = id;
        }
        id
    }

    /// The open document with this file, if any.
    pub fn find(&self, path: &Path) -> Option<u64> {
        self.docs().into_iter().find(|(_, d)| d.lock().path.as_deref().is_some_and(|p| same_file(p, path))).map(|(id, _)| id)
    }

    /// Open a .scrbr file in a new tab, or return the tab that already has it (`true` = it was
    /// open already). With `activate`, the tab becomes the active one, and an untouched empty
    /// active tab (a fresh start) makes way for it. The file goes on the recent list.
    pub fn open(&self, path: &Path, activate: bool) -> anyhow::Result<(u64, bool)> {
        if let Some(id) = self.find(path) {
            if activate {
                self.activate(id);
            }
            self.remember(path);
            return Ok((id, true));
        }
        let doc = Document::open(path)?;
        let (blank, shared) = self.active();
        let replace = activate && pristine(&shared.lock());
        let id = self.add(doc, activate);
        if replace && self.docs().len() > 1 {
            self.close(blank);
        }
        self.remember(path);
        Ok((id, false))
    }

    /// Make a tab the active one; false when there is no such tab.
    pub fn activate(&self, id: u64) -> bool {
        let mut w = self.inner();
        let found = w.docs.iter().any(|(i, _)| *i == id);
        if found {
            w.active = id;
        }
        found
    }

    /// The tab `step` places after the active one (negative = before), wrapping around.
    pub fn neighbour(&self, step: isize) -> u64 {
        let w = self.inner();
        let at = w.docs.iter().position(|(i, _)| *i == w.active).unwrap_or(0) as isize;
        let n = w.docs.len() as isize;
        w.docs[(at + step).rem_euclid(n) as usize].0
    }

    /// Close a tab, unsaved or not (callers ask first). The next tab (else the one before) becomes
    /// active when it was; closing the last leaves a new empty document of the same size.
    /// Returns false when there is no such tab.
    pub fn close(&self, id: u64) -> bool {
        let Some(shared) = self.get(id) else { return false };
        let (w, h) = {
            let d = shared.lock();
            (d.s.width, d.s.height)
        };
        let mut i = self.inner();
        let Some(at) = i.docs.iter().position(|(d, _)| *d == id) else { return false };
        i.docs.remove(at);
        if i.docs.is_empty() {
            let id = i.next;
            i.next += 1;
            i.docs.push((id, Arc::new(Mutex::new(Document::new(w, h)))));
        }
        if i.active == id {
            i.active = i.docs[at.min(i.docs.len() - 1)].0;
        }
        true
    }

    /// Recently opened or saved files, newest first.
    pub fn recent(&self) -> Vec<PathBuf> {
        self.inner().recent.clone()
    }

    pub fn set_recent(&self, recent: Vec<PathBuf>) {
        let mut r = recent;
        r.truncate(RECENT_MAX);
        self.inner().recent = r;
    }

    /// Put a file at the top of the recent list.
    pub fn remember(&self, path: &Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut w = self.inner();
        w.recent.retain(|p| !same_file(p, &path));
        w.recent.insert(0, path);
        w.recent.truncate(RECENT_MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let mut d = Document::new(50.0, 40.0);
        d.add("box", crate::model::Op::Cut, crate::model::Shape::Path { contours: vec![crate::model::Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0)] });
        d.save(&path).unwrap();
        path
    }

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scriber-ws-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn open_focuses_an_open_file_and_replaces_a_blank_start() {
        let dir = tmp();
        let a = saved(&dir, "a.scrbr");
        let ws = Workspace::new(Document::new(300.0, 200.0));
        let (id, existed) = ws.open(&a, true).unwrap();
        assert!(!existed);
        assert_eq!((ws.docs().len(), ws.active_id()), (1, id), "the untouched blank tab made way");

        let other = ws.add(Document::new(10.0, 10.0), true);
        // Not activating: the person's tab stays; the same file gives the same id.
        assert_eq!(ws.open(&dir.join("./a.scrbr"), false).unwrap(), (id, true));
        assert_eq!(ws.active_id(), other);
        assert_eq!(ws.open(&a, true).unwrap(), (id, true));
        assert_eq!(ws.active_id(), id);
        assert_eq!(ws.recent(), vec![a.canonicalize().unwrap()]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn close_moves_to_the_next_tab_and_never_leaves_none() {
        let ws = Workspace::new(Document::new(30.0, 20.0));
        let one = ws.active_id();
        let two = ws.add(Document::new(30.0, 20.0), true);
        let three = ws.add(Document::new(30.0, 20.0), false);
        ws.get(two).unwrap().lock().dirty = true;
        assert_eq!(ws.neighbour(1), three);
        assert_eq!(ws.neighbour(-2), three, "wraps around");
        assert!(ws.close(two), "closing does not ask; callers do");
        assert_eq!(ws.active_id(), three, "the next tab takes its place");
        assert!(ws.close(three));
        assert_eq!(ws.active_id(), one, "the last tab: the one before");
        assert!(ws.close(one));
        assert!(!ws.close(one));
        let (id, d) = ws.active();
        assert!(id > three && ws.docs().len() == 1);
        let d = d.lock();
        assert_eq!((d.s.width, d.path.clone()), (30.0, None), "a new empty document of the same size");
    }

    #[test]
    fn recent_list_is_newest_first_without_duplicates() {
        let ws = Workspace::new(Document::default());
        for i in 0..12 {
            ws.remember(Path::new(&format!("/nowhere/{i}.scrbr")));
        }
        ws.remember(Path::new("/nowhere/5.scrbr"));
        let r = ws.recent();
        assert_eq!((r.len(), r[0].clone(), r[1].clone()), (RECENT_MAX, PathBuf::from("/nowhere/5.scrbr"), PathBuf::from("/nowhere/11.scrbr")));
        assert_eq!(r.iter().filter(|p| p.ends_with("5.scrbr")).count(), 1);
    }

    #[test]
    fn a_panic_while_a_lock_is_held_leaves_the_locks_usable() {
        let ws = Workspace::new(Document::new(50.0, 40.0));
        let (w, doc) = (ws.clone(), ws.active().1);
        // The panic message below is expected in the test output.
        let r = std::thread::spawn(move || {
            let _ws = w.inner();
            let _d = doc.lock();
            panic!("boom");
        })
        .join();
        assert!(r.is_err());
        // The GUI's path to the document: workspace, then the active document's lock.
        let (_, doc) = ws.active();
        doc.lock().add("box", crate::model::Op::Cut, crate::model::Shape::Path { contours: vec![crate::model::Contour::rect(0.0, 0.0, 10.0, 10.0, 0.0)] });
        assert_eq!(doc.lock().s.items.len(), 1);
    }
}
