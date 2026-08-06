# 0001 — API HTTP et temps réel

## Objectif

Exposer une API locale, versionnée et cohérente pour la console React, les intégrations locales et les outils d'administration.

## Principes

- Base : `/api/v1`.
- JSON UTF-8 ; les identifiants sont opaques ; les dates sont RFC 3339 en UTC.
- Toute erreur a un code stable, un message lisible et un identifiant de corrélation.
- L'API traduit les entrées vers des cas d'utilisation ; elle ne contient pas de règle métier.
- Les écritures demandent un jeton local ou une session autorisée. Le mode sans authentification n'est permis que durant l'amorçage et sur loopback.

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

## Flux temps réel

`GET /api/v1/stream` ouvre un WebSocket authentifié. Le client s'abonne explicitement aux thèmes `state`, `device`, `automation` et `adapter`. Chaque message contient un identifiant d'événement, un type, un horodatage et une charge versionnée.

Le client peut fournir le dernier identifiant reçu. Si le serveur ne peut plus rejouer ce point, il envoie `resync_required`; le client doit relire les ressources concernées. La connexion ne garantit pas la livraison exactement une fois.

## Commandes

La réponse synchrone d'une commande indique seulement l'acceptation ou le refus de la demande. Le résultat final est suivi par les événements `command.*` et `state.*`. Une même clé d'idempotence, pour le même appelant, retourne le résultat initial au lieu d'émettre une seconde commande.

## Critères d'acceptation

- Un client peut reconstituer l'état affiché après une déconnexion via resynchronisation.
- Une requête non autorisée ne révèle ni appareil ni métrique sensible.
- Une commande invalide retourne une erreur déterministe sans atteindre un adaptateur.
- Les schémas de requête et réponse sont publiés et testés contre le serveur.
