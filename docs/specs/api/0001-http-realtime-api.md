# 0001 — API HTTP et temps réel

## Objectif

Exposer une API locale, versionnée et cohérente pour les apps natives iOS/macOS, la console Leptos minimale, les intégrations locales et les outils d'administration.

## Principes

- `robine-api-http` est construit avec Actix Web. Actix est un détail d'infrastructure : les handlers adaptent HTTP/WebSocket vers des commandes et requêtes applicatives, sans logique métier.
- Base : `/api/v1`.
- JSON UTF-8 ; les identifiants sont opaques ; les dates sont RFC 3339 en UTC.
- Toute erreur a un code stable, un message lisible et un identifiant de corrélation.
- L'API traduit les entrées vers des cas d'utilisation ; elle ne contient pas de règle métier.
- Les écritures demandent un jeton local ou une session autorisée. Le mode sans authentification n'est permis que durant l'amorçage et sur loopback.

## Contrats clients

`robine-api-contract` définit les DTO et versions de message utilisés par le serveur et la console Leptos. Le build publie aussi un document OpenAPI et des schémas JSON versionnés pour les messages WebSocket et Robine Flow. Les clients Apple génèrent ou maintiennent leurs modèles Swift depuis ces artefacts ; ils ne dépendent pas de crates Rust.

Une modification incompatible exige une nouvelle version d'API ou de message. Les tests de contrat valident le serveur Actix, les DTO Rust et les modèles Swift contre les mêmes fixtures JSON.

## Responsabilités de l'adaptateur Actix

L'adaptateur gère le routage, la limite de taille des requêtes, l'authentification, l'autorisation, la corrélation des requêtes, le mapping d'erreurs, la sérialisation JSON et les connexions WebSocket. Toute opération bloquante, notamment une lecture SQLite, est déléguée au composant de stockage prévu ; un handler Actix ne bloque pas son worker sur une I/O synchrone.

## Ressources V1

| Méthode | Chemin | Usage |
|---|---|---|
| `GET` | `/health` | santé du processus, sans détails sensibles |
| `GET` | `/api/v1/devices` | liste filtrable et paginée des appareils |
| `GET` | `/api/v1/entities/{id}` | détail, capacités et état courant |
| `POST` | `/api/v1/entities/{id}/commands` | demande de commande avec clé d'idempotence |
| `GET` | `/api/v1/events` | historique paginé par curseur |
| `GET/POST/PATCH` | `/api/v1/automations` | lecture et gestion des règles |
| `POST` | `/api/v1/automations/{id}/simulate` | simulation sans effet de bord |
| `GET` | `/api/v1/adapters` | santé et configuration non secrète |

## Plan temps réel asynchrone

HTTP REST est le plan de requête et de commande ; le WebSocket est le plan de notification temps réel. Une commande HTTP destinée à un appareil renvoie `202 Accepted` avec un `command_id` lorsqu'elle est acceptée, puis le résultat réel arrive par événement `command.*` et, éventuellement, `state.*`. L'API ne fait donc jamais attendre une requête HTTP jusqu'à la confirmation physique d'un appareil.

`GET /api/v1/stream` effectue un upgrade WebSocket authentifié, implémenté par `actix-ws`. Le serveur pousse les événements **après leur commit** dans le journal ; un adaptateur ne peut pas publier directement vers un client.

À l'ouverture, le client envoie un unique message `subscribe` avec ses thèmes et son dernier curseur durable :

```json
{
  "type": "subscribe",
  "topics": ["state", "device", "automation", "adapter", "command"],
  "after": 18420
}
```

Le serveur répond par `ready`, puis émet des enveloppes d'événement :

```json
{
  "type": "event",
  "id": 18421,
  "topic": "state",
  "event_type": "state.reported",
  "occurred_at": "2026-08-06T10:15:22.126Z",
  "correlation_id": "cor_...",
  "data_version": 1,
  "data": {}
}
```

`id` est la `sequence` monotone du journal. Le client ne conserve son curseur qu'après avoir appliqué l'événement à sa vue locale. Il peut envoyer `{ "type": "ack", "id": 18421 }` afin d'exposer son retard au serveur ; l'accusé est indicatif et ne constitue pas une garantie de livraison.

Le serveur rejoue d'abord les événements postérieurs à `after`, puis joint le flux direct sans trou. Si le curseur est absent, invalide, ou hors de la rétention, il envoie `resync_required` et ferme proprement la session ; le client relit les ressources HTTP concernées, sauvegarde le nouveau curseur, puis se reconnecte.

Chaque connexion a une file sortante bornée. Un client lent ne peut pas ralentir le moteur d'état : lorsqu'il dépasse cette limite, le serveur envoie `resync_required` si possible puis ferme la connexion. Des messages `ping`/`pong` détectent les clients abandonnés. La connexion ne fournit pas de livraison exactement une fois ; les consommateurs dédupliquent par `id`.

Les messages entrants V1 sont limités à `subscribe`, `ack`, `ping` et `unsubscribe`. Les commandes continuent à passer par HTTP afin de conserver un contrat simple, idempotent et facilement exploitable. Une commande par WebSocket pourra être ajoutée dans une version de protocole ultérieure si un cas de latence le justifie.

## Commandes

La réponse synchrone d'une commande indique seulement l'acceptation ou le refus de la demande. Le résultat final est suivi par les événements `command.*` et `state.*`. Une même clé d'idempotence, pour le même appelant, retourne le résultat initial au lieu d'émettre une seconde commande.

## Critères d'acceptation

- Un client peut reconstituer l'état affiché après une déconnexion via resynchronisation.
- Une requête non autorisée ne révèle ni appareil ni métrique sensible.
- Une commande invalide retourne une erreur déterministe sans atteindre un adaptateur.
- Les schémas de requête et réponse sont publiés et testés contre le serveur.
