# 0001 — Architecture du serveur

## Objectif

Définir une architecture local-first qui isole le métier de la domotique des protocoles, transports, frameworks et mécanismes de persistance, tout en restant performante et facile à étendre.

## Périmètre

Le serveur Robine gère des appareils, leur état, les commandes, les automatisations et l'observation opérationnelle. Les apps natives iOS et macOS sont les clients quotidiens de l'API locale. La console Leptos exécutée dans le navigateur est limitée à l'amorçage et à l'administration de secours ; aucun client ne contient de règle métier autoritaire.

Hors périmètre V1 : cloud obligatoire, compte utilisateur distant, marketplace de plugins et synchronisation multi-sites.

## Cibles non fonctionnelles V1

Ces valeurs sont des objectifs initiaux à confirmer par benchmark sur le matériel minimal supporté :

- démarrage à service prêt en moins de 3 secondes, hors temps de découverte radio ;
- latence interne p95 inférieure à 20 ms entre un événement d'adaptateur et sa publication locale, sans écriture lente ;
- au moins 10 000 entités connues et 1 000 changements d'état par seconde soutenus sur une machine de référence ;
- aucune connexion sortante nécessaire au fonctionnement normal ;
- redémarrage sans perte des événements confirmés comme persistés.

## Découpage Cargo

Le workspace sépare explicitement les responsabilités :

```text
crates/
  robine-domain/             # entités, value objects, événements, invariants
  robine-application/        # ports et cas d'utilisation
  robine-runtime/            # composition, tâches, démarrage et arrêt
  robine-store-sqlite/       # persistance des ports applicatifs
  robine-api-contract/       # DTO HTTP/WebSocket et schémas sérialisables partagés
  robine-api-http/           # adaptateur Actix Web : REST/WebSocket, authentification locale
  robine-mcp-types/          # schémas MCP et mapping d'erreurs
  robine-mcp-tools/          # adaptation MCP vers les cas d'utilisation
  robine-mcp-http/           # transport MCP Streamable HTTP sur Actix Web
  robine-integration-hue/    # adaptateur du bridge Philips Hue local
  robine-protocol-mqtt/      # adaptateur MQTT
  robine-protocol-zigbee/    # adaptateur Zigbee
  robine-integration-matter/ # adaptateur du contrôleur Matter local
  robine-matterd/            # sidecar contrôleur Matter, isolé du cœur
  robine-observability/      # logs, métriques et traces
  robine-web/                # console Leptos minimale compilée en WASM

apps/apple/                  # apps SwiftUI iOS et macOS ; hors workspace Cargo
```

Les dépendances autorisées sont :

```text
robine-domain <- robine-application <- infrastructure / runtime / robine-api-http
robine-application <- robine-mcp-tools <- robine-mcp-http
robine-mcp-types <- robine-mcp-tools / robine-mcp-http

robine-api-contract <- robine-api-http
robine-api-contract <- robine-web

robine-web -- HTTP/WebSocket uniquement --> robine-api-http
apps/apple -- HTTP/WebSocket uniquement --> robine-api-http
clients MCP -- Streamable HTTP uniquement --> robine-mcp-http
```

`robine-web` ne dépend donc pas du binaire ou du crate Actix : il partage des DTO de contrat, puis communique avec le serveur à travers le réseau local. Les apps Apple consomment les schémas de contrat publiés et ne dépendent d'aucun crate Rust du serveur.

`robine-domain` ne dépend que de la bibliothèque standard et de crates de bas niveau justifiées (par exemple sérialisation d'un value object). Il ne dépend jamais d'un client SQL, d'un runtime async, d'un SDK de protocole ni d'un framework Web.

Actix Web est le framework HTTP retenu en V1. Il reste confiné à `robine-api-http` : ses requêtes, extracteurs, réponses, middlewares et erreurs ne traversent jamais les ports applicatifs.

Le serveur MCP est un adaptateur supplémentaire, distinct de l'API produit. Il offre des outils et ressources MCP sur Streamable HTTP et délègue toute action aux mêmes cas d'utilisation que l'API. Il ne possède ni accès direct au store, ni accès direct à un adaptateur de protocole.

Leptos est le framework de console Web retenu en V1. `robine-web` est une SPA WebAssembly rendue côté client, dont les assets statiques sont servis par Actix. Le SSR et l'hydratation ne font pas partie de V1. Elle couvre uniquement l'amorçage, l'appairage des intégrations, les diagnostics et la récupération administrative. `robine-web` partage uniquement `robine-api-contract` avec l'API ; il ne dépend jamais du domaine, de l'application, d'un store ou d'un adaptateur de protocole.

SwiftUI est le framework client retenu pour les apps iOS et macOS. Leur UX et leur persistance locale relèvent des répertoires Apple, mais elles utilisent strictement le même contrat HTTP/WebSocket versionné que la console Web et les autres intégrations.

## Modèle d'exécution

- Chaque adaptateur traduit son protocole en commandes et événements du modèle canonique.
- Les cas d'utilisation sont les seuls points qui modifient le modèle métier.
- Le moteur d'état sérialise les mutations d'une même entité afin de garantir l'ordre de version.
- La persistance confirme l'événement avant sa diffusion durable ; les abonnés temps réel peuvent recevoir une notification éphémère supplémentaire mais ne doivent pas être source de vérité.
- Les opérations lentes ou bloquantes sont isolées de l'exécuteur async et disposent d'une limite de concurrence.
- Les tâches de protocole sont supervisées par le runtime : une panne est visible, redémarrée avec temporisation et n'arrête pas le cœur.

## Conventions de conception

- Les ports applicatifs sont des traits définis par `robine-application`.
- Les implémentations de port vivent exclusivement en infrastructure.
- Les erreurs métier sont typées ; les détails techniques sont attachés comme contexte, pas exposés au domaine.
- Les entrées/sorties de cas d'utilisation sont des commandes, requêtes et résultats nommés ; pas de types de framework dans leurs signatures.
- Les versions de schéma et de contrat API évoluent de façon compatible ou via une nouvelle version explicite.

## Critères d'acceptation

- Le workspace compile quand aucun crate de protocole n'est activé.
- Un test de dépendances garantit que `robine-domain` et `robine-application` ne dépendent pas de crates d'infrastructure.
- Un adaptateur Hue simulé peut créer une entité et pousser un changement d'état sans réseau ni base de données réelle.
- Un arrêt ordonné cesse les entrées, vide les écritures confirmables, puis ferme les adaptateurs.

## Décisions ouvertes

- Matériel de référence pour rendre les objectifs de performance contractuels.
- Choix du format d'archive et de rétention des événements historiques.

## Décision d'implémentation V1 — authentification locale

La première initialisation est autorisée uniquement depuis loopback. Elle crée
un administrateur local avec mot de passe haché par Argon2id et retourne un
jeton Bearer aléatoire, dont seul le hash est conservé. Toutes les ressources
produit, y compris le flux WebSocket, exigent ensuite ce jeton. Aucun mode
anonyme ne reste actif après l'amorçage.

Le runtime n'accepte HTTP sans TLS que sur une adresse loopback, pour cet
amorçage local. Toute écoute LAN (`ROBINE_BIND` non loopback) exige
`ROBINE_TLS_CERT` et `ROBINE_TLS_KEY`, des chemins PEM lus au démarrage ;
l'absence ou la configuration partielle est un échec de démarrage. Les apps
Apple utilisent alors HTTPS et WSS avec la chaîne configurée, sans désactiver
la validation de certificat.
