# Robine bootstrap implementation

This workspace contains the first executable Robine prototype. It is a
bootstrap implementation in Rust, not a claim that the Draft specifications
are Accepted.

The `prototype-conventional-0` profile uses `.ro` source files. The former
`.robine` suffix is intentionally not supported as an alias.

## Implemented specification subset

- `LANG-002`: one explicitly non-normative syntax profile,
  `prototype-conventional-0`;
- `LANG-003`: nominal multi-file modules, explicit imports, private-by-default
  functions, public functions, immutable local bindings, first-order calls,
  conditional expressions, signed 64-bit integer arithmetic, left-to-right
  evaluation and effect-free module loading for the implemented subset;
- `TYPE-004`: explicit effect rows and an explicit `Console` capability;
- `CPL-001`: source, resolved and typed HIR, a small explicit Core and a
  Cranelift native development backend;
- `DX-001`: persistent document snapshots, incremental Tree-sitter reparsing
  and interface-sensitive transitive module invalidation;
- `DX-004`: stable diagnostic codes and machine-readable source ranges;
- `TOOL-001` and `TOOL-002`: a shared semantic engine, CLI, workspace-aware
  LSP adapter with cross-file navigation and a thin Zed client;
- `PKG-001` and `PKG-003`: deterministic recursive `.ro` discovery below a
  declared source root and a manifest-backed synchronous application target;
- `FFI-001`: a small Rust bridge with an explicitly exported C ABI.

`examples/rust-bridge` lowers a Robine call through that stable ABI into the
`unicode-segmentation` crate. Its manifest names the library, symbol, ABI,
borrowed text parameter, result, effects and panic strategy. The wrapper
validates UTF-8 and converts both panic and invalid input into a sentinel error
instead of unwinding across the boundary.

For the provisional syntax profile, `Int` is a signed 64-bit value. `+`, `-`
and `*` wrap modulo 2^64 and comparisons are signed. This is published
bootstrap behavior for differential testing, not a decision for the canonical
numeric model.

## Known conformance gaps

- invalidation is minimal between modules, but resolution and typing still
  recheck the complete changed module instead of the minimal definition
  subgraph;
- module interfaces are kept in memory by the bootstrap and are not yet
  serialized as ARCH-001 artifacts for separate package compilation;
- the implemented type subset is `Unit`, `Bool`, `Int`, `Text`, `Console` and
  first-order functions, not the complete set-theoretic type system or numeric
  literal constraint system;
- the structured Core and Cranelift backend lower literals, locals,
  first-order calls, conditional expressions and the two implemented host
  adapters, but not closures, patterns or general collections;
- stable identities cover modules, definitions and named locals, not every
  significant anonymous syntax node;
- hot, sealed, Wasm, ownership, actors, realtime, UI and compute domains are
  not implemented.

These limits are reported here so the prototype cannot silently become the
normative definition of Robine.
