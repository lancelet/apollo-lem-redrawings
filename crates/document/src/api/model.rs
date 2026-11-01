//! The Elm model: a document, its history, a preview, and the journal.
//!
//! ```text
//! Model { doc, history, future, preview, journal }
//! Msg   = Op | Batch | Preview(Op) | DropPreview | Commit | Undo | Redo
//! update(&mut model, msg) -> Result<(), Error>
//! ```
//!
//! `doc` is the settled document. An [`Op`] applied through [`Msg::Op`]
//! replaces it with the changed value and pushes the old one onto
//! `history`; undo pops it back (and pushes the undone one onto `future`
//! for redo). No inverses, no back doors: the forward-only status rules
//! hold absolutely, because undo is a previous value, not an edit.
//!
//! A drag or an animation lives in `preview`: [`Msg::Preview`] applies an
//! op to the preview (started from `doc`), [`Msg::Commit`] moves the
//! preview into `doc` as one history step, [`Msg::DropPreview`] discards
//! it. The document records only settled states.
//!
//! The journal is the message log. `update` is deterministic, so
//! [`Model::replay`] from the starting document reproduces the current
//! one; the journal is also the API's wire format and the audit trail.

use crate::api::msg::Op;
use crate::state::document::{Document, Error};
use serde::{Deserialize, Serialize};

/// A message to the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum Msg {
    /// Apply an op to the document, as one history step.
    Op {
        /// The op.
        op: Op,
    },
    /// Apply several ops as one history step: all, or none and the error.
    Batch {
        /// The ops, in order.
        ops: Vec<Op>,
    },
    /// Apply an op to the preview, starting one from the document if
    /// there is none.
    Preview {
        /// The op.
        op: Op,
    },
    /// Discard the preview.
    DropPreview,
    /// Make the preview the document, as one history step.
    Commit,
    /// Back to the previous document.
    Undo,
    /// Forward again.
    Redo,
}

/// The model. See the module documentation.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    doc: Document,
    history: Vec<Document>,
    future: Vec<Document>,
    preview: Option<Document>,
    journal: Vec<Msg>,
}

impl Model {
    /// A model holding `doc`, with nothing to undo.
    pub fn new(doc: Document) -> Self {
        Self {
            doc,
            history: Vec::new(),
            future: Vec::new(),
            preview: None,
            journal: Vec::new(),
        }
    }

    /// The settled document.
    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// The preview, if a drag is in progress.
    pub fn preview(&self) -> Option<&Document> {
        self.preview.as_ref()
    }

    /// What to show: the preview if there is one, else the document.
    pub fn shown(&self) -> &Document {
        self.preview.as_ref().unwrap_or(&self.doc)
    }

    /// How many steps back are available.
    pub fn undo_depth(&self) -> usize {
        self.history.len()
    }

    /// How many steps forward are available.
    pub fn redo_depth(&self) -> usize {
        self.future.len()
    }

    /// Every message that changed the model, in order.
    pub fn journal(&self) -> &[Msg] {
        &self.journal
    }

    /// Handle a message. A message that fails changes nothing and is not
    /// journaled.
    pub fn update(&mut self, msg: Msg) -> Result<(), Error> {
        match &msg {
            Msg::Op { op } => {
                let next = self.doc.apply(op)?;
                self.settle(next);
            }
            Msg::Batch { ops } => {
                let mut next = self.doc.clone();
                for op in ops {
                    next.apply_in_place(op)?;
                }
                self.settle(next);
            }
            Msg::Preview { op } => {
                let base = self.preview.as_ref().unwrap_or(&self.doc);
                self.preview = Some(base.apply(op)?);
            }
            Msg::DropPreview => {
                if self.preview.take().is_none() {
                    return Err(Error::NoPreview);
                }
            }
            Msg::Commit => {
                let next = self.preview.take().ok_or(Error::NoPreview)?;
                self.settle(next);
            }
            Msg::Undo => {
                let prev = self.history.pop().ok_or(Error::NothingToUndo)?;
                let now = std::mem::replace(&mut self.doc, prev);
                self.future.push(now);
                self.preview = None;
            }
            Msg::Redo => {
                let next = self.future.pop().ok_or(Error::NothingToRedo)?;
                let now = std::mem::replace(&mut self.doc, next);
                self.history.push(now);
                self.preview = None;
            }
        }
        self.journal.push(msg);
        Ok(())
    }

    /// A new settled document: the old one goes on the history, the
    /// future is cleared.
    fn settle(&mut self, next: Document) {
        let prev = std::mem::replace(&mut self.doc, next);
        self.history.push(prev);
        self.future.clear();
        self.preview = None;
    }

    /// The model reached by handling `journal` from `start`, or the first
    /// message that fails and where.
    pub fn replay(start: Document, journal: &[Msg]) -> Result<Self, (usize, Error)> {
        let mut m = Self::new(start);
        for (i, msg) in journal.iter().enumerate() {
            m.update(msg.clone()).map_err(|e| (i, e))?;
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::document::{Note, Title};
    use crate::value::geometry::P2;
    use crate::value::provenance::{Agent, Date};

    fn note(text: &str) -> Op {
        Op::AddNote {
            note: Note {
                at: P2::new(0.0, 0.0),
                text: text.into(),
                by: Agent::User,
                when: Date::new(2026, 10, 15).unwrap(),
            },
        }
    }

    #[test]
    fn undo_is_by_value_and_the_journal_replays() {
        let start = Document::new(Title::default());
        let mut m = Model::new(start.clone());
        m.update(Msg::Op { op: note("one") }).unwrap();
        m.update(Msg::Op { op: note("two") }).unwrap();
        assert_eq!(m.doc().notes().len(), 2);
        assert_eq!(m.undo_depth(), 2);
        m.update(Msg::Undo).unwrap();
        assert_eq!(m.doc().notes().len(), 1);
        assert_eq!((m.undo_depth(), m.redo_depth()), (1, 1));
        m.update(Msg::Redo).unwrap();
        assert_eq!(m.doc().notes().len(), 2);
        // A new edit after an undo clears the future.
        m.update(Msg::Undo).unwrap();
        m.update(Msg::Op { op: note("three") }).unwrap();
        assert_eq!(m.redo_depth(), 0);
        assert_eq!(m.update(Msg::Redo), Err(Error::NothingToRedo));
        // A batch is one step, all or nothing.
        m.update(Msg::Batch {
            ops: vec![note("b1"), note("b2")],
        })
        .unwrap();
        assert_eq!(m.doc().notes().len(), 4);
        assert!(m
            .update(Msg::Batch {
                ops: vec![note("b3"), Op::RemoveNote { index: 99 }],
            })
            .is_err());
        assert_eq!(m.doc().notes().len(), 4);
        m.update(Msg::Undo).unwrap();
        assert_eq!(m.doc().notes().len(), 2);
        m.update(Msg::Redo).unwrap();
        // A failing message is not journaled.
        let n = m.journal().len();
        assert!(m
            .update(Msg::Op {
                op: Op::RemoveNote { index: 9 }
            })
            .is_err());
        assert_eq!(m.journal().len(), n);
        // Preview: shown but not settled until committed, one step.
        m.update(Msg::Preview { op: note("drag") }).unwrap();
        m.update(Msg::Preview {
            op: note("drag more"),
        })
        .unwrap();
        assert_eq!(m.shown().notes().len(), 6);
        assert_eq!(m.doc().notes().len(), 4);
        m.update(Msg::Commit).unwrap();
        assert_eq!(m.doc().notes().len(), 6);
        assert!(m.preview().is_none());
        assert_eq!(m.update(Msg::Commit), Err(Error::NoPreview));
        m.update(Msg::Preview {
            op: note("dropped"),
        })
        .unwrap();
        m.update(Msg::DropPreview).unwrap();
        assert_eq!(m.shown().notes().len(), 6);
        // Replay reproduces the model exactly.
        let again = Model::replay(start, m.journal()).unwrap();
        assert_eq!(again, m);
        // Undo all the way back is the start.
        while m.undo_depth() > 0 {
            m.update(Msg::Undo).unwrap();
        }
        assert_eq!(m.doc(), &Document::new(Title::default()));
        assert_eq!(m.update(Msg::Undo), Err(Error::NothingToUndo));
    }
}
