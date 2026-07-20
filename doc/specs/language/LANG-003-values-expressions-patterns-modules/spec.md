# LANG-003 — Valeurs, expressions, patterns et modules

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `language`

## Objet

Définir le noyau sémantique visible du langage, indépendamment de la syntaxe
choisie.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Valeurs

Le noyau comprend :

- unit, booléens, entiers et flottants explicites ;
- textes Unicode et octets ;
- tuples, records, variantes et fonctions ;
- vecteurs, maps et sets persistants ;
- ressources uniques ;
- valeurs de tâches, acteurs, flux et kernels.

Il n’existe pas de valeur `null`. L’absence se représente par `Option<T>` et
l’échec attendu par `Result<T, E>`.

### Expressions

Toute construction de contrôle produit une valeur : bloc, condition, pattern
match et boucle réductrice. Une boucle impérative qui ne produit rien retourne
`Unit`.

L’ordre d’évaluation est de gauche à droite. Les effets observables NE DOIVENT
PAS être réordonnés, sauf preuve qu’ils sont indépendants.

### Bindings et mutation

Un binding est immuable par défaut. La mutation exige :

- une valeur `transient` ou `unique` ;
- l’état privé d’un acteur ;
- une cellule ou primitive atomique déclarée ;
- une sortie `out` ou un paramètre `inout`.

Une mutation locale NE DOIT PAS être observable avant la publication ou le
gel explicite de la valeur.

### Patterns

Les patterns peuvent décomposer :

- variantes et littéraux ;
- tuples et records ;
- séquences de taille connue ;
- types et raffinements autorisés ;
- gardes pures.

Le compilateur DOIT calculer la partie de l’entrée couverte par chaque branche,
signaler une branche vide et vérifier que le reste est `Never`.

### Modules

Un module définit un espace de symboles, une frontière d’inférence et une unité
d’interface incrémentale.

L’identité nominale déclarée d’un module DOIT être unique dans un package et
NE DOIT PAS dépendre de son chemin absolu. Deux fichiers qui déclarent la même
identité produisent un conflit au lieu d’être fusionnés.

Les membres sont privés par défaut. Une déclaration publique DOIT posséder une
signature stable explicite conformément à ARCH-001.

Les imports sont nominaux et non textuels. Leur ordre NE DOIT PAS modifier la
sémantique. Un import donne accès aux seuls membres publics du module nommé ;
il NE DOIT PAS réexporter ces membres implicitement.

Le graphe d’import des modules source du package DOIT être acyclique. Les
cycles entre signatures de types nommés au sein d’une interface PEUVENT être
autorisés ultérieurement sans rendre cyclique l’initialisation des modules.

Un appel qualifié vers un module non importé DOIT être rejeté, même si ce
module existe dans le package. Cette règle rend les dépendances observables
dans le graphe réel du programme.

### Initialisation

Le chargement d’un module n’exécute aucun effet implicite. Une initialisation
avec effet DOIT appartenir à une fonction ou ressource explicite appelée depuis
la composition de l’application.

Cette règle permet un hot reload sans dupliquer handlers, threads ou connexions.

## Diagnostics et erreurs

Le profil bootstrap utilise les diagnostics stables suivants :

- `RBN2100` pour une identité de module déclarée plusieurs fois ;
- `RBN2101` pour un module importé introuvable ;
- `RBN2102` pour un import dupliqué ;
- `RBN2103` pour un cycle d’import ;
- `RBN2104` pour l’accès à un membre privé ;
- `RBN2105` pour un appel qualifié vers un module non importé.

Chaque diagnostic DOIT être rattaché à la déclaration, à l’import ou à l’appel
responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- ARCH-001
- DX-001
- PKG-001
- TOOL-001

## Compatibilité et migration

La version 0.2.0 précise identité, visibilité, imports et graphe acyclique des
modules. Les programmes mono-module restent valides ; l’ajout est compatible.
Une implémentation expérimentale qui fusionnait silencieusement deux modules de
même nom ou autorisait l’accès privé doit désormais produire un diagnostic.

## Tests de conformité

Les tests DOIVENT couvrir :

- résolution d’un appel public entre deux modules ;
- indépendance à l’ordre des fichiers et des imports ;
- module absent, identité dupliquée, import dupliqué et cycle ;
- rejet d’un membre privé et d’un module non importé ;
- ordre d’évaluation ;
- absence de `null` ;
- exhaustivité et redondance des patterns ;
- visibilité ;
- absence d’effets au chargement ;
- mutation qui ne s’échappe pas d’une région unique.

## Questions ouvertes

Aucune à ce stade.
