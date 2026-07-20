# COMP-001 — Fabrique de calcul hétérogène

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `compute`

## Objet

Définir comme cible minimale une machine combinant contrôle généraliste et
calcul vectoriel, matriciel ou dataflow, sans exposer un fabricant dans le
modèle source.

## Non-objectifs

Le NPU n’exécute pas arbitrairement acteurs, sockets, GC ou appels système. La
fabrique hétérogène n’est pas présentée comme une mémoire uniforme lorsque le
matériel ne l’est pas.

## Spécification normative

### Profil matériel minimal

Une cible Robine Compute fournit logiquement :

- un CPU 64 bits pour contrôle, branches et système ;
- un moteur vectoriel ou matriciel proche du CPU ;
- un NPU ou moteur dataflow asynchrone ;
- des buffers partageables ou des transferts explicites ;
- files de commandes et événements ;
- au moins `f32`, `f16` ou `bf16`, et `i8` ;
- découverte des opérations, formes et précisions supportées.

`i4`, sparsité, mémoire cohérente et compteurs d’énergie sont des capacités
profilées, pas des hypothèses universelles.

### Rôles

Le CPU exécute :

- acteurs, I/O, logique irrégulière ;
- petits calculs et latence de lancement faible ;
- fallback de conformité.

Le moteur matriciel CPU exécute les graphes trop petits pour rentabiliser un
offload. Le NPU exécute les graphes purs, réguliers et suffisamment grands.

### Découverte

Le runtime expose une description abstraite :

```text
devices
operations
element types
shape limits
alignment
shared buffer modes
queue count
energy telemetry availability
```

Le code applicatif NE DEVRAIT PAS brancher sur un nom de produit. Un backend
spécifique peut être demandé dans un profil de déploiement ou de benchmark.

### Mémoire

Le partage zéro copie est préféré lorsqu’il préserve cohérence et sécurité. Une
transition nécessitant copie, conversion de layout ou synchronisation est
représentée dans le plan d’exécution et visible au profiler.

### Portabilité

Chaque opération du noyau portable DOIT posséder une implémentation de
référence CPU. Une opération sans cette implémentation appartient à une
extension profilée, pas au noyau portable ; elle DOIT déclarer sa capacité
requise et produire un diagnostic lorsque cette capacité manque.

Un artefact peut contenir plusieurs variantes, mais son comportement numérique
doit respecter COMP-004.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Un kernel ne peut accéder qu’aux buffers qui lui sont explicitement liés. Les
backends DOIVENT vérifier dimensions, offsets, alignements et durée de vie avant
dispatch.

## Interactions

- RUN-004 définit le domaine `kernel` ;
- DATA-002 définit espaces mémoire, layouts et transferts ;
- COMP-002 définit opérations et IR portables ;
- COMP-003 choisit les variantes et fallbacks ;
- COMP-004

## Compatibilité et migration

La version 0.2.0 sépare le noyau portable, qui exige un fallback CPU, des
extensions profilées. Une opération précédemment qualifiée de portable sans
référence CPU doit fournir cette référence ou changer de catégorie ; ce
changement est source-breaking pour son profil.

## Tests de conformité

La suite de conformité DOIT couvrir :

- chaque opération du noyau portable sur l’interprétation CPU ;
- rejet d’une opération profilée sans sa capacité ;
- fallback CPU d’un artefact multi-variantes ;
- validation des buffers et transferts visibles ;
- découverte sans dépendance au nom commercial du produit.

## Questions ouvertes

- Ensemble initial minimal des opérations du noyau portable.
