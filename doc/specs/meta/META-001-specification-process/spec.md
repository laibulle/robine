# META-001 — Processus de spécification et conformité

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `meta`

## Objet

Définir la forme, le cycle de vie et les critères de conformité des
spécifications Robine.

## États

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

## Structure d’une spec

Chaque fonctionnalité vit dans :

```text
doc/specs/<domain>/<FEAT-ID>-<feat-name>/spec.md
```

Elle DEVRAIT contenir :

- objet et non-objectifs ;
- terminologie ;
- comportement normatif ;
- exemples non ambigus ;
- erreurs et diagnostics ;
- implications sur le runtime et les outils ;
- tests de conformité ;
- questions ouvertes.

Des fichiers `examples/`, `tests/`, `model/` et `rationale.md` PEUVENT être
ajoutés dans le même répertoire.

## Compatibilité

Une modification est classée :

- **éditoriale** : aucun comportement modifié ;
- **compatible** : élargit les programmes acceptés sans changer les résultats ;
- **source-breaking** : exige une modification du source ;
- **ABI-breaking** : invalide un artefact compilé ;
- **semantic-breaking** : change le résultat d’un programme valide.

Toute modification autre qu’éditoriale DOIT apparaître dans l’historique du
document et recevoir une nouvelle version.

## Conformité d’une implémentation

Une implémentation conforme DOIT publier :

- les specs `Accepted` qu’elle implémente ;
- ses extensions ;
- les limites de ressources dépendantes de la cible ;
- les comportements `implementation-defined` ;
- les échecs de conformité connus.

Une extension NE DOIT PAS modifier la signification d’un programme conforme.
Elle DOIT être activée explicitement dans le manifeste de projet.

## Exemples et syntaxe

Tant que LANG-002 n’est pas `Accepted`, la syntaxe des exemples est
illustrative. La sémantique exprimée par le texte normatif prime.

## Questions ouvertes

- Format machine-readable du catalogue de specs.
- Gouvernance des changements `Accepted`.
- Durée minimale de dépréciation avant suppression.
