# LANG-003 — Valeurs, expressions, patterns et modules

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `language`

## Objet

Définir le noyau sémantique visible du langage, indépendamment de la syntaxe
choisie.

## Valeurs

Le noyau comprend :

- unit, booléens, entiers et flottants explicites ;
- textes Unicode et octets ;
- tuples, records, variantes et fonctions ;
- vecteurs, maps et sets persistants ;
- ressources uniques ;
- valeurs de tâches, acteurs, flux et kernels.

Il n’existe pas de valeur `null`. L’absence se représente par `Option<T>` et
l’échec attendu par `Result<T, E>`.

## Expressions

Toute construction de contrôle produit une valeur : bloc, condition, pattern
match et boucle réductrice. Une boucle impérative qui ne produit rien retourne
`Unit`.

L’ordre d’évaluation est de gauche à droite. Les effets observables NE DOIVENT
PAS être réordonnés, sauf preuve qu’ils sont indépendants.

## Bindings et mutation

Un binding est immuable par défaut. La mutation exige :

- une valeur `transient` ou `unique` ;
- l’état privé d’un acteur ;
- une cellule ou primitive atomique déclarée ;
- une sortie `out` ou un paramètre `inout`.

Une mutation locale NE DOIT PAS être observable avant la publication ou le
gel explicite de la valeur.

## Patterns

Les patterns peuvent décomposer :

- variantes et littéraux ;
- tuples et records ;
- séquences de taille connue ;
- types et raffinements autorisés ;
- gardes pures.

Le compilateur DOIT calculer la partie de l’entrée couverte par chaque branche,
signaler une branche vide et vérifier que le reste est `Never`.

## Modules

Un module définit un espace de symboles, une frontière d’inférence et une unité
d’interface incrémentale.

Les membres sont privés par défaut. Une déclaration publique DOIT posséder une
signature stable explicite conformément à ARCH-001.

Les imports sont nominaux et non textuels. Leur ordre NE DOIT PAS modifier la
sémantique. Les cycles entre valeurs initialisées sont interdits ; les cycles
entre signatures de types nommés PEUVENT être autorisés.

## Initialisation

Le chargement d’un module n’exécute aucun effet implicite. Une initialisation
avec effet DOIT appartenir à une fonction ou ressource explicite appelée depuis
la composition de l’application.

Cette règle permet un hot reload sans dupliquer handlers, threads ou connexions.

## Conformité

Les tests DOIVENT couvrir :

- ordre d’évaluation ;
- absence de `null` ;
- exhaustivité et redondance des patterns ;
- visibilité ;
- absence d’effets au chargement ;
- mutation qui ne s’échappe pas d’une région unique.
