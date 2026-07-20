# DX-004 — Diagnostics, inspection, tests et preuves

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `devex`

## Objet

Unifier l’explication du compilateur et les différents niveaux de validation,
sans faire passer un test pour une preuve ou une estimation pour une mesure.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Commandes d’explication

Le toolchain fournit au minimum :

```text
explain type
explain effect
explain capability
explain domain
explain allocation
explain dispatch
explain placement
explain dependency
explain realtime
```

Une explication relie toujours la décision à la source et à une règle de spec.

### Exemples et tests

- `example` vérifie un scénario nommé ;
- `test` vérifie une assertion déterministe ;
- `property` génère ou quantifie des entrées ;
- `model` explore une machine d’états bornée ;
- `prove` tente une obligation logique ;
- `bench` mesure un profil.

Chaque résultat indique sa nature.

### Propriétés

Le framework de propriétés conserve seed, générateur, shrink path et entrée
minimale. Les générateurs dérivés des types respectent leurs raffinements ou
signalent les cas qu’ils ne savent pas produire.

### Modèles

Les protocoles d’acteurs, migrations et workflows peuvent déclarer états,
transitions et interdictions. L’exploration signale deadlocks, états
inaccessibles, transitions non couvertes et violations de borne.

### Preuves et contrôles runtime

Une obligation possède l’état :

- `proved`;
- `checked-runtime`;
- `assumed(reason)`;
- `unresolved`;
- `disproved(counterexample)`.

Une release définit quels états sont admissibles par domaine. `realtime` refuse
les contrôles non bornés ; la sécurité peut refuser toute hypothèse.

### Différentiel

Les kernels et backends numériques DEVRAIENT être comparés à l’interprétation
de référence selon COMP-004.

## Diagnostics et erreurs

Un diagnostic possède :

- code stable ;
- message orienté domaine ;
- emplacement primaire et causes ;
- chaîne de dépendance ou d’effet pertinente ;
- correctifs structurés lorsque sûrs ;
- représentation machine pour éditeurs et IA.

Les détails de solveur sont disponibles à la demande mais ne constituent pas
le message principal.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- DX-001
- COMP-004
- TYPE-004 distingue effets et capacités ;
- RUN-004 définit domaines et admission.

## Compatibilité et migration

La version 0.2.0 ajoute les explications distinctes de capacité et de domaine.
Les clients qui les présentaient comme effets doivent migrer leurs codes et
structures de diagnostic ; ce changement est ABI-breaking pour le protocole.

## Tests de conformité

DX-001 maintient le graphe reliant définitions, specs, exemples et tests. Une
modification peut sélectionner les validations directement ou transitivement
affectées, sans prétendre remplacer une suite complète périodique. La suite
DOIT vérifier qu’un même appel affiche séparément effet, autorité requise,
domaine et règle d’admission.

## Questions ouvertes

- Taxonomie stable des codes partagés entre diagnostics de type, domaine et
  politique.
