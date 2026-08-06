# 0002 — Moteur d'état et journal d'événements

## Objectif

Fournir l'état canonique, cohérent et historisable des entités, sans exposer les représentations variables des protocoles.

## État canonique

L'état d'une entité est un document typé par capacité. Chaque propriété possède : valeur normalisée, unité éventuelle, qualité (`reported`, `estimated`, `unavailable`, `invalid`), horodatage de source, horodatage de réception et `StateVersion` monotone.

Exemples : une température est stockée en degrés Celsius ; une luminosité en pourcentage ; un interrupteur en booléen. La conversion d'unité et le décodage de payload appartiennent aux adaptateurs.

## Invariants

- Une modification porte sur une entité existante et active.
- Un événement plus ancien que la dernière version connue ne remplace pas l'état courant, mais peut être archivé comme événement tardif.
- Les valeurs ne respectant pas le schéma de capacité ne sont jamais intégrées à l'état courant.
- Une commande demandée et l'état réellement rapporté sont distingués ; l'état rapporté prévaut.

## Cas d'utilisation

### Appliquer un état rapporté

L'adaptateur fournit une observation normalisée et son horodatage. Le cas d'utilisation valide, compare la version, persiste l'événement `StateReported`, met à jour la projection courante et notifie les abonnés.

### Demander une commande

Le client fournit un `EntityId`, une capacité et une valeur désirée. Le cas d'utilisation valide la commande, crée un `CommandRequested` avec un identifiant de corrélation puis délègue son acheminement au port de commande. L'échec technique donne `CommandFailed`; un succès de transport n'est pas une confirmation d'état.

### Lire l'état et l'historique

L'état courant se lit depuis la projection. L'historique se filtre par entité, propriété et période ; il est paginé et trié chronologiquement.

## Ports applicatifs

- `StateRepository` : écriture atomique des événements et projection courante.
- `CommandDispatcher` : envoi vers l'adaptateur possédant l'entité.
- `EventStream` : diffusion de changements strictement ordonnés par entité.

## Persistance

La source de vérité est un journal append-only. La projection d'état courant est reconstruisible. Les migrations de schéma sont versionnées et transactionnelles. SQLite est la persistance de référence V1, derrière les ports applicatifs.

## Critères d'acceptation

- Après redémarrage, l'état courant est identique à celui obtenu avant l'arrêt confirmé.
- Deux mises à jour concurrentes de la même entité produisent une suite de versions sans trou ni doublon.
- Une commande non confirmée n'est pas affichée comme état rapporté.
- Un abonné qui reprend depuis une version reçoit les événements ultérieurs dans l'ordre pour cette entité.
