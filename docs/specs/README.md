# Spécifications Robine

Robine est un serveur de domotique local-first, performant et extensible. Son cœur est écrit en Rust. L'expérience quotidienne cible des apps natives iOS et macOS ; une console Web Leptos minimale sert à l'amorçage et à l'administration de secours.

Chaque spécification suit le chemin `docs/specs/[domain]/[feat-id]-[feat-name].md`. Les numéros sont séquentiels à l'intérieur d'un domaine et ne doivent jamais être réutilisés.

## Principes d'architecture

- Clean Architecture : le domaine ne dépend d'aucun framework, protocole, base de données ou transport.
- Les cas d'utilisation constituent la frontière applicative et dépendent de ports définis par le cœur.
- Les protocoles (Zigbee, MQTT, Matter, etc.) et leurs SDK sont des adaptateurs d'infrastructure.
- Les crates sont petits, à responsabilité unique et possèdent des dépendances directionnelles explicites.
- Le serveur fonctionne sans Internet ; l'accès distant est optionnel et ne fait pas partie du périmètre initial.
- Toute mutation d'état produit un événement de domaine persistant et diffusable.

## Spécifications V1

| Domaine | Spécification | Objet |
|---|---|---|
| platform | [0001-architecture](platform/0001-architecture.md) | frontières, crates et objectifs non fonctionnels |
| core | [0001-device-registry](core/0001-device-registry.md) | identité, capacités et cycle de vie des appareils |
| core | [0002-state-engine](core/0002-state-engine.md) | état normalisé, événements et historique |
| automation | [0001-rule-engine](automation/0001-rule-engine.md) | scénarios et exécution fiable des automatisations |
| automation | [0002-robine-flow-dsl](automation/0002-robine-flow-dsl.md) | DSL déclaratif, typé et éditable visuellement |
| storage | [0001-sqlite-persistence](storage/0001-sqlite-persistence.md) | stockage local, journal, projections, rétention et sauvegarde |
| infrastructure | [0001-protocol-adapters](infrastructure/0001-protocol-adapters.md) | contrats d'adaptation des protocoles et connecteurs |
| infrastructure | [0002-philips-hue-bridge](infrastructure/0002-philips-hue-bridge.md) | première intégration locale avec un bridge Hue |
| infrastructure | [0003-mqtt](infrastructure/0003-mqtt.md) | interopérabilité MQTT et découverte d'appareils locale |
| infrastructure | [0004-matter-controller](infrastructure/0004-matter-controller.md) | contrôleur Matter local isolé dans un sidecar |
| api | [0001-http-realtime-api](api/0001-http-realtime-api.md) | API locale HTTP et flux temps réel |
| frontend | [0001-web-console](frontend/0001-web-console.md) | console Leptos minimale d'administration |
| clients | [0001-apple-native-apps](clients/0001-apple-native-apps.md) | apps iOS et macOS natives, clientes de l'API locale |
| mcp | [0001-server](mcp/0001-server.md) | serveur MCP local pour agents et assistants IA |
| operations | [0001-observability-recovery](operations/0001-observability-recovery.md) | journalisation, métriques, sauvegarde et reprise |

## Règles de lecture

Les sections **Critères d'acceptation** sont vérifiables en tests automatisés ou en tests de recette. Les sections **Décisions ouvertes** ne bloquent pas le développement du noyau mais doivent être tranchées avant d'implémenter la partie concernée.
