# TYPE-001 — Fondation ensembliste des types

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `types`

## Objet

Définir les types de valeurs comme des ensembles et le sous-typage comme
l’inclusion entre ces ensembles.

## Algèbre

Le système fournit :

```text
Any             ensemble de toutes les valeurs
Never           ensemble vide
A | B           union
A & B           intersection
A \ B           différence
not A           complément relatif à Any
literal(v)      singleton contenant v
```

Les lois d’association, commutativité, idempotence et distributivité DOIVENT
être respectées sémantiquement, même si la représentation interne diffère.

```text
A | A = A
A & Any = A
A & not A = Never
A <: B  si et seulement si  A \ B = Never
```

## Types nommés et opacité

Un alias transparent désigne le même ensemble que sa définition. Un type
`opaque` crée une identité nominale dont la représentation n’est visible que
dans son module propriétaire.

Deux types opaques de même représentation NE SONT PAS substituables.

## Fonctions

Une fonction multiclause peut avoir une intersection de flèches :

```text
(Text -> Int) & (Vector<A> -> Int)
```

Pour qu’un appel soit accepté, au moins une branche applicable DOIT couvrir la
totalité du type de l’argument. Les domaines qui se chevauchent avec résultats
incompatibles produisent une ambiguïté statique, sauf priorité définie par des
patterns ordonnés dans une même fonction.

## Occurrence typing

Après un test de type ou un pattern :

```text
avant : x : A
branche vraie : x : A & Tested
branche fausse : x : A \ Tested
```

Les gardes utilisées pour raffiner un type DOIVENT être pures et appartenir à
un ensemble reconnu par le compilateur.

## Exhaustivité

Pour des branches couvrant `B1...Bn` sur une entrée `A` :

```text
reste = A \ (B1 | ... | Bn)
```

Le match est exhaustif si `reste = Never`. Une branche est redondante lorsque
sa couverture, moins les branches précédentes, vaut `Never`.

## Représentation et coût

Le compilateur PEUT utiliser BDD, DAG canoniques, normalisation paresseuse ou
formes spécialisées. Les opérations ensemblistes statiques sont effacées en
release et NE DOIVENT PAS ajouter de tags d’exécution inutiles.

## Restrictions

- Les types récursifs publics DOIVENT être nommés.
- Le complément d’un type contenant des variables libres PEUT être refusé.
- Les formes internes trop complexes PEUVENT être résumées dans les diagnostics.
- La décision de sous-typage DOIT terminer pour tout programme accepté.

## Diagnostics

Une erreur DOIT présenter une différence concrète, pas seulement une formule :

```text
Attendu : Text | Bytes
Reçu    : Text | Connection
Non couvert : Connection
```

## Conformité

La suite de tests DOIT vérifier les lois algébriques, les singletons, les
fonctions intersection, l’occurrence typing et l’exhaustivité.
