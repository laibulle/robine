# Robine for Zed

This development extension supports the explicitly non-normative
`prototype-conventional-0` syntax profile and associates it with `.ro` source
files. The language server loads the owning `robine.toml` workspace, resolves
nominal imports and provides cross-file diagnostics, completion and navigation.

Build and install the language server:

```bash
cargo install --path crates/robine-cli
```

In Zed, run `zed: install dev extension` and select `editors/zed-robine`.
The extension is intentionally thin: Tree-sitter provides presentation
features and `robine lsp --stdio` provides all semantic features.

The grammar manifest follows the `main` branch during local prototyping. Before
marketplace publication, `rev` must be replaced by the exact commit containing
the grammar and release provenance for the server must be defined.
