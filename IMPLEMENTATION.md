# Robine bootstrap implementation

This workspace contains the first executable Robine prototype. It is a
bootstrap implementation in Rust, not a claim that the Draft specifications
are Accepted.

## Implemented specification subset

- `LANG-002`: one explicitly non-normative syntax profile,
  `prototype-conventional-0`;
- `LANG-003`: modules, immutable local bindings, left-to-right calls and
  effect-free module loading for the implemented subset;
- `TYPE-004`: explicit effect rows and an explicit `Console` capability;
- `CPL-001`: source, resolved and typed HIR, a small explicit Core and a
  Cranelift native development backend;
- `DX-001`: persistent document snapshots in the language server;
- `DX-004`: stable diagnostic codes and machine-readable source ranges;
- `TOOL-001` and `TOOL-002`: a shared semantic engine, CLI, LSP adapter and
  thin Zed client;
- `PKG-001` and `PKG-003`: a manifest-backed synchronous application target;
- `FFI-001`: a small Rust bridge with an explicitly exported C ABI.

`examples/rust-bridge` lowers a Robine call through that stable ABI into the
`unicode-segmentation` crate. The wrapper validates UTF-8 and converts both
panic and invalid input into a sentinel error instead of unwinding across the
boundary.

## Known conformance gaps

- Tree-sitter reparses edited text incrementally, but resolution and typing
  still recheck the complete changed file instead of the minimal definition
  subgraph;
- the implemented type subset is `Unit`, `Bool`, `Int`, `Text`, `Console` and
  first-order functions, not the complete set-theoretic type system;
- the Core and Cranelift backend only lower the constructs exercised by the
  bootstrap conformance programs;
- stable identities cover modules, definitions and named locals, not every
  significant anonymous syntax node;
- hot, sealed, Wasm, ownership, actors, realtime, UI and compute domains are
  not implemented.

These limits are reported here so the prototype cannot silently become the
normative definition of Robine.
