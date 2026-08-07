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
| `GET` | `/api/v1/openapi.json` | contrat OpenAPI versionné publié par le serveur |
| `POST` | `/api/v1/setup/administrator` | amorçage unique de l’administrateur, uniquement depuis loopback |
| `POST` | `/api/v1/auth/tokens` | émet ou récupère localement un nouveau jeton de session après réauthentification |
| `POST` | `/api/v1/auth/stream-session` | crée une session WebSocket navigateur HttpOnly, SameSite et bornée, sans bearer dans l’URL |
| `GET` | `/api/v1/devices` | liste filtrable et paginée des appareils |
| `PATCH/DELETE` | `/api/v1/devices/{id}` | renomme ou retire logiquement un appareil |
| `GET` | `/api/v1/entities/{id}` | détail, capacités et état courant |
| `PATCH` | `/api/v1/entities/{id}` | renomme une entité sans modifier son adresse protocolaire |
| `PUT` | `/api/v1/entities/{id}/area` | affecte l'entité à une pièce, ou la retire avec `area_id: null` |
| `POST` | `/api/v1/entities/{id}/commands` | demande de commande avec clé d'idempotence |
| `GET` | `/api/v1/events` | historique paginé par curseur, ou dernières enveloppes avec `?tail=N` |
| `GET/POST/PATCH` | `/api/v1/automations` | lecture et gestion des règles |
| `POST` | `/api/v1/automations/{id}/simulate` | simulation sans effet de bord |
| `GET` | `/api/v1/automations/{id}/runs` | historique borné des traces d’exécution, de la plus récente à la plus ancienne |
| `GET` | `/api/v1/adapters` | santé et configuration non secrète |
| `GET/POST` | `/api/v1/areas` | lit ou crée les pièces de la maison |
| `GET/POST` | `/api/v1/adapters/hue/discover`, `/pair` | découvre puis associe un bridge Hue épinglé TLS |
| `POST` | `/api/v1/adapters/hue/synchronize` | resynchronisation explicite de l’inventaire Hue |
| `GET/POST` | `/api/v1/adapters/hue/rooms`, `/rooms/import` | suggère des pièces/zones Hue, puis importe explicitement une sélection validée |
| `POST` | `/api/v1/adapters/matter/commission` | démarre une commission Matter asynchrone |
| `GET` | `/api/v1/adapters/matter/jobs/{id}` | suit une commission Matter asynchrone |
| `POST` | `/api/v1/backups` | crée un instantané SQLite vérifié |
| `GET` | `/api/v1/stream` | upgrade WebSocket authentifié et rejeu d’événements |
| `POST` | `/api/v1/auth/mcp-tokens`, `/api/v1/auth/mcp-approvals` | délégation MCP à politique explicite |

`POST /api/v1/setup/administrator` ne porte jamais d’authentification mais le
serveur refuse toute origine non-loopback et tout second amorçage. Il retourne
une fois le premier bearer. Les autres demandes administratives exigent ce
bearer. `POST /api/v1/auth/tokens` demande toujours le mot de passe : depuis
loopback, il permet de récupérer une association perdue sans ancien bearer ;
depuis le LAN, il exige aussi un bearer valide. Le mot de passe seul ne devient
donc jamais une API de connexion exposée au réseau domestique.

`GET /api/v1/devices` retourne une page `{ "devices": [...], "next_cursor": "uuid?" }`. `limit` vaut 50 par défaut et est borné à 100 ; `cursor` est l'identifiant opaque du dernier appareil appliqué et `status` filtre parmi `discovered`, `available`, `unavailable` et `removed`. Le store exécute ce parcours sur son index `(status, nom normalisé, id)` : une page ne charge jamais toute la collection en mémoire.

## Plan temps réel asynchrone

HTTP REST est le plan de requête et de commande ; le WebSocket est le plan de notification temps réel. Une commande HTTP destinée à un appareil renvoie `202 Accepted` avec un `command_id` lorsqu'elle est acceptée, puis le résultat réel arrive par événement `command.*` et, éventuellement, `state.*`. L'API ne fait donc jamais attendre une requête HTTP jusqu'à la confirmation physique d'un appareil.

`GET /api/v1/stream` effectue un upgrade WebSocket authentifié, implémenté par `actix-ws`. Le serveur pousse les événements **après leur commit** dans le journal ; un adaptateur ne peut pas publier directement vers un client.

À l'ouverture, le client envoie un unique message `subscribe` avec ses thèmes et son dernier curseur durable :

```json
{
  "type": "subscribe",
  "topics": ["state", "device", "area", "automation", "adapter", "command"],
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

`GET /api/v1/events?after=<cursor>&limit=<1..500>` est le rejet HTTP du même flux : il répond `{ "events": [<mêmes enveloppes>], "next_cursor": 18421 }`. Un client peut donc employer le même décodeur pour une reprise HTTP et le WebSocket ; une valeur `after` ou `limit` invalide est refusée en `400`. Si `after` désigne une portion déjà purgée (ou un futur), l'endpoint répond `409` avec le code `resync_required` : l'app relit alors ses ressources plutôt que d'appliquer un delta incomplet.

Le serveur rejoue d'abord les événements postérieurs à `after`, puis joint le flux direct sans trou. Si le curseur est absent, invalide, futur, ou hors de la rétention, il envoie `resync_required` et ferme proprement la session ; le client relit les ressources HTTP concernées, sauvegarde le nouveau curseur, puis se reconnecte.

Chaque connexion a une file sortante bornée. Un client lent ne peut pas ralentir le moteur d'état : lorsqu'il dépasse cette limite, le serveur envoie `resync_required` si possible puis ferme la connexion. Des messages `ping`/`pong` détectent les clients abandonnés. La connexion ne fournit pas de livraison exactement une fois ; les consommateurs dédupliquent par `id`.

L'implémentation V1 utilise une file de 128 événements par connexion, alimentée depuis le broadcast SQLite par une tâche distincte de la session Actix. Une saturation ou un retard du broadcast déclenche `resync_required`; les producteurs d'adaptateurs et le writer ne sont jamais suspendus par le socket d'un client.

Les messages entrants V1 sont limités à `subscribe`, `ack`, `ping` et `unsubscribe`. Les commandes continuent à passer par HTTP afin de conserver un contrat simple, idempotent et facilement exploitable. Une commande par WebSocket pourra être ajoutée dans une version de protocole ultérieure si un cas de latence le justifie.

## Commandes

La réponse synchrone d'une commande indique seulement l'acceptation ou le refus de la demande. Le résultat final est suivi par les événements `command.*` et `state.*`. Une commande qui ne reçoit pas de confirmation rapportée avant son délai d'adaptateur devient `command.expired`; elle n'est jamais affichée comme état rapporté. Une même clé d'idempotence, pour le même appelant, retourne le résultat initial au lieu d'émettre une seconde commande.

## Critères d'acceptation

- Un client peut reconstituer l'état affiché après une déconnexion via resynchronisation.
- Une requête non autorisée ne révèle ni appareil ni métrique sensible.
- Une commande invalide retourne une erreur déterministe sans atteindre un adaptateur.
- Les schémas de requête et réponse sont publiés et testés contre le serveur.
