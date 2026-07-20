# TYPE-003 — Records, variantes, lignes et protocoles

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `types`

## Objet

Fournir un modèle orienté données, extensible sans hiérarchie de classes et
compatible avec les types ensemblistes.

## Records

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

## Maps dynamiques

Un champ statique et une clé dynamique sont distincts :

```text
user.name                 champ vérifié
headers["content-type"]   Map<Text, Text>
```

Une entrée externe NE DOIT PAS être internée comme identifiant global. Le
décodage vers un record passe par un schéma explicite.

## Variantes

Une variante est une union étiquetée :

```text
Result<T, E> = Ok(T) | Error(E)
```

Les tags sont des singletons de TYPE-001. Les variantes fermées permettent
l’exhaustivité. Des unions structurelles ouvertes PEUVENT composer des familles
d’événements sans modifier leur définition d’origine.

## Protocoles

Un protocole définit un ensemble d’opérations sans stockage ni héritage :

```text
protocol Render<Target> {
    render(Self, inout Target) -> Unit
}
```

Une implémentation est identifiée par le triplet protocole, type implémenté,
paramètres de protocole.

La résolution statique DOIT être cohérente : deux implémentations également
spécifiques visibles au même point sont une erreur.

## Dispatch

Trois formes sont distinctes :

- dispatch statique, spécialisable et sans coût obligatoire ;
- dispatch par table demandé explicitement ;
- fonction multiclause définie par intersections et patterns.

Un protocole NE DOIT PAS être utilisé comme classe porteuse d’état.

## Évolution

Ajouter un champ public obligatoire est source-breaking. Ajouter un champ avec
valeur par défaut peut être compatible pour les données versionnées, mais PEUT
modifier l’ABI compacte.

Ajouter une variante à une union fermée est source-breaking pour les matches
exhaustifs. Une union ouverte DOIT définir son comportement face aux tags
inconnus.

## Optimisation

Le compilateur PEUT :

- spécialiser une ligne fermée ;
- convertir un record scellé en layout compact ;
- dévirtualiser un protocole à implémentation unique ;
- fusionner une mise à jour transiente.

Ces transformations doivent préserver la réflexion explicitement demandée.
