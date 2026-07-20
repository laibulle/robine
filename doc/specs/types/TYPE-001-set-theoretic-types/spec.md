# TYPE-001 — Fondation ensembliste des types

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `types`

## Objet

Définir les types de valeurs comme des ensembles et le sous-typage comme
l’inclusion entre ces ensembles.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Algèbre

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

### Types nommés et opacité

Un alias transparent désigne le même ensemble que sa définition. Un type
`opaque` crée une identité nominale dont la représentation n’est visible que
dans son module propriétaire.

Deux types opaques de même représentation NE SONT PAS substituables.

### Fonctions

Une fonction multiclause peut avoir une intersection de flèches :

```text
(Text -> Int) & (Vector<A> -> Int)
```

Pour qu’un appel soit accepté, au moins une branche applicable DOIT couvrir la
totalité du type de l’argument.

Pour une intersection de flèches indépendante :

```text
(A -> R1) & (B -> R2)
```

une valeur d’entrée appartenant à `A & B` DOIT produire un résultat appartenant
à `R1 & R2`. Lorsque cette intersection de résultats est vide, aucun
chevauchement habité ne peut être implémenté par cette intersection de
fonctions.

Les patterns ordonnés d’une même fonction utilisent des domaines effectifs
disjoints. Pour des couvertures brutes `P1...Pn` :

```text
D1 = P1
Di = Pi \ (P1 | ... | P(i-1))
```

La flèche de la clause `i` porte sur `Di`, pas sur la totalité de `Pi`. La
priorité d’une clause NE DOIT PAS servir à prétendre qu’une fonction satisfait
deux résultats incompatibles sur le même domaine ensembliste.

### Occurrence typing

Après un test de type ou un pattern :

```text
avant : x : A
branche vraie : x : A & Tested
branche fausse : x : A \ Tested
```

Les gardes utilisées pour raffiner un type DOIVENT être pures et appartenir à
un ensemble reconnu par le compilateur.

### Exhaustivité

Pour des branches couvrant `B1...Bn` sur une entrée `A` :

```text
reste = A \ (B1 | ... | Bn)
```

Le match est exhaustif si `reste = Never`. Une branche est redondante lorsque
sa couverture, moins les branches précédentes, vaut `Never`.

### Représentation et coût

Le compilateur PEUT utiliser BDD, DAG canoniques, normalisation paresseuse ou
formes spécialisées. Les opérations ensemblistes statiques sont effacées en
release et NE DOIVENT PAS ajouter de tags d’exécution inutiles.

### Restrictions

- Les types récursifs publics DOIVENT être nommés.
- Le complément d’un type contenant des variables libres PEUT être refusé.
- Les formes internes trop complexes PEUVENT être résumées dans les diagnostics.
- La décision de sous-typage DOIT terminer pour tout programme accepté.

## Diagnostics et erreurs

Une erreur DOIT présenter une différence concrète, pas seulement une formule :

```text
Attendu : Text | Bytes
Reçu    : Text | Connection
Non couvert : Connection
```

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- LANG-003 définit l’ordre des patterns et leur couverture ;
- TYPE-002 compose sous-typage et polymorphisme local ;
- TYPE-003 utilise singletons, unions et intersections pour les variantes et
  protocoles ;
- TYPE-005 ajoute des obligations distinctes de l’algèbre ensembliste ;
- DX-003 utilise inclusion et différence pour la compatibilité de reload.

## Compatibilité et migration

La version 0.2.0 sépare les intersections de flèches des priorités de patterns
ordonnés. Une fonction auparavant acceptée avec deux résultats incompatibles
sur un domaine commun est rejetée ou doit typer ses clauses sur leurs domaines
effectifs. Ce changement est source-breaking et corrige une incohérence de
sémantique statique.

## Tests de conformité

La suite de tests DOIT vérifier les lois algébriques, les singletons, les
fonctions intersection, l’occurrence typing et l’exhaustivité. Elle couvre en
plus :

- intersection de flèches avec codomaines compatibles sur le chevauchement ;
- rejet de codomaines incompatibles sur un chevauchement habité ;
- calcul des domaines effectifs de patterns ordonnés ;
- absence de priorité implicite entre deux flèches indépendantes.

## Questions ouvertes

- Représentation diagnostique canonique des domaines effectifs d’une grande
  fonction multiclause.
