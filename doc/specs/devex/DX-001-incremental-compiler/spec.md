# DX-001 — Compilateur incrémental et compilation étagée

- Statut : **Draft**
- Version : **0.1.0**
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

Une transformation de compilation pure est cachée par empreinte de ses entrées.
Une transformation avec effets déclare ses capacités et rend le build
non reproductible sauf environnement hermétique capturé.

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
- PKG-002

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
