//! Review comments pinned on the canvas: the person points at a spot or at shapes and writes what
//! to change; an agent (over MCP) picks them up, acts, replies and resolves them. Agents may leave
//! notes for the person the same way.
//!
//! Annotations are saved in the .scrbr file but are not part of the drawing: never lasered or
//! exported, and kept out of the undo history. A comment is a conversation about the drawing, not
//! a change to it: undoing a drawing edit (the person's or an agent's `undo`) must not take back a
//! comment or its reply, and undoing a comment must not undo drawing work.
//!
//! Every change stamps the annotation with a new process-wide `revision` and wakes everyone waiting
//! in `subscribe` (the MCP long-poll `wait_for_annotations`). Process-wide rather than per
//! document, so it keeps growing when a file is opened or a new document started.

use super::{Document, Point};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    /// The person at the GUI.
    #[default]
    Person,
    /// An agent, by the name it gave.
    Agent(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Open,
    Resolved,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub author: Author,
    pub text: String,
    /// Unix seconds.
    pub time: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    /// Also the number on its pin.
    pub id: u64,
    pub at: Point,
    /// The shapes it points at (may be empty, or name shapes deleted since).
    #[serde(default)]
    pub items: Vec<u64>,
    pub text: String,
    #[serde(default)]
    pub author: Author,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub replies: Vec<Reply>,
    /// Unix seconds.
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
    /// Who made the last change (comment, reply, resolve), so agents wait for the person's changes only.
    #[serde(default)]
    pub changed_by: Author,
    /// The revision of the last change; fresh on every load.
    #[serde(skip)]
    pub revision: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Annotations {
    pub list: Vec<Annotation>,
    #[serde(default)]
    next_id: u64,
}

static REVISION: LazyLock<watch::Sender<u64>> = LazyLock::new(|| watch::Sender::new(0));

/// The latest revision.
pub fn revision() -> u64 {
    *REVISION.borrow()
}

/// Wakes (`changed()`) on every annotation change after this call, in any document.
pub fn subscribe() -> watch::Receiver<u64> {
    REVISION.subscribe()
}

fn bump() -> u64 {
    let mut r = 0;
    REVISION.send_modify(|v| {
        *v += 1;
        r = *v;
    });
    r
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Annotations {
    pub fn get(&self, id: u64) -> Option<&Annotation> {
        self.list.iter().find(|a| a.id == id)
    }

    pub fn open_count(&self) -> usize {
        self.list.iter().filter(|a| a.status == Status::Open).count()
    }

    /// Fresh revisions after loading from a file (they are not saved).
    pub(super) fn loaded(mut self) -> Self {
        for a in &mut self.list {
            a.revision = bump();
        }
        self.next_id = self.next_id.max(self.list.iter().map(|a| a.id).max().unwrap_or(0));
        self
    }
}

fn check_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() { Err("the text is empty".into()) } else { Ok(()) }
}

impl Document {
    /// Pin a comment at `at`, or at the centre of `items` when `at` is missing. Returns its id.
    pub fn annotate(&mut self, author: Author, text: &str, at: Option<Point>, items: Vec<u64>) -> Result<u64, String> {
        check_text(text)?;
        if let Some(bad) = items.iter().find(|id| self.item(**id).is_none()) {
            return Err(format!("no item with id {bad}"));
        }
        let at = match (at, self.bbox(&items)) {
            (Some(at), _) => at,
            (None, Some([x, y, w, h])) => [x + w / 2.0, y + h / 2.0],
            (None, None) => return Err("give at (a point) or items to pin the annotation to".into()),
        };
        let n = &mut self.annotations;
        n.next_id += 1;
        let id = n.next_id;
        let t = now();
        n.list.push(Annotation { id, at, items, text: text.trim().into(), author: author.clone(), status: Status::Open, replies: Vec::new(), created: t, updated: t, changed_by: author, revision: 0 });
        self.annotation_changed(id, None);
        Ok(id)
    }

    /// Add a reply; `resolve` also resolves (true) or reopens (false) it.
    pub fn reply_annotation(&mut self, id: u64, author: Author, text: &str, resolve: Option<bool>) -> Result<(), String> {
        check_text(text)?;
        let a = self.annotation_mut(id)?;
        a.replies.push(Reply { author: author.clone(), text: text.trim().into(), time: now() });
        if let Some(r) = resolve {
            a.status = if r { Status::Resolved } else { Status::Open };
        }
        self.annotation_changed(id, Some(author));
        Ok(())
    }

    pub fn set_annotation_status(&mut self, id: u64, author: Author, status: Status) -> Result<(), String> {
        self.annotation_mut(id)?.status = status;
        self.annotation_changed(id, Some(author));
        Ok(())
    }

    pub fn delete_annotation(&mut self, id: u64) -> Result<(), String> {
        self.annotation_mut(id)?;
        self.annotations.list.retain(|a| a.id != id);
        bump();
        self.dirty = true;
        self.rev += 1;
        Ok(())
    }

    fn annotation_mut(&mut self, id: u64) -> Result<&mut Annotation, String> {
        self.annotations.list.iter_mut().find(|a| a.id == id).ok_or_else(|| format!("no annotation with id {id}"))
    }

    /// Stamp a change: revision, time, who; the document needs saving and views refresh.
    fn annotation_changed(&mut self, id: u64, by: Option<Author>) {
        if let Some(a) = self.annotations.list.iter_mut().find(|a| a.id == id) {
            a.revision = bump();
            a.updated = now();
            if let Some(by) = by {
                a.changed_by = by;
            }
        }
        self.dirty = true;
        self.rev += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Contour, Op, Shape};

    fn agent() -> Author {
        Author::Agent("claude".into())
    }

    #[test]
    fn add_reply_resolve_bump_revisions() {
        let mut d = Document::new(100.0, 100.0);
        let sq = d.add("sq", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 20.0, 20.0, 0.0)));
        let start = revision();
        let id = d.annotate(Author::Person, " make it bigger ", None, vec![sq]).unwrap();
        let a = d.annotations.get(id).unwrap().clone();
        assert_eq!((a.at, a.text.as_str(), a.status, a.changed_by.clone()), ([20.0, 20.0], "make it bigger", Status::Open, Author::Person));
        assert!(a.revision > start);
        assert!(d.dirty);

        d.reply_annotation(id, agent(), "done", Some(true)).unwrap();
        let b = d.annotations.get(id).unwrap();
        assert!(b.revision > a.revision);
        assert_eq!((b.status, b.changed_by.clone(), b.replies.len()), (Status::Resolved, agent(), 1));
        assert_eq!(d.annotations.open_count(), 0);

        d.set_annotation_status(id, Author::Person, Status::Open).unwrap();
        assert_eq!(d.annotations.open_count(), 1);
        assert!(d.annotate(Author::Person, "  ", Some([0.0, 0.0]), vec![]).is_err());
        assert!(d.annotate(Author::Person, "x", None, vec![]).is_err());
        assert!(d.reply_annotation(99, agent(), "x", None).is_err());
        let second = d.annotate(agent(), "note", Some([1.0, 2.0]), vec![]).unwrap();
        assert_eq!(second, id + 1);
        d.delete_annotation(id).unwrap();
        assert_eq!(d.annotations.list.len(), 1);
    }

    #[test]
    fn undo_leaves_annotations_alone() {
        let mut d = Document::new(100.0, 100.0);
        d.checkpoint();
        d.add("sq", Op::Cut, Shape::path(Contour::rect(10.0, 10.0, 20.0, 20.0, 0.0)));
        d.annotate(Author::Person, "hi", Some([1.0, 1.0]), vec![]).unwrap();
        d.undo();
        assert!(d.s.items.is_empty());
        assert_eq!(d.annotations.list.len(), 1);
    }
}
