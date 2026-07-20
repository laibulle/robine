# NET-001 — Protocoles typés et distribution

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `network`

## Objet

Étendre tâches, acteurs et Live UI au réseau sans faire croire qu’un appel
distant possède la sémantique d’un appel local.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Frontière distante

Une opération distante retourne une tâche et déclare :

- protocole et versions ;
- sérialisation DATA-001 ;
- authentification et capacités ;
- deadline ;
- idempotence ;
- politique de retry ;
- erreurs de transport séparées des erreurs métier.

La latence et la partition réseau sont toujours représentables.

Une opération distante retourne conceptuellement :

```text
Task<Response, RemoteError<BusinessError, TransportError>>
```

`BusinessError` et `TransportError` restent distinguables. `Cancelled` et
`RuntimeFault` sont les issues universelles de `TaskOutcome` selon RUN-002 et
NE DOIVENT PAS être reclassées comme erreur de transport.

### Sémantique de livraison

Les garanties disponibles sont explicites :

- at-most-once ;
- at-least-once avec idempotence ;
- déduplication bornée ;
- flux ordonné dans une session.

« Exactly once » NE DOIT PAS être promis sans transaction partagée définie et
preuve de ses limites.

### Protocoles

Un protocole est une machine d’état typée. Le compilateur vérifie messages
autorisés par état, réponses, timeouts et fermeture.

Les changements de protocole sont comparés selon ARCH-003 et DATA-001.

### Acteurs distants

Un handle distant est distinct d’un handle local. Il ne garantit ni
colocalisation, ni mémoire partagée, ni supervision identique.

La mailbox distante possède limites côté émetteur et récepteur. La
contre-pression traverse le protocole ou provoque un résultat de saturation.

### Retries

Une opération non idempotente n’est jamais retry automatiquement. Une clé
d’idempotence, sa portée et sa durée de rétention sont définies par le contrat.

### Observabilité

Traces et métriques propagent une corrélation sans exposer données sensibles.
Temps de queue, réseau, traitement et retry sont séparés.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Les capacités sont réduites à un jeton de délégation réseau vérifiable ; un
handle mémoire n’est jamais sérialisé. Le protocole applique authentification,
confidentialité et protection contre replay selon son profil.

## Interactions

- DATA-001
- ARCH-003
- RUN-002 définit `TaskOutcome`, annulation et deadlines ;
- RUN-003 distingue handles et mailboxes d’acteurs ;
- TYPE-004 définit capacités réseau et effets.

## Compatibilité et migration

La version 0.2.0 aligne les opérations distantes sur `Task<T,E>` et maintient
annulation, faute runtime, erreur métier et erreur de transport comme issues
distinctes. Une API qui fusionnait ces cas doit migrer ; ce changement est
source-breaking.

## Tests de conformité

Le simulateur modèle perte, duplication, réordre, partition, expiration,
backpressure et migration de version. Il vérifie séparément succès, erreur
métier, erreur de transport, annulation et faute runtime.

## Questions ouvertes

- Forme standard de `RemoteError` et conservation de causalité entre hops.
