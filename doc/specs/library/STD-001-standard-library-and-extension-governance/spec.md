# STD-001 — Bibliothèque standard et gouvernance des extensions

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `library`

## Objet

Éviter que l’expressivité du langage produise plusieurs abstractions
incompatibles pour les mêmes besoins fondamentaux.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Surface standard

La distribution officielle possède une solution canonique pour :

- valeurs, collections et itération ;
- texte, octets, temps et unités ;
- `Option`, `Result` et erreurs ;
- tâches, acteurs, flux et annulation ;
- I/O, réseau et sérialisation ;
- test, propriétés et benchmarks ;
- tenseurs et calcul portable ;
- capacités de plateforme essentielles.

Canonique ne signifie pas que toute bibliothèque tierce est interdite ; cela
signifie que les APIs publiques de l’écosystème peuvent partager un vocabulaire
de base.

### Taille

La bibliothèque est modulaire. Importer une abstraction n’embarque pas toutes
les autres. Les parties inutilisées sont éliminables lors du scellement.

### Stabilité

Les modules suivent ARCH-001 et ARCH-003. Les protocoles fondamentaux évoluent
plus lentement que leurs implémentations. Une nouvelle abstraction concurrente
doit expliquer pourquoi l’existante ne peut être étendue.

### Extensions

Une extension peut ajouter :

- implémentation de protocole ;
- backend ;
- format ;
- adaptateur ;
- algorithme ;
- type métier.

Elle NE PEUT PAS :

- modifier le reader global ;
- redéfinir la résolution des noms ;
- changer la signification d’un effet standard ;
- introduire une autorité ambiante ;
- remplacer silencieusement scheduler ou allocateur.

### Incubation

Les nouvelles abstractions passent par package expérimental, mesures et usages
réels avant intégration standard. Plusieurs candidats peuvent exister pendant
l’exploration, mais une intégration officielle exige convergence, documentation
et plan de migration.

### Qualité

Un module standard publie :

- complexités et allocations ;
- effets ;
- comportement de concurrence ;
- limites de sécurité ;
- tests de conformité ;
- benchmarks représentatifs ;
- compatibilité de cible.

### But social

La facilité à construire un DSL ou un framework ne doit pas remplacer le
travail de coordination. Le langage rend l’extension possible ; la bibliothèque
standard rend la coopération économique.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- ARCH-001
- ARCH-003

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
