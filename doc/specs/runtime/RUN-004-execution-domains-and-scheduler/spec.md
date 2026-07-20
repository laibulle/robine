# RUN-004 — Domaines d’exécution et scheduler

- Statut : **Draft**
- Version : **0.1.0**
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

Une fonction appartient à un domaine par annotation, inférence depuis ses
effets ou contexte d’appel.

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

Aucune interaction normative supplémentaire n’est déclarée.

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
