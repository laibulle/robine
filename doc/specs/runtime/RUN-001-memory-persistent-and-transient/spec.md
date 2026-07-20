# RUN-001 — Mémoire, valeurs persistantes et transients

- Statut : **Draft**
- Version : **0.2.0**
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

Le choix d’implémentation NE DOIT PAS modifier l’égalité, le contenu ou le
comportement observable d’une valeur immuable. Une valeur sans contrat
d’identité selon LANG-005 n’acquiert aucune identité observable du seul fait de
son placement mémoire.

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

Découper, indexer ou avancer de manière bornée dans un buffer ou une arène
entièrement préallouée n’est pas une allocation dynamique lorsque cette
opération :

- ne crée pas de nouvelle durée de vie indépendante ;
- ne peut ni agrandir ni remplacer le stockage ;
- possède un coût maximal vérifié ;
- reste une mutation de l’état unique préparé.

Cette opération porte l’effet d’état borné approprié, pas `Allocate`. Une
réservation qui peut épuiser puis agrandir le stockage, déclencher une
récupération ou différer une libération conserve `Allocate` et reste interdite
dans `realtime`.

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

- TYPE-004 définit `Allocate` comme effet de ressource ;
- TYPE-005 définit ownership, déplacements et borrows ;
- LANG-005 distingue valeur, identité et ressource ;
- RUN-004 applique les restrictions par domaine ;
- RUN-005 attribue les allocations et récupérations locales d’acteur ;
- RT-001 définit l’admission du chemin audio ;
- DATA-002 définit buffers, vues, layouts et matérialisations.

## Compatibilité et migration

La version 0.2.0 remplace la notion erronée d’identité observable d’une valeur
immuable par son comportement observable et distingue allocation dynamique de
réservation bornée dans un stockage préalloué. Ce changement est compatible
pour le code qui exposait déjà `Allocate` correctement et source-breaking pour
les APIs temps réel qui qualifiaient une croissance ou récupération de bornée.

## Tests de conformité

Les tests couvrent partage structurel, consommation des transients, cycles
explicites, absence d’allocation dans un chemin prouvé et destruction
déterministe des ressources. Ils distinguent également :

- égalité d’une valeur immuable sous plusieurs placements ;
- découpe bornée d’un buffer préalloué sans `Allocate` ;
- croissance ou récupération conservant `Allocate` ;
- rejet de cette croissance depuis `realtime`.

## Questions ouvertes

- Ensemble standard des effets d’état bornés pour les arènes préallouées.
