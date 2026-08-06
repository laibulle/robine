# 0001 — Observabilité, sauvegarde et reprise

## Objectif

Permettre d'exploiter Robine localement, de diagnostiquer une panne et de restaurer les données sans compromettre les secrets.

## Journalisation et métriques

Les logs sont structurés et incluent horodatage, niveau, crate/composant, identifiant de corrélation et identifiants opaques pertinents. Les valeurs d'état, payloads bruts et secrets sont masqués par défaut.

Les métriques V1 couvrent au minimum : disponibilité et reconnexions des adaptateurs, file de commandes, latence de persistance, débit d'événements, exécutions d'automatisations, connexions WebSocket et mémoire/processus.

Un endpoint d'état distingue :

- `healthy` : le processus et ses dépendances indispensables sont opérationnels ;
- `degraded` : le cœur fonctionne mais au moins un adaptateur ou composant optionnel est indisponible ;
- `unhealthy` : le serveur ne peut pas fournir ses garanties de cohérence.

## Sauvegarde et restauration

Une sauvegarde cohérente contient la base d'état et d'événements, les définitions d'automatisations, la version de schéma et un manifeste avec checksum. Les secrets sont exclus par défaut et réinjectés par un mécanisme d'administration séparé.

La restauration valide le manifeste, effectue une sauvegarde préventive de l'état courant et applique les migrations nécessaires avant de redémarrer les adaptateurs. Elle est refusée si une incompatibilité de schéma ne peut pas être migrée.

## Reprise au démarrage

Au démarrage, Robine vérifie l'intégrité du store, applique les migrations, restaure les projections, identifie les exécutions d'automatisation inachevées puis démarre les adaptateurs. Les règles et l'API ne deviennent disponibles qu'après la reconstruction du cœur ; la découverte radio peut continuer ensuite.

## Critères d'acceptation

- Un export puis une restauration sur une instance vide retrouvent appareils, état, automatisations et historique dans la limite de la rétention.
- Un journal ne contient pas un secret connu injecté au test.
- L'arrêt brutal pendant une écriture ne produit pas de projection acceptée sans événement correspondant.
- L'endpoint de santé passe à `degraded` lorsque MQTT échoue tout en laissant l'API locale fonctionnelle.
