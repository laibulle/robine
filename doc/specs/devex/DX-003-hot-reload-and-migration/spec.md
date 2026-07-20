# DX-003 — Hot reload transactionnel et migration d’état

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `devex`

## Objet

Mettre à jour code et état d’un programme vivant sans réexécuter implicitement
les effets de chargement.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Phases

Une mise à jour suit :

1. calcul des définitions affectées ;
2. compilation des nouvelles versions ;
3. validation types, effets, contrats et architecture ;
4. tests ciblés optionnels ou obligatoires ;
5. préparation des migrations ;
6. admission des ressources ;
7. commit aux points sûrs ;
8. retrait des anciennes versions après quiescence.

Avant le commit, tout échec DOIT laisser l’image inchangée.

### Types et état

Ajouter un champ avec valeur par défaut peut produire une migration automatique.
Renommer, supprimer ou changer la signification exige :

```text
migrate Type@v1 -> Type@v2
```

Une migration est pure par défaut. Les effets externes exigent une migration
orchestrée, idempotente et récupérable.

### Acteurs

Un acteur migre entre deux messages. Sa mailbox est gelée, bornée et conservée
ou transformée selon le protocole. Le rollback doit connaître la version des
messages et de l’état.

### UI

L’état local est associé à des identités structurelles stables. Une vue peut
changer sans perdre son état lorsque son identité et son type restent
compatibles.

### Audio

Le reload audio suit RT-002 : préparation hors callback, échange à une frontière
de bloc et transition sonore déclarée.

### Effets de module

Recompiler un module NE réexécute jamais automatiquement ouverture de connexion,
enregistrement de handler ou création de thread. `reload definitions`,
`restart resource` et `migrate state` sont des opérations distinctes.

### Production

Une release peut conserver uniquement des frontières d’upgrade choisies :
acteurs, plugins ou services. À l’intérieur de ces frontières, les appels
restent directs et optimisables.

### Audit

Chaque commit enregistre versions avant/après, migrations, résultats de
validation, opérateur et rollback disponible.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- TYPE-001
- RT-002

## Compatibilité et migration

Une fonction est remplaçable directement si son contrat public est compatible.
Un élargissement d’entrée ou rétrécissement de sortie peut être sûr selon
TYPE-001 ; les changements d’effets, ownership et deadline sont vérifiés
séparément.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
