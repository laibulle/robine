# <FEAT-ID> — <Titre concis>

- Statut : **Exploration**
- Version : **0.1.0**
- Domaine : `<domain>`

## Objet

Décrire en un paragraphe le comportement spécifié, le problème résolu et la
frontière couverte. Ne pas placer l’historique de discussion dans cette
section.

## Non-objectifs

Lister ce que la spec ne cherche pas à résoudre. Écrire « Aucun non-objectif
supplémentaire n’est déclaré à ce stade » lorsqu’il n’y en a pas.

## Spécification normative

Décrire le comportement avec les mots normatifs de META-001 : **DOIT**,
**NE DOIT PAS**, **DEVRAIT**, **NE DEVRAIT PAS** et **PEUT**.

Les sections propres à la fonctionnalité sont des sous-sections `###`.

### Règle ou concept

Définir une règle observable, sa sémantique statique ou dynamique et ses
limites.

### Exemple

```text
exemple minimal et indépendant de la syntaxe si LANG-002 reste ouvert
```

## Diagnostics et erreurs

Décrire les échecs observables, leur attribution et la qualité minimale des
diagnostics. Écrire explicitement qu’aucune exigence supplémentaire n’est
définie lorsque la section est sans objet.

## Sécurité, confidentialité et ressources

Documenter autorité, données sensibles, mémoire, temps CPU, énergie et bornes.
Ne conserver que les dimensions pertinentes.

## Interactions

Lister les identifiants des specs qui contraignent ou utilisent cette
fonctionnalité. Écrire explicitement qu’aucune interaction normative
supplémentaire n’est déclarée lorsqu’il n’y en a pas.

## Compatibilité et migration

Classer les changements selon META-001 et décrire versionnement, fallback,
upgrade ou absence de mécanisme supplémentaire.

## Tests de conformité

Lister les cas positifs, négatifs, limites et différentiels nécessaires avant
le passage à `Proposed`.

## Alternatives rejetées

Section facultative. Conserver uniquement les alternatives importantes et la
raison vérifiable de leur rejet. Supprimer le titre lorsqu’il n’y en a pas.

## Questions ouvertes

Lister les décisions non prises. Écrire « Aucune à ce stade » lorsqu’aucune
question n’est connue. Ne pas utiliser de marqueur temporaire.

<!--
Règles contrôlées par scripts/validate-specs.mjs :

1. chemin doc/specs/<domain>/<FEAT-ID>-<feat-name>/spec.md ;
2. titre H1 « <FEAT-ID> — <Titre> » ;
3. statut, version SemVer et domaine ;
4. les neuf sections H2 obligatoires ci-dessus, dans cet ordre ;
5. seules les Alternatives rejetées sont facultatives au niveau H2 ;
6. au moins une exigence normative vérifiable ;
7. identifiant unique, références connues et liens valides ;
8. présence dans doc/specs/README.md ;
9. aucun marqueur temporaire TODO, TBD, FIXME ou XXX.
-->
