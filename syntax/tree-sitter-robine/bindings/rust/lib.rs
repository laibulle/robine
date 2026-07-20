//! Tree-sitter language binding for Robine's explicitly non-normative
//! `prototype-conventional-0` syntax profile.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_robine() -> *const ();
}

/// Tree-sitter language function for the bootstrap Robine grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_robine) };

/// Generated node kinds for this grammar.
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// Canonical highlighting query shared with editor clients.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

#[cfg(test)]
mod tests {
    #[test]
    fn grammar_loads_in_tree_sitter() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("Robine grammar should load");
    }
}
