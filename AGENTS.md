# AGENTS.md

This file applies to the entire repository.

## Mission

Robine is a specification-first programming language project.

The project starts from observed engineering failures and successes across
systems programming, Lisp development, actor runtimes, native mobile, Web
frontends and AI tooling. It does not select a favorite historical language and
extend it. It re-evaluates each trade-off.

The central design rule is:

> Strong guarantees must be local, composable and checked. Their costs must not
> become a global tax on unrelated code.

Agents working in this repository must preserve that rule.

## Read before changing anything

Read these files in order:

1. `README.md`
2. `doc/specs/README.md`
3. `doc/specs/meta/META-001-specification-process/spec.md`
4. `doc/specs/_template/spec.md`
5. every spec directly relevant to the requested change

Follow references from the relevant specs when the change crosses type,
runtime, architecture, security or compatibility boundaries.

Do not infer the language design from README examples alone. The specs are the
source of truth.

## Current project state

- The repository currently defines the language through specifications.
- There is no compiler implementation yet.
- Specs are written in French.
- The root README is written in English.
- The source syntax is deliberately unresolved.
- `LANG-002` has status `Exploration`.
- Code examples are semantic sketches until a syntax is accepted.
- The type design combines set-theoretic semantics with local
  Hindley–Milner-style polymorphic inference.
- Effects, capabilities, ownership, refinements and tensor shapes are separate
  axes rather than one universal solver.

Never present an unresolved proposal as an accepted language feature.

## Non-negotiable design constraints

### Local execution domains

Robine distinguishes:

- `normal`
- `script`
- `responsive`
- `realtime`
- `kernel`
- `ui`
- `isolated`

These domains share one language. They restrict legal operations and select
runtime services locally.

Do not solve a domain-specific problem by imposing its runtime model on the
whole program.

### Development and release are different materializations

Development may use versioned calls, metadata, inspection and hot reload.

A sealed release may specialize, inline and remove those mechanisms when they
are no longer required.

Do not claim that hot reload, fair scheduling, dynamic dispatch or migration
has literally zero cost. State where the cost exists and when it can be
eliminated.

### Real-time means bounded behavior

The real-time domain forbids unbounded allocation, blocking, suspension,
general I/O and uncertified foreign code.

Do not hide such behavior behind a safe-looking abstraction.

### CPU/NPU is a heterogeneous target

The CPU owns irregular control, actors and operating-system interaction.
Vector or matrix CPU instructions handle small kernels. NPU/dataflow engines
handle sufficiently large, pure and regular graphs.

Placement must account for wake-up, transfer, conversion, queueing, memory,
latency, energy and numerical quality—not marketing TOPS alone.

### Architecture must be executable

Public contracts are written once and compiled into machine-readable interface
artifacts. Do not introduce human-maintained header duplication.

Dependency, effect, capability and ownership policies should be checked against
the real program graph.

Do not create an interface or layer without identifying the boundary of
change, privilege, ownership, process or platform that justifies it.

### No ambient authority

Packages and libraries receive no filesystem, network, process, secret or
platform capability implicitly.

Build scripts and foreign code must declare their authority and execution
domain.

## Specification workflow

Every new language, runtime or tooling feature must have a spec.

Create it at:

```text
doc/specs/<domain>/<FEAT-ID>-<feature-name>/spec.md
```

Start from:

```text
doc/specs/_template/spec.md
```

Feature IDs must be unique and use the domain prefix followed by three digits.

Every spec must contain these H2 sections in this order:

1. `Objet`
2. `Non-objectifs`
3. `Spécification normative`
4. `Diagnostics et erreurs`
5. `Sécurité, confidentialité et ressources`
6. `Interactions`
7. `Compatibilité et migration`
8. `Tests de conformité`
9. `Questions ouvertes`

`Alternatives rejetées` is the only optional H2 section. Place it immediately
before `Questions ouvertes`.

Feature-specific structure belongs under `Spécification normative` using H3
headings.

Add every spec to `doc/specs/README.md`.

## Spec status

Valid statuses are:

- `Exploration`
- `Draft`
- `Proposed`
- `Accepted`
- `Deprecated`
- `Rejected`

Do not promote a spec to `Proposed` without a prototype and conformance tests.

Do not promote a spec to `Accepted` without:

- defined static and dynamic semantics;
- expected diagnostics;
- positive and negative tests;
- compilation and runtime cost analysis;
- documented cross-domain interactions;
- a compatibility strategy or an explicit justification for none.

Status changes are design decisions. Do not make them as incidental cleanup.

## Normative language

French specs use these exact normative terms:

- `DOIT`
- `NE DOIT PAS`
- `DEVRAIT`
- `NE DEVRAIT PAS`
- `PEUT`

Use normative language only for observable or testable requirements.

Avoid vague claims such as “fast,” “safe,” “zero-cost,” “native” or
“energy-efficient” without defining the profile, boundary or measurement that
makes the claim testable.

## Syntax discipline

Do not select S-expressions, conventional expression syntax or any other source
syntax unless the task explicitly addresses `LANG-002`.

Until `LANG-002` is accepted:

- keep examples syntax-neutral where practical;
- label syntax experiments as non-normative;
- do not let one example silently settle punctuation or macro semantics;
- evaluate syntax against DSP, actors, UI, types, incremental parsing,
  structural editing and AI patches;
- distinguish expression orientation, homoiconicity and REPL architecture.

## Changes to existing specs

Before modifying a spec:

1. identify the behavior being changed;
2. inspect all referenced specs;
3. classify the change using `META-001`;
4. update interactions, compatibility and tests;
5. preserve deliberate open questions;
6. update the version when the change is not purely editorial.

Do not copy the same normative rule into multiple specs. Choose one owner and
reference it from the others.

If two specs conflict, report the conflict explicitly and resolve it at the
semantic boundary. Do not make their wording merely appear compatible.

## Implementation work

When implementation begins:

- keep the semantic core independent of surface syntax;
- use the same compiler engine for CLI, editor, REPL and AI tooling;
- retain stable structural identities through incremental edits;
- make every lowering stage verifiable;
- keep immediate, optimized and sealed compilation semantically equivalent;
- avoid mandatory runtime services;
- treat FFI and generated code as explicit trust boundaries;
- add conformance tests alongside each implemented spec.

Prototype code must state which Draft or Proposed specs it implements. A
prototype must not silently become normative.

## Documentation style

- Lead with the engineering outcome.
- Prefer precise prose over slogans.
- Explain unavoidable costs.
- Separate guarantees from measurements and estimates.
- Separate current behavior from roadmap.
- Use small examples that demonstrate one rule.
- Keep humor in narrative documentation; keep normative specs unambiguous.
- Preserve the exact opening of the root README:

```text
# Robine

My dog wrote a programming language.
```

Do not add invented benchmark numbers, hardware claims or compatibility
promises.

## Validation

Run from the repository root:

```bash
./scripts/validate-specs.mjs
git diff --check
```

The spec validator checks:

- path and feature ID;
- unique IDs;
- status, SemVer and domain metadata;
- required section order;
- normative requirements;
- index coverage;
- known cross-references;
- Markdown links;
- temporary placeholders.

If implementation code is added, run its formatter, unit tests, conformance
tests and relevant benchmarks in addition to the commands above.

## Repository hygiene

- Preserve unrelated user changes.
- Do not rewrite accepted decisions as cleanup.
- Do not commit generated build artifacts or local caches.
- Keep dependencies minimal and justified.
- Prefer deterministic tools without hidden network access.
- Do not commit, push, publish or change spec status unless the user requested
  it or the active task clearly requires it.

## Definition of done

A change is complete only when:

- the requested behavior or documentation exists;
- relevant specs and interactions agree;
- compatibility and resource implications are documented;
- required tests or validation are present;
- `./scripts/validate-specs.mjs` passes;
- `git diff --check` passes;
- unresolved decisions remain explicitly unresolved.
