# PKG-001 — Projet, build et lockfile

- Statut : **Draft**
- Version : **0.2.0**
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

Le build distingue :

- le **payload canonique**, résultat non signé de la compilation ;
- l’**enveloppe de distribution**, qui peut contenir signature, notarisation,
  attestations et métadonnées du builder.

Avec source, lockfile, toolchain et profil identiques, le payload canonique
scellé DOIT être bit-à-bit identique. Les timestamps, chemins absolus et ordre
de filesystem NE DOIVENT PAS l’influencer.

L’enveloppe référence le hachage du payload. Elle n’est déclarée bit-à-bit
reproductible que si identité de signature, clés, horodatage, service de
notarisation et toutes ses autres entrées sont également fixés et
reproductibles. Une variation d’enveloppe NE DOIT PAS modifier le payload
qu’elle atteste.

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
- ARCH-001 fournit les artefacts d’interface ;
- CPL-001 produit le payload scellé ;
- DX-001 définit les caches reproductibles.

## Compatibilité et migration

La version 0.2.0 sépare payload canonique et enveloppe de distribution. Les
outils qui incorporaient signature ou identité du builder dans l’artefact
comparé bit-à-bit doivent publier deux empreintes ; ce changement est
ABI-breaking pour le format de publication.

## Tests de conformité

La suite de conformité DOIT couvrir :

- payload canonique identique depuis deux répertoires et builders ;
- enveloppes différentes attestant le même hachage de payload ;
- enveloppe reproductible lorsque toutes ses entrées sont fixées ;
- rejet d’une signature portant sur un autre payload ;
- résolution, features et publication déterministes.

## Questions ouvertes

- Format commun des enveloppes pour registres, notarisation Apple et
  signatures Android.
