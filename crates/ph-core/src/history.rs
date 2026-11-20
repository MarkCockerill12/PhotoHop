//! Snapshot undo for the document. A document holds no pixels, so a snapshot is a cheap clone.
//! Gestures (a drag, a slider scrub) are grouped: `begin` before the first change, `commit` after.

use crate::doc::Project;

#[derive(Default)]
pub struct History {
    past: Vec<(String, Project)>,
    future: Vec<(String, Project)>,
    pending: Option<(String, Project)>,
    limit: usize,
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self { limit, ..Default::default() }
    }

    /// Remember the state before a gesture. Repeated calls during one gesture are ignored.
    pub fn begin(&mut self, label: &str, doc: &Project) {
        if self.pending.is_none() {
            self.pending = Some((label.to_owned(), doc.clone()));
        }
    }

    /// Finish a gesture; only records an entry if the document actually changed.
    pub fn commit(&mut self, doc: &Project) {
        if let Some((label, before)) = self.pending.take()
            && before != *doc
        {
            self.past.push((label, before));
            if self.past.len() > self.limit {
                self.past.remove(0);
            }
            self.future.clear();
        }
    }

    /// One-shot change: snapshot `doc` as it is *before* the caller mutates it.
    pub fn record(&mut self, label: &str, doc: &Project) {
        self.commit(doc);
        self.past.push((label.to_owned(), doc.clone()));
        if self.past.len() > self.limit {
            self.past.remove(0);
        }
        self.future.clear();
    }

    pub fn in_gesture(&self) -> bool {
        self.pending.is_some()
    }

    pub fn undo(&mut self, doc: &mut Project) -> Option<String> {
        self.commit(doc);
        let (label, prev) = self.past.pop()?;
        self.future.push((label.clone(), std::mem::replace(doc, prev)));
        Some(label)
    }

    pub fn redo(&mut self, doc: &mut Project) -> Option<String> {
        let (label, next) = self.future.pop()?;
        self.past.push((label.clone(), std::mem::replace(doc, next)));
        Some(label)
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|(l, _)| l.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|(l, _)| l.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesture_groups_changes() {
        let mut doc = Project::new("t", 100, 100, 24.0, 1.0);
        let mut h = History::new(50);
        h.begin("rename", &doc);
        doc.name = "a".into();
        doc.name = "ab".into();
        h.commit(&doc);
        assert_eq!(h.undo(&mut doc).as_deref(), Some("rename"));
        assert_eq!(doc.name, "t");
        h.redo(&mut doc);
        assert_eq!(doc.name, "ab");
    }

    #[test]
    fn unchanged_gesture_not_recorded() {
        let doc = Project::new("t", 100, 100, 24.0, 1.0);
        let mut h = History::new(50);
        h.begin("noop", &doc);
        h.commit(&doc);
        assert!(h.undo_label().is_none());
    }
}
