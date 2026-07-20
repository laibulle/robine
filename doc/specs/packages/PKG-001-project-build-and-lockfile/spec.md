# PKG-001 — Projet, build et lockfile

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `packages`

## Objet

Fournir une chaîne officielle unique pour projet, dépendances, tests, build et
publication.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Projet

Un manifeste déclare :

- identité et version ;
- modules source ;
- cibles et profils ;
- dépendances ;
- capacités maximales ;
- politiques d’architecture ;
- budgets de build, bundle et ressources ;
- features explicitement activables.

Le format est canonique et validé par schéma.

### Commandes

Le toolchain standard couvre au minimum :

```text
new
check
run
test
bench
prove
fmt
doc
build
publish
api diff
```

Un package NE DEVRAIT PAS exiger un second système de build pour du code
Robine ordinaire.

### Résolution

Les dépendances sont résolues de manière déterministe à partir de contraintes
et d’un registre choisi. Le lockfile enregistre :

- identité exacte et provenance ;
- hachage du contenu ;
- features ;
- dépendances transitives ;
- artefacts générés ;
- version de langage et outils pertinents ;
- permissions déclarées.

### Reproductibilité

Avec source, lockfile, toolchain et profil identiques, le build scellé DOIT
produire un artefact bit-à-bit identique, hors champs explicitement exclus et
normalisés.

Les timestamps, chemins absolus et ordre de filesystem ne doivent pas influencer
l’artefact.

### Monorepo et workspaces

Un workspace partage résolution et caches sans fusionner les interfaces
publiques. Les dépendances entre membres utilisent les mêmes artefacts
d’interface que les packages externes.

### Features

Une feature est additive et nommée. Deux features incompatibles produisent un
diagnostic au lieu d’un comportement dépendant de l’ordre.

### Publication

Avant publication :

- tests et politiques demandés passent ;
- interface publique est extraite ;
- licences et provenance sont présentes ;
- source correspond à l’artefact ;
- capacités et code généré sont déclarés.

### Builds natifs

Un build script externe est une capacité exceptionnelle, hermétique et
entièrement déclaré selon PKG-002.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- PKG-002

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
