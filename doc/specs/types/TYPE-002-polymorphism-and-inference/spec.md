# TYPE-002 — Polymorphisme et inférence

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `types`

## Objet

Combiner une inférence locale de style Hindley–Milner avec la sémantique
ensembliste de TYPE-001, sans sacrifier compilation incrémentale et diagnostics.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Polymorphisme

Les fonctions peuvent quantifier types, lignes, effets, dimensions et
multiplicités :

```text
identity : forall A. A -> A
map      : forall A B E. (A -> B ! E) -> Vector<A> -> Vector<B> ! E
```

Le polymorphisme implicite est de rang 1. Les types de rang supérieur exigent
une annotation explicite.

### Généralisation

Un binding `let` pur et non expansif PEUT être généralisé. Un binding qui
alloue une cellule mutable, capture une ressource unique ou exécute un effet
n’est pas généralisé automatiquement.

Cette restriction de valeur empêche une même cellule mutable d’être utilisée à
plusieurs types incompatibles.

### Portée de l’inférence

Le compilateur infère :

- les bindings et expressions locales ;
- les fonctions privées non récursives ;
- les variables de types d’un pipeline ;
- les lignes de records et d’effets.

Une signature explicite est obligatoire pour :

- les symboles publics ;
- la récursion polymorphe ;
- les frontières FFI ;
- les kernels exportés ;
- les contrats temps réel ;
- les migrations de versions ;
- une ambiguïté de protocoles non résolue localement.

### Types principaux

Le compilateur DEVRAIT produire un type principal lorsqu’il existe dans le
sous-système utilisé. Lorsqu’une intersection, une négation ou une surcharge
empêche une présentation principale utile, il DOIT :

1. demander une annotation locale ; ou
2. choisir une approximation sûre explicitement signalée.

Il NE DOIT PAS choisir silencieusement un type plus spécifique dépendant de
l’ordre de compilation.

### Inférence incrémentale

Une interface publique est un point de coupure. Modifier le corps sans changer
son interface NE DOIT PAS reconstruire les types des consommateurs.

Les résultats d’inférence sont indexés par :

- identité de définition ;
- empreinte des entrées sémantiques ;
- version du solveur ;
- capacités de cible qui influencent réellement le typage.

### Littéraux

Les littéraux numériques sont polymorphes sous contraintes jusqu’à résolution.
Une conversion perdante est explicite. Le type par défaut, lorsqu’il est
nécessaire au REPL, DOIT être documenté et stable.

### Limites

Robine ne promet ni inférence globale complète, ni polymorphisme récursif
implicite, ni résolution automatique de preuves arbitraires.

## Diagnostics et erreurs

Une erreur d’unification DOIT identifier la première contrainte incompatible et
son origine. Les cascades secondaires DEVRAIENT être supprimées.

Le REPL DOIT pouvoir afficher :

```text
type expression
why-type expression
constraints expression
```

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- TYPE-001

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
