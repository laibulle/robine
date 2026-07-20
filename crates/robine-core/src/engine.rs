use crate::Analysis;
use crate::semantic::analyze_incremental;
use std::collections::BTreeMap;
use tree_sitter::{InputEdit, Point, Tree};

#[derive(Clone, Debug)]
pub struct DocumentSnapshot {
    pub version: i32,
    pub source: String,
    pub analysis: Analysis,
    syntax_tree: Option<Tree>,
}

#[derive(Default)]
pub struct Engine {
    documents: BTreeMap<String, DocumentSnapshot>,
}

impl Engine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs a snapshot unless a newer version is already present.
    ///
    /// # Panics
    ///
    /// This function would panic only if the internal map failed to retain
    /// both the inserted snapshot and any pre-existing newer snapshot.
    pub fn update(
        &mut self,
        uri: impl Into<String>,
        version: i32,
        source: impl Into<String>,
    ) -> &DocumentSnapshot {
        let uri = uri.into();
        let source = source.into();
        let should_update = self
            .documents
            .get(&uri)
            .is_none_or(|current| version >= current.version);
        if should_update {
            let old_snapshot = self.documents.get(&uri);
            let mut old_tree = old_snapshot.and_then(|snapshot| snapshot.syntax_tree.clone());
            if let (Some(snapshot), Some(tree)) = (old_snapshot, old_tree.as_mut()) {
                tree.edit(&replacement_edit(&snapshot.source, &source));
            }
            let (analysis, syntax_tree) = analyze_incremental(&source, old_tree.as_ref());
            self.documents.insert(
                uri.clone(),
                DocumentSnapshot {
                    version,
                    source,
                    analysis,
                    syntax_tree,
                },
            );
        }
        self.documents
            .get(&uri)
            .expect("document exists after update")
    }

    #[must_use]
    pub fn get(&self, uri: &str) -> Option<&DocumentSnapshot> {
        self.documents.get(uri)
    }

    pub fn close(&mut self, uri: &str) -> Option<DocumentSnapshot> {
        self.documents.remove(uri)
    }
}

fn replacement_edit(old: &str, new: &str) -> InputEdit {
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(old, new)| old == new)
        .count();
    while prefix > 0 && (!old.is_char_boundary(prefix) || !new.is_char_boundary(prefix)) {
        prefix -= 1;
    }

    let maximum_suffix = old.len().min(new.len()).saturating_sub(prefix);
    let mut suffix = old
        .as_bytes()
        .iter()
        .rev()
        .zip(new.as_bytes().iter().rev())
        .take(maximum_suffix)
        .take_while(|(old, new)| old == new)
        .count();
    while suffix > 0
        && (!old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix))
    {
        suffix -= 1;
    }

    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    InputEdit {
        start_byte: prefix,
        old_end_byte: old_end,
        new_end_byte: new_end,
        start_position: point_at(old, prefix),
        old_end_position: point_at(old, old_end),
        new_end_position: point_at(new, new_end),
    }
}

fn point_at(source: &str, offset: usize) -> Point {
    let before = &source[..offset.min(source.len())];
    let row = before.bytes().filter(|byte| *byte == b'\n').count();
    let column = before
        .rfind('\n')
        .map_or(before.len(), |line_end| before.len() - line_end - 1);
    Point::new(row, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_replace_newer_snapshot() {
        let mut engine = Engine::new();
        engine.update("file:///main.robine", 2, "module newest");
        engine.update("file:///main.robine", 1, "module stale");
        assert_eq!(
            engine.get("file:///main.robine").expect("snapshot").source,
            "module newest"
        );
    }

    #[test]
    fn computes_utf8_safe_incremental_edit() {
        let old = "module demo\nfn value() -> Text { \"👩🏽‍💻\" }\n";
        let new = "module demo\nfn value() -> Text { \"Robine 👩🏽‍💻\" }\n";
        let edit = replacement_edit(old, new);
        assert!(old.is_char_boundary(edit.start_byte));
        assert!(old.is_char_boundary(edit.old_end_byte));
        assert!(new.is_char_boundary(edit.new_end_byte));
    }

    #[test]
    fn reparses_valid_document_from_previous_tree() {
        let mut engine = Engine::new();
        let first = "module demo\nfn value() -> Int { 1 }\n";
        let second = "module demo\nfn value() -> Int { 42 }\n";
        engine.update("file:///main.robine", 1, first);
        assert!(
            engine
                .get("file:///main.robine")
                .and_then(|snapshot| snapshot.syntax_tree.as_ref())
                .is_some()
        );
        let updated = engine.update("file:///main.robine", 2, second);
        assert!(updated.analysis.is_valid());
        assert!(updated.syntax_tree.is_some());
    }
}
