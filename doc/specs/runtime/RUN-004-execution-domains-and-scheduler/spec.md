# RUN-004 — Domaines d’exécution et scheduler

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `runtime`

## Objet

Éviter qu’un seul compromis de runtime s’impose au programme entier.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Domaines standards

```text
normal       code natif sans garantie temporelle
script       image vivante et réflexion de développement
responsive   préemption, budgets et équité
realtime     exécution bornée sans suspension
kernel       graphe pur destiné CPU matriciel ou NPU
ui           boucle d’événements de plateforme
isolated     worker ou processus pour code non maîtrisé
```

Une fonction possède un domaine déclaré ou inféré depuis sa définition et ses
effets. Le domaine retenu DOIT apparaître dans la HIR et, pour une fonction
publique, dans son artefact d’interface.

Un contexte d’appel PEUT demander une variante d’exécution plus contrainte,
par exemple l’abaissement préemptible de RUN-005. Cette variante NE DOIT PAS
changer silencieusement le domaine contractuel de la définition. Une fonction
réutilisable sous plusieurs domaines déclare un polymorphisme de domaine ou
produit des variantes vérifiées dont la relation est enregistrée.

Les domaines sont distincts des effets et capacités de TYPE-004.

### Transitions

Une transition entre domaines est un point sémantique visible :

- `spawn` vers `responsive` ;
- `dispatch` vers `kernel` ;
- queue bornée vers `realtime` ;
- appel plateforme vers `ui` ;
- RPC local vers `isolated`.

Le compilateur NE DOIT PAS masquer une copie, suspension ou perte de garantie
derrière un appel qui semble local.

### Admission

Une garantie temporelle ou d’équité exige une admission :

```text
cpu budget
memory budget
mailbox capacity
deadline
hardware profile
```

Le runtime refuse, dégrade selon une politique déclarée ou classe la garantie
en `best_effort` lorsque les ressources sont insuffisantes.

### Points sûrs

Le compilateur place des points sûrs aux :

- retours de boucle non bornée ;
- appels potentiellement longs ;
- allocations ;
- frontières de messages ;
- opérations explicitement cancellables.

Il PEUT les regrouper ou les supprimer après preuve de coût borné.

`realtime` ne contient aucun point de préemption injecté.

### Runtime par capacités

Le linker inclut uniquement les services utilisés :

```text
actors       scheduler + mailboxes
gc-local     collecteur de heaps isolés
hot-reload   table de versions + métadonnées
realtime     primitives bornées et atomiques
compute      files de commandes et backends
```

### Déterminisme

Le résultat fonctionnel d’un programme sans effet de concurrence est
déterministe. Les programmes concurrents doivent déclarer les points où l’ordre
est observable. Un mode de test déterministe PEUT contrôler le scheduler.

### Observabilité

Chaque transition de domaine produit une trace corrélable sans obliger un
appel temps réel à bloquer. Les buffers d’observation temps réel sont bornés et
peuvent perdre des événements avec compteur de perte.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- TYPE-004 sépare effets, capacités et domaines ;
- RUN-001 définit les restrictions d’allocation ;
- RUN-002 utilise les domaines pour tâches, deadlines et annulation ;
- RUN-003 demande `responsive` pour l’équité des acteurs ;
- RUN-005 définit les variantes préemptibles et la fermeture de runtime ;
- RT-001 spécialise l’admission du domaine `realtime` ;
- COMP-001 et COMP-002 définissent le domaine `kernel` ;
- UI-001 définit l’exécution sur la boucle `ui` ;
- FFI-001 contraint les appels étrangers par domaine.

## Compatibilité et migration

La version 0.2.0 rend le domaine visible dans les interfaces et interdit qu’un
contexte d’appel change silencieusement le domaine d’une définition. Les
artefacts qui inféraient des domaines différents selon le consommateur doivent
publier un polymorphisme ou des variantes ; ce changement est ABI-breaking
pour ces artefacts.

## Tests de conformité

La suite de conformité DOIT couvrir :

- domaine inféré identique dans la HIR et l’artefact public ;
- variante préemptible conservant le domaine contractuel d’origine ;
- rejet d’un changement de domaine dépendant seulement du consommateur ;
- transition visible avec copie ou suspension ;
- admission, dégradation explicite et refus ;
- absence de poll injecté dans `realtime`.

## Questions ouvertes

- Forme publique du polymorphisme de domaine et identité ABI de ses variantes.
