# META-001 — Processus de spécification et conformité

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `meta`

## Objet

Définir la forme, le cycle de vie et les critères de conformité des
spécifications Robine.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### États

Une spec possède exactement un état :

- **Exploration** : problème cadré, solutions encore concurrentes ;
- **Draft** : proposition suffisamment précise pour prototypage ;
- **Proposed** : prototype et tests de conformité disponibles ;
- **Accepted** : décision normative intégrée à une version du langage ;
- **Deprecated** : maintenue uniquement pour compatibilité ;
- **Rejected** : conservée comme historique, non implémentable.

Un document NE DOIT PAS passer à `Accepted` sans :

1. sémantique statique et dynamique définie ;
2. diagnostics attendus pour les principaux échecs ;
3. tests positifs et négatifs ;
4. analyse de coût de compilation et d’exécution ;
5. interaction documentée avec les autres domaines ;
6. stratégie de compatibilité ou justification de son absence.

### Structure d’une spec

Chaque fonctionnalité vit dans :

```text
doc/specs/<domain>/<FEAT-ID>-<feat-name>/spec.md
```

Elle DOIT partir du gabarit canonique :

```text
doc/specs/_template/spec.md
```

Le validateur est exécuté avec :

```text
node scripts/validate-specs.mjs
```

Une spec DOIT contenir :

- identité, titre, statut, version et domaine ;
- les neuf sections obligatoires du gabarit, dans le même ordre ;
- uniquement des sous-sections métier de niveau trois sous
  `Spécification normative` ;
- au moins une exigence normative vérifiable ;
- des liens et références vers des specs connues ;
- une entrée dans l’index.

`Alternatives rejetées` est la seule section de niveau deux facultative. Les
sections sans exigence spécifique l’indiquent explicitement au lieu de
disparaître.

Des fichiers `examples/`, `tests/`, `model/` et `rationale.md` PEUVENT être
ajoutés dans le même répertoire.

### Conformité d’une implémentation

Une implémentation conforme DOIT publier :

- les specs `Accepted` qu’elle implémente ;
- ses extensions ;
- les limites de ressources dépendantes de la cible ;
- les comportements `implementation-defined` ;
- les échecs de conformité connus.

Une extension NE DOIT PAS modifier la signification d’un programme conforme.
Elle DOIT être activée explicitement dans le manifeste de projet.

### Exemples et syntaxe

Tant que LANG-002 n’est pas `Accepted`, la syntaxe des exemples est
illustrative. La sémantique exprimée par le texte normatif prime.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- LANG-002

## Compatibilité et migration

Une modification est classée :

- **éditoriale** : aucun comportement modifié ;
- **compatible** : élargit les programmes acceptés sans changer les résultats ;
- **source-breaking** : exige une modification du source ;
- **ABI-breaking** : invalide un artefact compilé ;
- **semantic-breaking** : change le résultat d’un programme valide.

Toute modification autre qu’éditoriale DOIT apparaître dans l’historique du
document et recevoir une nouvelle version.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

- Format machine-readable du catalogue de specs.
- Gouvernance des changements `Accepted`.
- Durée minimale de dépréciation avant suppression.
