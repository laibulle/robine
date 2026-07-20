# DX-003 — Hot reload transactionnel et migration d’état

- Statut : **Draft**
- Version : **0.2.0**
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

Avant la première publication d’une nouvelle génération, tout échec DOIT
laisser l’image observable inchangée.

### Unité et atomicité

Une transaction de reload déclare ses participants et l’un des niveaux
d’atomicité suivants :

- `boundary` : chaque acteur, graphe audio, vue ou service publie atomiquement
  sa propre génération ;
- `coordinated` : tous les participants préparés deviennent visibles par la
  publication d’une génération commune.

La préparation produit des états candidats sans modifier les états vivants.
Un commit `coordinated` DOIT disposer d’une barrière, d’un routage par
génération et de points sûrs compatibles pour tous ses participants. Si une
plateforme ne peut pas fournir ces conditions, le niveau `coordinated` est
rejeté ; il NE DOIT PAS être abaissé silencieusement vers `boundary`.

Avec `boundary`, plusieurs générations PEUVENT coexister pendant la
transaction. Les messages, callbacks et appels qui traversent deux générations
DOIVENT avoir un adaptateur compatible ou être retenus jusqu’au commit de leur
destination.

Après une première publication, un échec NE PEUT PLUS être décrit comme une
image inchangée. L’orchestrateur DOIT soit :

- restaurer chaque participant déjà publié avec un rollback préparé ;
- terminer vers l’avant la génération cible ;
- isoler les participants affectés dans un état de récupération explicite.

La stratégie est choisie avant le commit et enregistrée dans l’audit.

### Types et état

Ajouter un champ avec valeur par défaut peut produire une migration automatique.
Renommer, supprimer ou changer la signification exige :

```text
migrate Type@v1 -> Type@v2
```

Une migration est pure par défaut. Les effets externes exigent une migration
orchestrée, idempotente et récupérable.

Une migration avec effet externe NE PEUT appartenir à un commit atomique que
si son journal et sa compensation sont préparés et que le contrat externe les
admet. Sinon l’effet est exécuté avant ou après la transaction par une étape
orchestrée dont l’échec utilise une reprise vers l’avant.

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
- DATA-001 définit les migrations de schéma ;
- RUN-002 définit tâches, annulation et travail résiduel ;
- RUN-003 définit les points sûrs et mailboxes d’acteurs ;
- RUN-005 définit générations vivantes et fermeture de runtime ;
- ARCH-003 définit la compatibilité des adaptateurs.

## Compatibilité et migration

La version 0.2.0 remplace le commit global implicite par les niveaux
`boundary` et `coordinated`, et définit les échecs après publication. Une
transaction antérieure doit déclarer son niveau, ses adaptateurs et sa stratégie
de récupération ; ce changement est source-breaking pour les manifestes de
reload.

Une fonction est remplaçable directement si son contrat public est compatible.
Un élargissement d’entrée ou rétrécissement de sortie peut être sûr selon
TYPE-001 ; les changements d’effets, ownership et deadline sont vérifiés
séparément.

## Tests de conformité

La suite de conformité DOIT couvrir :

- échec de préparation sans modification observable ;
- commit atomique d’une frontière ;
- coexistence de deux générations avec adaptateur ;
- rejet d’un commit coordonné sans barrière commune ;
- rollback après publication partielle ;
- reprise vers l’avant lorsque le rollback est impossible ;
- migration avec effet refusée sans journal ni compensation ;
- retrait des anciennes versions après quiescence.

## Questions ouvertes

- Protocole de génération coordonnée entre plusieurs processus.
- Durée maximale de coexistence de générations avant reprise imposée.
