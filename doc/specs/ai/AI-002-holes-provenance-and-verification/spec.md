# AI-002 — Trous typés, provenance et vérification

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `ai`

## Objet

Représenter un travail incomplet comme obligation vérifiable et conserver la
provenance des propositions automatiques sans mettre un modèle dans le binaire.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Trous

Un trou possède :

- type attendu ;
- effets autorisés ;
- variables et capacités visibles ;
- préconditions et postconditions ;
- objectif d’optimisation optionnel ;
- exemples ou propriétés liées.

```text
?implementation
    returns Route
    ensures result in routes
    minimize result.energy
```

Le compilateur peut typecheck le contexte autour d’un trou, mais une release
standard NE DOIT contenir aucun trou non résolu.

### Résolution

Une résolution peut venir d’un humain, d’un solveur, d’une IA ou d’une
bibliothèque. Elle devient un patch AI-001 et suit exactement les mêmes gates.

L’identité du producteur ne modifie pas le niveau de confiance : seule
l’évidence vérifiée le fait.

### Provenance

Le dépôt peut enregistrer :

- intention et prompt pertinents ;
- modèle, version et configuration ;
- contexte source identifié par empreintes ;
- patch proposé puis patch accepté ;
- validations exécutées ;
- décisions humaines ;
- licences ou sources utilisées lorsque connues.

Les données sensibles sont exclues ou chiffrées selon politique. La provenance
NE DOIT PAS être nécessaire au fonctionnement du binaire.

### États d’évidence

Pour chaque obligation :

- typecheck ;
- effets/capacités ;
- tests ;
- propriétés ;
- preuve ;
- benchmark ;
- revue humaine requise ou effectuée.

Une interface présente clairement ce qui n’a pas été vérifié.

### Non-déterminisme

La génération peut être non déterministe ; l’application du patch et le build
sont déterministes. Le code accepté, ses tests et ses preuves deviennent les
sources de vérité.

### Politique de release

Un projet définit les validations minimales selon domaine. Exemple :

- `realtime` : analyse mémoire, FFI et benchmark de deadline ;
- sécurité : revue humaine et absence de nouvelles capacités ;
- migration : modèle ou test de compatibilité ;
- kernel : comparaison numérique différentielle.

### Refus

Un système doit pouvoir expliquer pourquoi une proposition est refusée sans
demander à l’IA d’interpréter seule l’erreur.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- AI-001

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
