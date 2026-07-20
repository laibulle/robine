# CPL-001 — Pipeline de compilation, scellement et cibles

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `compiler`

## Objet

Définir les représentations et validations qui relient le langage aux cibles
natives, WebAssembly et CPU/NPU.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Étages

```text
source
→ arbre syntaxique stable
→ noms résolus
→ HIR typée (ensembles, effets, ownership)
→ Core fonctionnel explicite
→ IR de domaines
→ SSA et mémoire
→ IR backend
→ objet natif, Wasm ou graphe accélérateur
```

Chaque transition possède un vérificateur. Un backend NE DOIT PAS recevoir une
IR dont les invariants de l’étage précédent n’ont pas été validés.

### HIR

La HIR conserve :

- types normalisés ;
- patterns et couvertures ;
- lignes d’effets ;
- multiplicité et borrows ;
- contrats et obligations ;
- identités source.

Elle est sérialisable pour caches et outils, avec version stricte.

### Core

Le Core rend explicites contrôle, handlers d’effets, captures, appels de
protocoles et mutations uniques. Il ne contient plus de sucre syntaxique.

Les domaines abaissent ensuite :

- acteurs vers machines d’état et messages ;
- tâches vers continuations structurées ;
- realtime vers appels bornés ;
- kernels vers COMP-002 ;
- UI vers backend de plateforme.

### Backends

Les cibles initiales sont :

- natif via une infrastructure de codegen rapide en développement ;
- natif optimisé pour build scellé ;
- WebAssembly/WASI pour portabilité ;
- IR tensorielle portable et backends NPU.

Un backend publie ABI, modèle mémoire, précisions numériques et extensions.

### Scellement

Le build scellé fixe composition, features et profils. Il peut :

- résoudre appels versionnés ;
- dévirtualiser protocoles ;
- monomorphiser ;
- fusionner pipelines ;
- supprimer métadonnées REPL ;
- sélectionner runtime minimal ;
- produire plusieurs variantes de calcul.

Il NE DOIT PAS supprimer une frontière d’upgrade explicitement conservée.

### Bootstrap

Le premier compilateur peut être écrit dans un autre langage. L’auto-hébergement
ultérieur conserve une chaîne de bootstrap reproductible et auditée, capable de
reconstruire chaque étape depuis une base publiée.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- COMP-002

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

Des tests différentiels comparent niveaux immédiat, chaud et scellé. Les IR
possèdent round-trip, fuzzing de parse/désérialisation et vérification
d’invariants.

## Questions ouvertes

Aucune à ce stade.
