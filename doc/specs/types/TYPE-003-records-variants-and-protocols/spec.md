# TYPE-003 — Records, variantes, lignes et protocoles

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `types`

## Objet

Fournir un modèle orienté données, extensible sans hiérarchie de classes et
compatible avec les types ensemblistes.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Records

Un record décrit des champs nommés. Les champs sont immuables par défaut et
possèdent des identifiants stables dans l’interface compilée.

```text
User = { id: UserId, name: Text }
```

Une ligne ouverte s’écrit abstraitement :

```text
{ name: A | R }
```

Elle accepte tout record contenant au moins `name: A`. La mise à jour
fonctionnelle produit une nouvelle valeur avec partage ou copie optimisée.

### Maps dynamiques

Un champ statique et une clé dynamique sont distincts :

```text
user.name                 champ vérifié
headers["content-type"]   Map<Text, Text>
```

Une entrée externe NE DOIT PAS être internée comme identifiant global. Le
décodage vers un record passe par un schéma explicite.

### Variantes

Une variante est une union étiquetée :

```text
Result<T, E> = Ok(T) | Error(E)
```

Les tags sont des singletons de TYPE-001. Les variantes fermées permettent
l’exhaustivité. Des unions structurelles ouvertes PEUVENT composer des familles
d’événements sans modifier leur définition d’origine.

### Protocoles

Un protocole définit un ensemble d’opérations sans stockage ni héritage :

```text
protocol Render<Target> {
    render(Self, inout Target) -> Unit
}
```

Une implémentation est identifiée par le triplet protocole, type implémenté,
paramètres de protocole.

Une implémentation publique PEUT être déclarée uniquement dans le package qui
possède le protocole ou dans celui qui possède le type nominal implémenté. Un
alias transparent ne confère pas cette propriété.

La cohérence est vérifiée sur tout le graphe de packages résolu, pas seulement
sur les imports textuellement visibles. L’ordre ou l’ajout d’un import NE DOIT
PAS choisir une implémentation différente.

Le profil portable interdit deux implémentations dont les domaines peuvent se
chevaucher, même si l’une semble plus spécifique. Une spécialisation avec
chevauchement est autorisée uniquement dans un arbre fermé, explicitement
ordonné et possédé par le package du protocole. Ses domaines effectifs suivent
TYPE-001.

Un package qui ajoute une implémentation susceptible de chevaucher une
implémentation légale existante ou aval modifie son contrat de compatibilité.
Les contraintes de non-chevauchement DOIVENT apparaître dans l’artefact
d’interface.

Un besoin de comportement local différent utilise un nouveau type opaque, un
adaptateur ou un argument de capacité ; il NE DOIT PAS injecter une instance
locale qui change silencieusement la résolution d’une fonction générique.

### Dispatch

Trois formes sont distinctes :

- dispatch statique, spécialisable et sans coût obligatoire ;
- dispatch par table demandé explicitement ;
- fonction multiclause définie par intersections et patterns.

Un protocole NE DOIT PAS être utilisé comme classe porteuse d’état.

### Optimisation

Le compilateur PEUT :

- spécialiser une ligne fermée ;
- convertir un record scellé en layout compact ;
- dévirtualiser un protocole à implémentation unique ;
- fusionner une mise à jour transiente.

Ces transformations doivent préserver la réflexion explicitement demandée.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- TYPE-001
- TYPE-002 définit les contraintes génériques et frontières d’inférence ;
- ARCH-001 publie les implémentations et contraintes de cohérence ;
- ARCH-003 classe l’ajout d’une implémentation ;
- LANG-005 distingue protocole statique, existentiel et extension receveur.

## Compatibilité et migration

La version 0.2.0 remplace la cohérence limitée aux implémentations « visibles »
par une cohérence sur le graphe résolu et interdit les chevauchements ouverts.
Les instances locales ou orphelines doivent devenir newtypes ou adaptateurs ;
ce changement est source-breaking.

Ajouter un champ public obligatoire est source-breaking. Ajouter un champ avec
valeur par défaut peut être compatible pour les données versionnées, mais PEUT
modifier l’ABI compacte.

Ajouter une variante à une union fermée est source-breaking pour les matches
exhaustifs. Une union ouverte DOIT définir son comportement face aux tags
inconnus.

## Tests de conformité

La suite de conformité DOIT couvrir :

- implémentation possédée par le protocole et par le type ;
- rejet d’une implémentation orpheline ;
- résolution identique quel que soit l’ordre des imports ;
- rejet de deux domaines d’implémentation qui se chevauchent ;
- spécialisation fermée avec domaines effectifs ;
- compatibilité lors de l’ajout d’une implémentation ;
- records, variantes, lignes ouvertes et dispatch explicite.

## Questions ouvertes

- Forme d’une délégation explicite de l’autorité de cohérence à un autre
  package.
