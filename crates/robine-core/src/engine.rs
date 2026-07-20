use crate::semantic::{analyze_parsed_modules, analyze_parsed_modules_selected};
use crate::{Analysis, Diagnostic, Program};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{InputEdit, Point, Tree};

#[derive(Clone, Debug)]
pub struct DocumentSnapshot {
    pub version: i32,
    pub source: String,
    pub analysis: Analysis,
    parsed: Result<Program, Vec<Diagnostic>>,
    syntax_tree: Option<Tree>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InvalidationReport {
    pub reparsed: BTreeSet<String>,
    pub retyped: BTreeSet<String>,
}

#[derive(Default)]
pub struct Engine {
    documents: BTreeMap<String, DocumentSnapshot>,
    module_uris: BTreeMap<String, String>,
    last_invalidation: InvalidationReport,
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
            let old_graph = module_graph(&self.documents);
            let old_module = self
                .documents
                .get(&uri)
                .and_then(|snapshot| parsed_program(&snapshot.parsed))
                .map(|program| program.module.clone());
            let old_interface = self
                .documents
                .get(&uri)
                .and_then(|snapshot| parsed_program(&snapshot.parsed))
                .map(interface_key);
            let old_imports = self
                .documents
                .get(&uri)
                .and_then(|snapshot| parsed_program(&snapshot.parsed))
                .map(import_key);
            let old_snapshot = self.documents.get(&uri);
            let mut old_tree = old_snapshot.and_then(|snapshot| snapshot.syntax_tree.clone());
            if let (Some(snapshot), Some(tree)) = (old_snapshot, old_tree.as_mut()) {
                tree.edit(&replacement_edit(&snapshot.source, &source));
            }
            let (parsed, syntax_tree) =
                crate::syntax::parse_incremental(&source, old_tree.as_ref());
            let placeholder = analyze_parsed_modules(std::slice::from_ref(&parsed))
                .pop()
                .expect("one parsed module produces one placeholder analysis");
            let new_module = parsed_program(&parsed).map(|program| program.module.clone());
            let new_interface = parsed_program(&parsed).map(interface_key);
            let new_imports = parsed_program(&parsed).map(import_key);
            self.documents.insert(
                uri.clone(),
                DocumentSnapshot {
                    version,
                    source,
                    analysis: placeholder,
                    parsed,
                    syntax_tree,
                },
            );
            let new_graph = module_graph(&self.documents);
            let graph_changed = old_module != new_module || old_imports != new_imports;
            let interface_changed = old_interface != new_interface;
            let mut selected_modules = BTreeSet::new();
            if graph_changed || interface_changed {
                let seeds = old_module
                    .iter()
                    .chain(new_module.iter())
                    .cloned()
                    .collect::<BTreeSet<_>>();
                selected_modules.extend(reverse_closure(&old_graph, &seeds));
                selected_modules.extend(reverse_closure(&new_graph, &seeds));
            } else if let Some(module) = new_module.clone().or(old_module.clone()) {
                selected_modules.insert(module);
            }
            self.reanalyze(&uri, new_module.or(old_module), &selected_modules);
        }
        self.documents
            .get(&uri)
            .expect("document exists after update")
    }

    #[must_use]
    pub fn get(&self, uri: &str) -> Option<&DocumentSnapshot> {
        self.documents.get(uri)
    }

    #[must_use]
    pub fn uri_for_module(&self, module: &str) -> Option<&str> {
        self.module_uris.get(module).map(String::as_str)
    }

    pub fn snapshots(&self) -> impl Iterator<Item = (&str, &DocumentSnapshot)> {
        self.documents
            .iter()
            .map(|(uri, snapshot)| (uri.as_str(), snapshot))
    }

    #[must_use]
    pub fn dependency_graph(&self) -> BTreeMap<String, Vec<String>> {
        module_graph(&self.documents)
    }

    #[must_use]
    pub const fn last_invalidation(&self) -> &InvalidationReport {
        &self.last_invalidation
    }

    pub fn close(&mut self, uri: &str) -> Option<DocumentSnapshot> {
        let old_graph = module_graph(&self.documents);
        let removed = self.documents.remove(uri)?;
        let removed_module = parsed_program(&removed.parsed).map(|program| program.module.clone());
        let new_graph = module_graph(&self.documents);
        let seeds = removed_module.iter().cloned().collect::<BTreeSet<_>>();
        let mut selected_modules = reverse_closure(&old_graph, &seeds);
        selected_modules.extend(reverse_closure(&new_graph, &seeds));
        self.reanalyze(uri, removed_module, &selected_modules);
        Some(removed)
    }

    fn reanalyze(
        &mut self,
        changed_uri: &str,
        changed_module: Option<String>,
        selected_modules: &BTreeSet<String>,
    ) {
        let uris = self.documents.keys().cloned().collect::<Vec<_>>();
        let parsed = uris
            .iter()
            .map(|uri| self.documents.get(uri).expect("known URI").parsed.clone())
            .collect::<Vec<_>>();
        let previous = uris
            .iter()
            .map(|uri| self.documents.get(uri).expect("known URI").analysis.clone())
            .collect::<Vec<_>>();
        let selected = uris
            .iter()
            .enumerate()
            .filter_map(|(index, uri)| {
                let snapshot = self.documents.get(uri).expect("known URI");
                let module = parsed_program(&snapshot.parsed).map(|program| &program.module);
                (uri == changed_uri
                    || module.is_some_and(|module| selected_modules.contains(module)))
                .then_some(index)
            })
            .collect::<BTreeSet<_>>();
        let analyses = analyze_parsed_modules_selected(&parsed, &selected, Some(&previous));
        for ((uri, analysis), parsed) in uris.iter().zip(analyses).zip(&parsed) {
            self.documents.get_mut(uri).expect("known URI").analysis = analysis;
            if parsed.is_err() {
                self.module_uris.retain(|_, owner| owner != uri);
            }
        }
        self.module_uris.clear();
        for uri in &uris {
            let snapshot = self.documents.get(uri).expect("known URI");
            if let Some(program) = parsed_program(&snapshot.parsed) {
                self.module_uris
                    .entry(program.module.clone())
                    .or_insert_with(|| uri.clone());
            }
        }
        let retyped = selected
            .iter()
            .map(|index| {
                parsed_program(&parsed[*index])
                    .map_or_else(|| uris[*index].clone(), |program| program.module.clone())
            })
            .collect();
        self.last_invalidation = InvalidationReport {
            reparsed: changed_module.into_iter().collect(),
            retyped,
        };
    }
}

fn parsed_program(parsed: &Result<Program, Vec<Diagnostic>>) -> Option<&Program> {
    parsed.as_ref().ok()
}

fn interface_key(program: &Program) -> Vec<String> {
    let mut interface = program
        .functions
        .iter()
        .filter(|function| function.public)
        .map(|function| {
            format!(
                "{}({})->{}!{}",
                function.name,
                function
                    .params
                    .iter()
                    .map(|parameter| parameter.type_name.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                function.return_type,
                function
                    .effects
                    .iter()
                    .map(|(effect, _)| effect.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>();
    interface.sort();
    interface
}

fn import_key(program: &Program) -> BTreeSet<String> {
    program
        .imports
        .iter()
        .map(|import| import.module.clone())
        .collect()
}

fn module_graph(documents: &BTreeMap<String, DocumentSnapshot>) -> BTreeMap<String, Vec<String>> {
    let mut graph = BTreeMap::new();
    for snapshot in documents.values() {
        let Some(program) = parsed_program(&snapshot.parsed) else {
            continue;
        };
        graph
            .entry(program.module.clone())
            .or_insert_with(Vec::new)
            .extend(program.imports.iter().map(|import| import.module.clone()));
    }
    for dependencies in graph.values_mut() {
        dependencies.sort();
        dependencies.dedup();
    }
    graph
}

fn reverse_closure(
    graph: &BTreeMap<String, Vec<String>>,
    seeds: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut reverse = BTreeMap::<String, Vec<String>>::new();
    for (module, dependencies) in graph {
        for dependency in dependencies {
            reverse
                .entry(dependency.clone())
                .or_default()
                .push(module.clone());
        }
    }
    let mut selected = seeds.clone();
    let mut pending = seeds.iter().cloned().collect::<Vec<_>>();
    while let Some(module) = pending.pop() {
        for consumer in reverse.get(&module).into_iter().flatten() {
            if selected.insert(consumer.clone()) {
                pending.push(consumer.clone());
            }
        }
    }
    selected
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
        engine.update("file:///main.ro", 2, "module newest");
        engine.update("file:///main.ro", 1, "module stale");
        assert_eq!(
            engine.get("file:///main.ro").expect("snapshot").source,
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
        engine.update("file:///main.ro", 1, first);
        assert!(
            engine
                .get("file:///main.ro")
                .and_then(|snapshot| snapshot.syntax_tree.as_ref())
                .is_some()
        );
        let updated = engine.update("file:///main.ro", 2, second);
        assert!(updated.analysis.is_valid());
        assert!(updated.syntax_tree.is_some());
    }

    #[test]
    fn retypes_only_changed_module_when_public_interface_is_stable() {
        let mut engine = Engine::new();
        engine.update(
            "file:///math.ro",
            1,
            "module app.math\npub fn answer() -> Int { 1 }\n",
        );
        engine.update(
            "file:///main.ro",
            1,
            "module app.main\nimport app.math\nfn value() -> Int { app.math.answer() }\n",
        );
        engine.update(
            "file:///other.ro",
            1,
            "module app.other\npub fn value() -> Int { 7 }\n",
        );
        assert!(
            engine
                .get("file:///main.ro")
                .expect("main")
                .analysis
                .is_valid()
        );

        engine.update(
            "file:///math.ro",
            2,
            "module app.math\npub fn answer() -> Int { 2 }\n",
        );
        assert_eq!(
            engine.last_invalidation().retyped,
            BTreeSet::from(["app.math".to_owned()])
        );
    }

    #[test]
    fn public_interface_change_retypes_transitive_consumers_only() {
        let mut engine = Engine::new();
        engine.update(
            "file:///math.ro",
            1,
            "module app.math\npub fn answer() -> Int { 1 }\n",
        );
        engine.update(
            "file:///middle.ro",
            1,
            "module app.middle\nimport app.math\npub fn answer() -> Int { app.math.answer() }\n",
        );
        engine.update(
            "file:///main.ro",
            1,
            "module app.main\nimport app.middle\nfn answer() -> Int { app.middle.answer() }\n",
        );
        engine.update(
            "file:///other.ro",
            1,
            "module app.other\npub fn value() -> Int { 7 }\n",
        );

        engine.update(
            "file:///math.ro",
            2,
            "module app.math\npub fn answer() -> Bool { true }\n",
        );
        assert_eq!(
            engine.last_invalidation().retyped,
            BTreeSet::from([
                "app.main".to_owned(),
                "app.math".to_owned(),
                "app.middle".to_owned(),
            ])
        );
        assert!(
            engine
                .get("file:///middle.ro")
                .expect("middle")
                .analysis
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RBN3002")
        );
        assert_eq!(engine.uri_for_module("app.math"), Some("file:///math.ro"));
        assert_eq!(
            engine.dependency_graph().get("app.middle"),
            Some(&vec!["app.math".to_owned()])
        );
    }
}
