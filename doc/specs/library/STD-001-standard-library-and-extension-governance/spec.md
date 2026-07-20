# STD-001 — Bibliothèque standard et gouvernance des extensions

- Statut : **Draft**
- Version : **0.2.0**
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
- `Option`, `Result`, `TaskOutcome` et erreurs ;
- tâches, acteurs, dispositions de livraison, flux et annulation ;
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
- RUN-002 définit `TaskOutcome` ;
- RUN-003 définit les dispositions de livraison ;
- TYPE-003 définit les règles de cohérence des protocoles.

## Compatibilité et migration

La version 0.2.0 ajoute `TaskOutcome` et les dispositions de livraison au
vocabulaire standard. Une bibliothèque qui encodait annulation ou saturation
dans une erreur métier doit migrer vers ces types ; ce changement est
source-breaking.

## Tests de conformité

La suite de conformité DOIT couvrir :

- interopérabilité de `TaskOutcome` entre modules ;
- dispositions de livraison communes aux acteurs ;
- modularité et élimination des parties inutilisées ;
- cohérence des extensions et absence d’autorité ambiante ;
- publication des coûts, effets et limites de cible.

## Questions ouvertes

- Placement exact de `TaskOutcome` et `DeliveryDisposition` dans les modules
  standards.
