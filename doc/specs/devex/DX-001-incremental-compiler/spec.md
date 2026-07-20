# DX-001 — Compilateur incrémental et compilation étagée

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `devex`

## Objet

Fournir une réponse interactive rapide sans imposer un interpréteur ou une VM
à la release.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Service de compilation

Le compilateur de développement est un service persistant qui conserve :

- arbres syntaxiques incrémentaux ;
- résolution de noms ;
- contraintes et types ;
- IR par définition ;
- graphe de dépendances ;
- caches de monomorphisation et codegen ;
- versions installées dans les processus connectés.

Chaque résultat est une requête pure ou explicitement dépendante d’une entrée
versionnée. Une modification invalide le sous-graphe minimal.

### Identités stables

Modules, définitions et nœuds significatifs possèdent des identités stables à
travers formatage et éditions locales. Une identité ne dépend pas uniquement de
la position en octets.

### Trois niveaux

#### Immédiat

Parse, typecheck et code natif local avec optimisations minimales. Cette version
est installable dès que ses contrats sont valides.

#### Chaud

En arrière-plan : spécialisation, fusion, vectorisation, inlining et codegen
plus coûteux. La version remplace l’immédiate à un point sûr.

#### Scellé

Build AOT reproductible avec analyse globale autorisée par les interfaces,
suppression des métadonnées de développement et runtime spécialisé.

Les trois niveaux DOIVENT préserver la même sémantique.

### Frontières d’invalidation

Une modification de corps avec interface identique :

- recompile la définition ;
- PEUT optimiser ses appelants chauds ;
- NE DOIT PAS les retyper.

Une modification d’interface invalide les consommateurs de cette interface
seulement. Les interfaces de dépendances sont chargées depuis ARCH-001.

### Macros et génération

Une macro structurelle, une dérivation ou un elaborator portable suit
LANG-004. Sa transformation est pure et mise en cache par empreinte de toutes
ses entrées sémantiques.

Une transformation qui exige filesystem, réseau, environnement, processus ou
outil externe est une tâche de build selon PKG-002, pas une macro. Elle produit
un artefact capturé et haché qui devient ensuite une entrée ordinaire de la
compilation incrémentale.

Une tâche de build hermétique PEUT être reproductible. L’environnement
hermétique, les capacités, entrées et sorties font partie de sa clé et de sa
provenance.

### Objectifs mesurables

Les budgets de latence sont définis par profil de dépôt et matériel, au minimum :

- édition locale vers diagnostic ;
- édition valide vers code immédiat ;
- warm build sans changement ;
- changement d’interface ;
- build scellé.

Le projet NE DOIT PAS revendiquer « compilation instantanée » sans publier ces
mesures.

### Cache

Les caches ne sont jamais sources de vérité. Leur corruption ou absence peut
ralentir, pas changer le résultat. Les artefacts distants sont vérifiés selon
PKG-002.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- ARCH-001
- LANG-004
- PKG-002

## Compatibilité et migration

La version 0.2.0 sépare les transformations pures de LANG-004 des tâches de
build de PKG-002. Une ancienne macro qui effectue une I/O de compilation doit
devenir une tâche de build déclarée, puis consommer son artefact haché. Ce
changement est source-breaking pour ces macros et compatible pour les
transformations déjà pures.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
