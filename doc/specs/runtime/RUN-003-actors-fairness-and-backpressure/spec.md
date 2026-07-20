# RUN-003 — Acteurs, équité et contre-pression

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `runtime`

## Objet

Fournir des identités durables propriétaires de leur état, avec isolation,
équité entre utilisateurs et surcharge bornée.

## Acteur

Un acteur possède :

- une identité ;
- un état privé ;
- un protocole de messages typé ;
- une mailbox bornée ;
- un budget de ressources ;
- une politique de supervision.

Un seul handler modifie son état à la fois. Un handle d’acteur NE DONNE PAS
accès à cet état.

## Messages

`send` n’attend pas de réponse. `ask` retourne une `Task<Response, Error>`.
L’envoi copie, déplace ou partage uniquement des valeurs autorisées par leurs
multiplicités.

L’ordre FIFO est garanti par couple émetteur-récepteur, sauf protocole déclarant
une autre politique. L’ordre global entre plusieurs émetteurs n’est pas garanti.

## Mailbox bornée

Chaque type de message définit son comportement à saturation :

- rejet ;
- attente du producteur hors temps réel ;
- suppression du plus ancien ;
- conservation du plus récent ;
- coalescence par clé.

Une mailbox non bornée est interdite dans une release conforme standard.

## Équité

Un acteur `responsive` reçoit un quantum et un poids. Le scheduler mesure un
budget de travail ou de temps CPU et préempte aux points sûrs.

Une boucle statiquement bornée PEUT être facturée d’avance, ce qui autorise la
suppression de polls internes.

L’équité n’est garantie que pour :

- code compilé avec points sûrs ;
- FFI déclarées non bloquantes ;
- kernels partitionnables ou soumis avec coût borné ;
- ressources admises par RUN-004.

## Supervision

Une faute d’acteur est une valeur structurée. Le superviseur choisit :

- arrêter ;
- redémarrer avec état initial ;
- redémarrer depuis snapshot ;
- propager ;
- isoler définitivement.

Un redémarrage NE DOIT PAS réexécuter arbitrairement des effets externes sans
idempotence ou journal explicite.

## Acteurs et tâches

Les acteurs possèdent ; les tâches calculent. Un handler DEVRAIT envoyer un
calcul intensif à une tâche plutôt que bloquer sa mailbox.

## Diagnostics

Le runtime expose latence de mailbox, profondeur, messages fusionnés/rejetés,
temps CPU, allocations et causes de starvation par acteur et tenant.
