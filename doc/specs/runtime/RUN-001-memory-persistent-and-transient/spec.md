# RUN-001 — Mémoire, valeurs persistantes et transients

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `runtime`

## Objet

Définir une mémoire automatique pour le code ordinaire, prédictible pour les
sections critiques et interdite lorsqu’un domaine temps réel l’exige.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Modèles disponibles

Le runtime peut employer :

- valeurs immédiates et stack ;
- ownership et déplacement ;
- comptage de références optimisé ;
- régions et arènes ;
- heap local d’acteur avec collecte incrémentale ;
- graphes explicitement traçables pour structures cycliques.

Le choix d’implémentation NE DOIT PAS modifier l’identité observable d’une
valeur immuable.

### Collections persistantes

`Vector<T>`, `Map<K,V>` et `Set<T>` sont immuables et persistants. Une
modification logique produit une nouvelle valeur et PEUT partager sa structure.

Les garanties asymptotiques minimales et les facteurs de copie DEVRAIENT être
publiés par la bibliothèque standard.

### Transients

Une collection persistante peut devenir `transient` lorsque l’appelant possède
un accès unique :

```text
transient v = Vector.capacity(n)
mutate v { ... }
result = freeze(v)
```

Après `freeze`, la valeur transiente est consommée. Avant `freeze`, elle NE PEUT
PAS être partagée, capturée ou observée par un autre domaine.

Le compilateur DEVRAIT abaisser les opérations transientes vers des buffers
contigus et des boucles impératives.

### Cycles

Les valeurs persistantes ordinaires NE DOIVENT PAS former de cycle implicite.
Un graphe cyclique utilise un type `Graph` ou une arène traçable explicite. La
durée de vie du graphe est celle de son propriétaire ou de sa région.

### Allocation et domaines

Chaque fonction expose l’effet `Allocate` lorsque l’allocation ne peut pas être
prouvée absente ou déplacée hors de son domaine.

`realtime` interdit `Allocate`. `responsive` autorise l’allocation mais exige
des limites par acteur ou tâche. `kernel` utilise des buffers préparés par son
appelant ou son runtime de calcul.

### Destructeurs

La libération d’une ressource externe est déterministe. Un destructeur NE DOIT
PAS bloquer, suspendre ou lever une erreur. Les opérations coûteuses utilisent
une méthode explicite comme `close_async`.

### Inspection

L’outil mémoire DOIT pouvoir expliquer pour une valeur :

- emplacement probable ;
- propriétaire ;
- nombre de copies conservées ;
- raison d’un partage ou d’une allocation ;
- domaine qui effectuera la libération.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

Aucune interaction normative supplémentaire n’est déclarée.

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

Les tests couvrent partage structurel, consommation des transients, cycles
explicites, absence d’allocation dans un chemin prouvé et destruction
déterministe des ressources.

## Questions ouvertes

Aucune à ce stade.
