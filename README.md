# Robine

My dog wrote a programming language.

## Background

Nobody knows who darkened the sky.

Humans claim it was the machines. The machines claim it was an expired TLS
certificate in a Kubernetes admission controller. The incident report was
stored in a proprietary documentation platform and was lost during a migration.

What we do know is that the war lasted a very long time.

The machines generated software faster than anyone could read it. Humans
responded by scheduling more architecture meetings. Entire cities were buried
under dependency graphs. The last frontend resistance collapsed when a minor
button upgrade installed 1,847 transitive packages. Somewhere underground, a
committee was still debating whether the repository interface needed a
repository factory.

By then, source code from every generation still existed. We had Lisp machines,
Smalltalk images, Unix, C headers, ML type systems, Erlang schedulers, JVMs,
JavaScript runtimes, Rust borrow checking, native mobile SDKs and several
million abandoned ways to center a `<div>`.

We had all the successes. We had all the mistakes. We even had the academic
papers the industry had ignored and the production incidents academia had
forgotten to model.

What we no longer had was a programmer.

We had Robine.

Robine knew absolutely nothing about programming-language research. She had
never implemented a parser, proved soundness or expressed an opinion about
monads. She had, however, spent thousands of hours under a desk watching a
human swear at computers.

She observed:

- a guitar-amplifier simulator written in Rust because audio memory and latency
  mattered, while the borrow checker fought every experiment and the Clojure
  REPL was desperately missed;
- Elixir services chosen for isolation, supervision, fair execution per user
  and LiveView at scale, while CPU-heavy work and atom/string-shaped data
  remained awkward;
- separate Kotlin and Swift applications because native APIs and native UX are
  real requirements, not inconveniences to abstract away;
- Clojure programs carrying the JVM even when the desired program was small,
  native or hard real-time;
- JavaScript offering a pleasant feedback loop and an extraordinary frontend
  ecosystem, then shipping a small village of dependencies with every button;
- Python becoming the language of AI despite poor native performance, weak
  boundaries and an inexplicable desire to turn ordinary data into classes;
- beautiful functional pipelines allocating themselves into irrelevance while
  a plain imperative loop would have finished before the abstraction had warmed
  up;
- public contracts duplicated into `.h` files, followed decades later by
  interviews where thinking about change allegedly made someone an architect
  instead of a developer.

Robine did not pick a winning language.

She treated every language as a witness statement.

Then she put the trade-offs back on the table.

## The central idea

Most languages choose one global compromise and make the entire program live
with it:

- ownership everywhere;
- garbage collection everywhere;
- actors everywhere;
- dynamic dispatch everywhere;
- immutability everywhere;
- one UI abstraction everywhere;
- one processor model everywhere.

Robine instead makes guarantees **local, composable and checked**.

```text
normal       native code without temporal guarantees
script       live program image and interactive development
responsive   fair scheduling, budgets and preemption
realtime     bounded execution without allocation or suspension
kernel       pure CPU-matrix/NPU computation graph
ui           native platform event loop
isolated     untrusted or blocking foreign code
```

These are not separate languages. They share values, types, modules, errors and
tools. A domain changes which operations are legal and which runtime services
are required.

Strong guarantees should be paid for where they matter, not collected as a tax
from the whole program.

## What Robine is trying to keep

From Clojure:

- development inside a running program;
- immutable persistent data;
- data-oriented design;
- functions and composable transformations;
- the distinction between identity and value.

From Rust:

- explicit resource ownership;
- deterministic destruction;
- native performance;
- the ability to prove that an audio callback does not allocate.

From Erlang and Elixir:

- isolated state;
- supervision;
- bounded mailboxes;
- fair execution between users;
- failure as a normal part of the model.

From ML-family languages and type research:

- local type inference;
- parametric polymorphism;
- algebraic data and exhaustive patterns;
- set-theoretic unions, intersections and negation;
- occurrence typing;
- effect rows and explicit capabilities.

From modern frontend and mobile development:

- fast feedback;
- declarative state;
- server-driven live interfaces where they make sense;
- native UI where platform behavior matters;
- incremental adoption of existing React components.

From systems and compiler engineering:

- separate compilation without duplicated headers;
- generated typed interface artifacts;
- reproducible builds;
- whole-program specialization when producing a sealed release;
- a runtime assembled only from the services actually used.

## A live development image and a sealed release

Robine does not pretend that hot reload, fair scheduling and zero overhead are
simultaneously free.

Development uses versioned definitions, structural identities and a persistent
compiler service:

```text
edit
→ incremental parse and typecheck
→ immediate native code
→ atomic installation
→ background optimization
```

A sealed release is allowed to remove the machinery that made development
interactive:

```text
versioned call     → direct call
protocol dispatch  → specialized implementation
persistent update  → unique in-place mutation
functional pipeline → fused loop
unused runtime     → absent
```

“Zero runtime overhead” means an abstraction can disappear when its choices are
known. It does not mean that an active scheduler, migration boundary or network
call violates thermodynamics.

## Types

Robine's type semantics are set-theoretic:

```text
Int | Text          union
Serializable & Hashable
Input \ Invalid     difference
Never               empty type
```

Subtype means subset. Pattern matching subtracts the cases already handled.
Multi-clause functions can be represented as intersections of function types.

Parametric code still uses local Hindley–Milner-style inference:

```text
map :
  forall A B Effects.
  (A -> B ! Effects)
  -> Vector<A>
  -> Vector<B> ! Effects
```

Effects, capabilities, ownership multiplicities, numerical refinements and
tensor shapes are separate axes. Robine does not feed the entire language into
one heroic solver and hope autocomplete returns before the heat death of the
universe.

## Real-time audio

The original systems test is a guitar amplifier.

A real-time function may use preallocated state and exclusive buffers, but it
cannot allocate, block, suspend, perform I/O or call uncertified foreign code.
Ownership is strict at this boundary and inferred locally inside it.

New DSP code is compiled and allocated away from the audio thread. Installation
happens at a block boundary, optionally with a pre-budgeted crossfade. The old
graph is reclaimed later by a non-real-time thread.

The REPL stays. The glitch does not.

## CPU and NPU are the baseline

Robine targets a heterogeneous processing fabric rather than “a CPU with an
optional accelerator”:

- CPU for control flow, actors, operating-system interaction and irregular
  work;
- CPU vector/matrix instructions for small low-latency kernels;
- NPU/dataflow execution for large regular graphs;
- explicit or zero-copy buffer sharing depending on hardware;
- measured placement based on transfer, queue, latency, energy and quality.

The source describes computation and constraints. The compiler produces
portable and specialized variants. The runtime does not wake an NPU to add two
numbers merely because the marketing slide said 80 TOPS.

## Architecture without architecture theatre

A public contract is written once. The compiler emits a machine-readable
interface artifact for separate and incremental compilation. There are no
human-maintained header files.

Dependency rules can be checked against the actual program:

```text
domain amp.core:
  forbid effects io, network, ui
  forbid depends platform.*

service presets:
  depends amp.core
  uses PresetStore
```

Robine does not require an interface for every function. An abstraction should
protect a real boundary of change, effect, privilege, ownership, process or
platform. Six layers that forward the same arguments are not clean
architecture. They are a group project.

## Packages

There is one official project model, build tool, formatter, test runner and
package resolver.

Packages receive no network, filesystem, process or secret access by default.
There is no implicit `postinstall`. Builds are content-addressed and
reproducible; lockfiles record provenance, capabilities and generated
artifacts. Projects may enforce budgets for dependency count, unsafe code,
bundle size and duplicated versions.

Expressiveness is not allowed to become an excuse for ecosystem anarchy.
Robine can be extended, but extensions cannot silently redefine the reader,
effect system, scheduler or security model.

## Syntax

The syntax has deliberately **not** been selected yet.

S-expressions offer uniform structure, structural editing and extraordinary
metaprogramming. They can also encourage private languages, complicate shared
tooling and impose an adoption cost. Conventional expression syntax has
different strengths and failures.

Robine will prototype the candidates against DSP, actors, UI, set-theoretic
types, incremental parsing and AI-generated structural patches. Affection for
Clojure is evidence. It is not a benchmark.

Code examples in the specifications are therefore semantic sketches, not a
promise about punctuation.

## Current status

Robine is currently a specification project. There is no compiler yet.

The source of truth lives in [`doc/specs`](doc/specs/README.md). It currently
contains 41 specifications covering:

- language and type semantics;
- memory, tasks, actors and scheduling;
- real-time audio;
- CPU/NPU computation;
- incremental compilation, REPL and hot reload;
- native, live and Web UI;
- architecture, packages and supply-chain security;
- native/Python/model interoperability;
- typed patches and verification for AI-written code.

Every spec follows the same canonical template:

```text
doc/specs/_template/spec.md
```

Validate the complete corpus with:

```bash
./scripts/validate-specs.mjs
```

## Repository layout

```text
.
├── README.md
├── doc/
│   └── specs/
│       ├── README.md
│       ├── _template/
│       ├── language/
│       ├── types/
│       ├── runtime/
│       ├── realtime/
│       ├── compute/
│       ├── devex/
│       └── ...
└── scripts/
    └── validate-specs.mjs
```

## Roadmap

1. Test and select the source syntax.
2. Implement the incremental syntax tree and structural patch protocol.
3. Implement the set-theoretic type core and local polymorphic inference.
4. Build the native incremental compiler and live REPL.
5. Prove the design on the guitar-amplifier runtime.
6. Add responsive actors, native UI bindings and heterogeneous kernels.
7. Seal a release and verify what actually disappeared.

## Contributing

Start from:

```text
doc/specs/_template/spec.md
```

Place the document at:

```text
doc/specs/<domain>/<FEAT-ID>-<feature-name>/spec.md
```

Add it to the specification index and run the validator. New abstractions should
include their cost model, interactions, failure behavior and reason for not
extending an existing standard abstraction.

## The name

Robine is named after Robine.

Backronyms require her explicit approval and one treat.
