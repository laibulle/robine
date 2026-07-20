# RUN-003 — Acteurs, équité et contre-pression

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `runtime`

## Objet

Fournir des identités durables propriétaires de leur état, avec isolation,
équité entre utilisateurs et surcharge bornée.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Acteur

Un acteur possède :

- une identité ;
- un état privé ;
- un protocole de messages typé ;
- une mailbox bornée ;
- un budget de ressources ;
- une politique de supervision.

Un seul handler modifie son état à la fois. Un handle d’acteur NE DONNE PAS
accès à cet état.

### Messages

L’API de messages distingue conceptuellement :

```text
try_send(message) -> DeliveryDisposition
send(message) -> Task<DeliveryDisposition, SendError>
ask(message) -> Task<Response, Error>
```

`try_send` ne suspend jamais. `send` PEUT attendre une place uniquement hors
`realtime` et selon la politique déclarée. Aucun des deux n’attend une réponse
métier ; seul `ask` le fait.

`DeliveryDisposition` distingue au minimum `enqueued`, `rejected`, `dropped`,
`replaced` et `coalesced`. Une politique de saturation NE DOIT PAS cacher au
producteur la disposition appliquée.

L’envoi copie, déplace ou partage uniquement des valeurs autorisées par leurs
multiplicités.

L’ordre FIFO est garanti par couple émetteur-récepteur, sauf protocole déclarant
une autre politique. L’ordre global entre plusieurs émetteurs n’est pas garanti.

### Mailbox bornée

Chaque type de message définit son comportement à saturation :

- rejet ;
- attente asynchrone du producteur hors temps réel ;
- suppression du plus ancien ;
- conservation du plus récent ;
- coalescence par clé.

Une mailbox non bornée est interdite dans une release conforme standard.

### Équité

Un acteur `responsive` reçoit un quantum et un poids. Le scheduler mesure un
budget de travail ou de temps CPU et préempte aux points sûrs.

Une boucle statiquement bornée PEUT être facturée d’avance, ce qui autorise la
suppression de polls internes.

L’équité n’est garantie que pour :

- code compilé avec points sûrs ;
- FFI déclarées non bloquantes ;
- kernels partitionnables ou soumis avec coût borné ;
- ressources admises par RUN-004.

### Supervision

Une faute d’acteur est une valeur structurée. Le superviseur choisit :

- arrêter ;
- redémarrer avec état initial ;
- redémarrer depuis snapshot ;
- propager ;
- isoler définitivement.

Un redémarrage NE DOIT PAS réexécuter arbitrairement des effets externes sans
idempotence ou journal explicite.

### Acteurs et tâches

Les acteurs possèdent ; les tâches calculent. Un handler DEVRAIT envoyer un
calcul intensif à une tâche plutôt que bloquer sa mailbox.

## Diagnostics et erreurs

Le runtime expose latence de mailbox, profondeur, messages fusionnés/rejetés,
temps CPU, allocations et causes de starvation par acteur et tenant.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- TYPE-005 contraint copie, partage et déplacement des messages ;
- RUN-002 définit `TaskOutcome` pour `send` et `ask` ;
- RUN-004 définit admission et domaine `responsive` ;
- RUN-005 spécialise mailboxes, budgets et supervision ;
- RT-001 interdit la suspension du producteur temps réel.

## Compatibilité et migration

La version 0.2.0 rend la disposition d’un envoi observable et sépare envoi
immédiat, attente de capacité et requête-réponse. Une API `send` qui perdait ou
rejetait silencieusement un message doit retourner une disposition ; ce
changement est source-breaking.

## Tests de conformité

La suite de conformité DOIT couvrir :

- chaque variante de `DeliveryDisposition` ;
- `try_send` immédiat et non suspendable ;
- `send` en attente asynchrone hors temps réel ;
- rejet de cette attente depuis `realtime` ;
- `ask` distingué d’un simple envoi ;
- ordre FIFO par couple, budgets, équité et supervision ;
- rejet de tout chevauchement de mutation d’état.

## Questions ouvertes

- Noms publics et niveau de détail standard de `DeliveryDisposition`.
