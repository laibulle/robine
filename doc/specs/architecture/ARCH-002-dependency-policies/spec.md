# ARCH-002 — Politiques de dépendance exécutables

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `architecture`

## Objet

Transformer les décisions d’architecture importantes en contraintes vérifiées
sur le graphe réel des imports, effets et capacités.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Politiques

Une architecture peut :

- autoriser ou interdire une dépendance entre groupes ;
- interdire un effet dans un domaine ;
- limiter les packages externes ;
- imposer qu’une capacité soit fournie par un adaptateur ;
- interdire un cycle ;
- limiter taille de bundle ou surface publique ;
- définir propriétaires et visibilité de données.

### Graphe

Les nœuds sont modules, packages, capacités, ressources et artefacts de
plateforme. Les arêtes distinguent :

- import de type ;
- appel de valeur ;
- effet ;
- implémentation de protocole ;
- génération de code ;
- FFI ;
- ressource au runtime.

Une politique sur les dépendances de valeurs ne doit pas bloquer par erreur un
simple import de type, sauf demande explicite.

### Exemple conceptuel

```text
domain amp.core:
    forbid effects io, network, ui
    forbid depends platform.*

service presets:
    depends amp.core
    uses PresetStore
```

Si une chaîne transitive viole la politique, le diagnostic montre le chemin
minimal complet.

### Absence de théâtre architectural

Les politiques sont optionnelles et ciblées. Le langage NE DOIT PAS imposer un
nombre de couches ou une nomenclature Clean Architecture.

L’outil d’inspection signale les frontières transparentes qui :

- ne changent ni type, ni effet, ni ownership ;
- n’ont qu’une implémentation ;
- ne protègent aucune politique ;
- transmettent seulement leurs arguments.

Il peut proposer une simplification mais NE modifie pas automatiquement
l’architecture.

### Exceptions

Une dérogation comporte motif, portée, propriétaire et échéance. Elle est
versionnée et visible dans les rapports ; un commentaire générique ne suffit
pas.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

Aucune interaction normative supplémentaire n’est déclarée.

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

Les politiques font partie du check normal et du gate de release. Les tests
doivent couvrir dépendances directes, transitives, génération, FFI et capacités
cachées.

## Questions ouvertes

Aucune à ce stade.
