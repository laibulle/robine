# TYPE-005 — Ownership, multiplicités, raffinements et formes

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `types`

## Objet

Ajouter les garanties que l’algèbre des ensembles et HM ne couvrent pas :
usage unique des ressources, invariants numériques et dimensions de données.

## Multiplicités

Une valeur possède une multiplicité d’usage :

- `many` : partage libre ;
- `affine` : utilisée au plus une fois ;
- `linear` : utilisée exactement une fois ;
- `borrow` : accès temporaire borné à un appel ou une région.

Les valeurs immuables ordinaires sont `many`. Les buffers mutables, fichiers,
sockets, secrets et handles natifs peuvent être `affine` ou `linear`.

## Paramètres d’accès

```text
in T       lecture
inout T    accès exclusif temporaire
out T      sortie initialisée avant retour
move T     transfert de propriété
```

Un `inout` NE PEUT PAS s’échapper, être capturé par une tâche ou coexister avec
un autre accès au même emplacement.

Le compilateur infère les durées à l’intérieur d’une fonction. Les annotations
de lifetime explicites ne font pas partie de l’API ordinaire.

## Raffinements

Un raffinement restreint un ensemble par un prédicat :

```text
Gain = Float where 0.0 <= self <= 10.0
```

Le compilateur classe chaque obligation :

1. prouvée par règles décidables ;
2. déléguée explicitement à un solveur ;
3. conservée comme contrôle d’exécution ;
4. rejetée si le contexte interdit ce contrôle.

Une fonction `realtime` NE DOIT PAS introduire implicitement un contrôle non
borné ou une allocation pour vérifier un raffinement.

## Contrats

Les préconditions, postconditions et invariants utilisent le même langage de
prédicats purs :

```text
requires routes.size > 0
ensures result in routes
```

Une postcondition NE PEUT PAS modifier l’exécution. Son échec produit une faute
de contrat attribuée au fournisseur ; une précondition violée est attribuée à
l’appelant.

## Formes et unités

Les tenseurs et blocs peuvent porter dimensions et unités :

```text
Tensor<B, T, D, bf16>
AudioBlock<N, 48kHz, f32>
Celsius
```

Les contraintes de formes de base sont limitées à une arithmétique décidable :
égalité, bornes, produits constants, divisibilité et relations affines
documentées.

## Compilation

Les preuves et raffinements sont effacés lorsque démontrés. Les multiplicités
permettent mutation sur place, élision de copies et désallocation déterministe.

## Diagnostics

Les erreurs DOIVENT parler de la ressource et de l’usage concret, pas exposer
le solveur interne :

```text
buffer a été transféré à freeze ici
une écriture ultérieure est impossible
```
