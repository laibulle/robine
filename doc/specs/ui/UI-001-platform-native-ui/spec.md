# UI-001 — Interfaces réellement natives par plateforme

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `ui`

## Objet

Partager le domaine et les machines d’état sans imposer une UI au plus petit
dénominateur commun entre iOS, Android, Web et desktop.

## Séparation

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

## Code partagé

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

## Bindings

Le backend iOS accède aux SDK Apple et ABI Objective-C/Swift compatibles. Le
backend Android accède aux API Android et à Compose via les formats requis par
la plateforme.

Les wrappers générés DOIVENT préserver nullabilité, ownership, threading et
disponibilité de version.

## Boucle UI

Les mises à jour de vue s’exécutent sur l’exécuteur UI de la plateforme. Un
travail bloquant ou intensif doit devenir une tâche. Le compilateur signale une
capacité `Blocking` appelée depuis le domaine `ui`.

## Évolution

Ajouter un événement obligatoire rend incomplètes les plateformes qui ne le
traitent pas. Les implémentations peuvent ajouter des événements privés
spécifiques.

## Tests

Le contrat partagé fournit des tests de machine d’état. Chaque plateforme
ajoute tests d’intégration, accessibilité, navigation et rendu selon ses outils
natifs.

## Non-objectif

Robine ne promet pas « écrire une UI une fois ». Elle promet de ne réécrire que
la partie dont la différence est réellement nécessaire.
