# Spécifications de Robine

Ce répertoire décrit Robine, un langage et une chaîne d’exécution conçus pour
faire cohabiter développement interactif, code natif, services équitables,
audio temps réel, interfaces natives et calcul CPU/NPU.

Les documents sont des brouillons d’ingénierie. Ils distinguent volontairement
les décisions prises, les propositions et les questions encore ouvertes. Les
exemples utilisent une syntaxe de travail non normative tant que
[LANG-002](language/LANG-002-source-forms-and-syntax/spec.md) reste ouvert.

## Principes normatifs

Les mots **DOIT**, **NE DOIT PAS**, **DEVRAIT**, **NE DEVRAIT PAS** et **PEUT**
ont un sens normatif :

- **DOIT** et **NE DOIT PAS** définissent une exigence de conformité ;
- **DEVRAIT** et **NE DEVRAIT PAS** autorisent une déviation documentée ;
- **PEUT** décrit un comportement facultatif.

## Catalogue

| Domaine | Spécifications |
|---|---|
| Méta | [META-001 — Processus de spécification](meta/META-001-specification-process/spec.md) |
| Langage | [LANG-001 — Principes](language/LANG-001-design-principles/spec.md) · [LANG-002 — Formes source](language/LANG-002-source-forms-and-syntax/spec.md) · [LANG-003 — Valeurs, expressions et modules](language/LANG-003-values-expressions-patterns-modules/spec.md) · [LANG-004 — Macros, dérivation et staging](language/LANG-004-hygienic-macros-staging-and-derivation/spec.md) · [LANG-005 — Programmation data-first](language/LANG-005-data-first-programming-and-explicit-identity/spec.md) |
| Types | [TYPE-001 — Types ensemblistes](types/TYPE-001-set-theoretic-types/spec.md) · [TYPE-002 — Polymorphisme et inférence](types/TYPE-002-polymorphism-and-inference/spec.md) · [TYPE-003 — Records, variantes et protocoles](types/TYPE-003-records-variants-and-protocols/spec.md) · [TYPE-004 — Effets et capacités](types/TYPE-004-effects-and-capabilities/spec.md) · [TYPE-005 — Ownership, raffinements et formes](types/TYPE-005-ownership-refinements-and-shapes/spec.md) |
| Données | [DATA-001 — Sérialisation et évolution de schémas](data/DATA-001-serialization-and-schema-evolution/spec.md) · [DATA-002 — Données logiques et layouts physiques](data/DATA-002-logical-data-and-physical-layout/spec.md) |
| Runtime | [RUN-001 — Mémoire et collections](runtime/RUN-001-memory-persistent-and-transient/spec.md) · [RUN-002 — Tâches structurées](runtime/RUN-002-tasks-and-structured-concurrency/spec.md) · [RUN-003 — Acteurs et équité](runtime/RUN-003-actors-fairness-and-backpressure/spec.md) · [RUN-004 — Domaines d’exécution](runtime/RUN-004-execution-domains-and-scheduler/spec.md) · [RUN-005 — Runtime synthétisé](runtime/RUN-005-synthesized-runtime-and-selective-preemption/spec.md) |
| Temps réel | [RT-001 — Audio temps réel](realtime/RT-001-realtime-audio/spec.md) · [RT-002 — Communication et échange de graphe](realtime/RT-002-lock-free-graph-swap/spec.md) |
| Calcul | [COMP-001 — Fabrique hétérogène](compute/COMP-001-heterogeneous-processing-fabric/spec.md) · [COMP-002 — Tenseurs, kernels et IR](compute/COMP-002-tensors-kernels-and-ir/spec.md) · [COMP-003 — Placement et spécialisation](compute/COMP-003-placement-specialization-and-fallback/spec.md) · [COMP-004 — Précision, énergie et qualité](compute/COMP-004-precision-energy-and-quality/spec.md) |
| Compilateur | [CPL-001 — Pipeline, scellement et cibles](compiler/CPL-001-pipeline-sealing-and-targets/spec.md) |
| Expérience développeur | [DX-001 — Compilation incrémentale](devex/DX-001-incremental-compiler/spec.md) · [DX-002 — REPL vivant](devex/DX-002-live-repl/spec.md) · [DX-003 — Hot reload et migration](devex/DX-003-hot-reload-and-migration/spec.md) · [DX-004 — Diagnostics, tests et preuves](devex/DX-004-diagnostics-testing-and-proofs/spec.md) |
| Outillage | [TOOL-001 — Service de langage et protocole structurel](tooling/TOOL-001-language-service-and-structural-protocol/spec.md) · [TOOL-002 — Adaptateurs éditeur et LSP](tooling/TOOL-002-editor-adapters-and-lsp/spec.md) |
| Interfaces | [UI-001 — UI native](ui/UI-001-platform-native-ui/spec.md) · [UI-002 — Live UI](ui/UI-002-live-ui/spec.md) · [UI-003 — Web et îlots React](ui/UI-003-web-and-react-islands/spec.md) |
| Architecture | [ARCH-001 — Contrats publics](architecture/ARCH-001-public-contracts/spec.md) · [ARCH-002 — Politiques de dépendance](architecture/ARCH-002-dependency-policies/spec.md) · [ARCH-003 — Adaptateurs et évolution](architecture/ARCH-003-adapters-and-api-evolution/spec.md) |
| Réseau | [NET-001 — Protocoles typés et distribution](network/NET-001-typed-protocols-and-distribution/spec.md) |
| Bibliothèque | [STD-001 — Bibliothèque standard et gouvernance](library/STD-001-standard-library-and-extension-governance/spec.md) |
| Paquets | [PKG-001 — Projet, build et lockfile](packages/PKG-001-project-build-and-lockfile/spec.md) · [PKG-002 — Sécurité de la chaîne logicielle](packages/PKG-002-security-and-supply-chain/spec.md) · [PKG-003 — Applications et frontière hôte](packages/PKG-003-applications-and-host-boundary/spec.md) |
| Interopérabilité | [FFI-001 — ABI natives et plateformes](interop/FFI-001-native-and-platform-abi/spec.md) · [FFI-002 — Python et modèles](interop/FFI-002-python-and-model-interop/spec.md) · [FFI-003 — Objets Swift et Kotlin](interop/FFI-003-swift-kotlin-object-interop/spec.md) |
| IA | [AI-001 — Patches structurels typés](ai/AI-001-typed-structural-patches/spec.md) · [AI-002 — Trous, provenance et vérification](ai/AI-002-holes-provenance-and-verification/spec.md) |

## Couverture des contraintes fondatrices

| Contrainte observée | Réponse principale |
|---|---|
| Rust et audio : mémoire sûre sans combattre le borrow checker partout | TYPE-005, RT-001, RUN-001 |
| Clojure : REPL vivant et métaprogrammation sans JVM imposée | LANG-004, DX-001, DX-002, DX-003 |
| Elixir : équité entre utilisateurs et LiveView à l’échelle | RUN-003, RUN-004, RUN-005, UI-002 |
| Kotlin/Swift : accès, objets et UX réellement natifs | UI-001, FFI-001, FFI-003 |
| JavaScript : excellente boucle de dev, dépendances et bundles incontrôlés | DX-001, PKG-002, UI-003 |
| Python : écosystème IA sans modèle d’exécution Python obligatoire | COMP-002, FFI-002 |
| FP/OOP : données par défaut, identité explicite et mutation locale efficace | LANG-005, RUN-001, TYPE-005 |
| Data-oriented design : layouts AoS/SoA, SIMD et transferts visibles | DATA-002, COMP-002, COMP-003 |
| C/C++ : contrats sans duplication de headers | ARCH-001 |
| CPU/NPU : calcul hétérogène sobre comme cible minimale | COMP-001 à COMP-004 |

## Questions structurantes encore ouvertes

1. La syntaxe source canonique est-elle une syntaxe S-expression, une syntaxe
   conventionnelle orientée expressions, ou une troisième forme ?
2. Jusqu’où exposer la négation et les intersections dans les annotations
   utilisateur sans dégrader les diagnostics ?
3. Quel sous-ensemble minimal de raffinements possède une preuve statique
   obligatoire, plutôt qu’un contrôle d’exécution ?
4. Quelle ABI publique doit être stable dès la première version ?

Ces questions sont des entrées du processus de spécification, pas des omissions
à masquer.

## Créer ou modifier une spec

Copier `doc/specs/_template/spec.md` vers
`doc/specs/<domain>/<FEAT-ID>-<feat-name>/spec.md`, remplir les sections utiles,
ajouter la spec au catalogue puis exécuter :

```text
node scripts/validate-specs.mjs
```

Le validateur contrôle structure, métadonnées, identifiants, exigences
normatives, références croisées, liens et présence dans cet index.
