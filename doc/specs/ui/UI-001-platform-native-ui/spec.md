# UI-001 — Interfaces réellement natives par plateforme

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `ui`

## Objet

Partager le domaine et les machines d’état sans imposer une UI au plus petit
dénominateur commun entre iOS, Android, Web et desktop.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Séparation

Une fonctionnalité peut définir un contrat d’écran :

```text
state
events
commands
navigation outcomes
accessibility semantics
```

Chaque plateforme fournit sa vue, sa navigation et ses interactions natives.
Le compilateur vérifie que les événements obligatoires sont gérés et que les
sorties respectent le contrat.

### Code partagé

Peuvent être partagés :

- types et validations du domaine ;
- client réseau et stockage portable ;
- reducers et machines d’état ;
- orchestration de use cases ;
- tests de comportement ;
- ressources indépendantes de plateforme.

Ne sont pas présumés partageables :

- hiérarchie de vues ;
- navigation ;
- gestes ;
- typographie et densité ;
- permissions ;
- lifecycle ;
- conventions d’accessibilité ;
- APIs de plateforme.

### Bindings

Le backend iOS accède aux SDK Apple et ABI Objective-C/Swift compatibles. Le
backend Android accède aux API Android et à Compose via les formats requis par
la plateforme.

Les wrappers générés DOIVENT préserver nullabilité, ownership, threading et
disponibilité de version.

### Boucle UI

Les mises à jour de vue s’exécutent sur l’exécuteur UI de la plateforme. Un
travail bloquant ou intensif DOIT devenir une tâche ou une transition explicite
vers un exécuteur admissible. `Blocking` est un effet de TYPE-004, pas une
capacité. Un appel portant cet effet directement depuis `ui` est rejeté selon
RUN-004. Lorsque l’appel est étranger, FFI-001 impose en plus son isolation ;
le diagnostic indique la capacité éventuellement requise par l’opération
séparée.

### Non-objectif

Robine ne promet pas « écrire une UI une fois ». Elle promet de ne réécrire que
la partie dont la différence est réellement nécessaire.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- TYPE-004 définit l’effet `Blocking` et les capacités ;
- RUN-002 définit les tâches utilisées pour quitter la boucle UI ;
- RUN-004 définit le domaine `ui` ;
- FFI-001 définit les appels bloquants et les exécuteurs étrangers ;
- FFI-003 définit les façades Swift et Kotlin.

## Compatibilité et migration

La version 0.2.0 corrige la classification de `Blocking` et rend son appel
direct depuis `ui` statiquement invalide. Le code concerné doit introduire une
tâche ou une transition d’exécuteur ; ce changement est source-breaking.

Ajouter un événement obligatoire rend incomplètes les plateformes qui ne le
traitent pas. Les implémentations peuvent ajouter des événements privés
spécifiques.

## Tests de conformité

Le contrat partagé fournit des tests de machine d’état. Chaque plateforme
ajoute tests d’intégration, accessibilité, navigation et rendu selon ses outils
natifs. La suite couvre aussi le rejet de `Blocking` depuis `ui`, l’acceptation
du même travail après transition asynchrone et le retour sur l’exécuteur UI.

## Questions ouvertes

- Convention standard de retour sur l’exécuteur UI après une tâche.
