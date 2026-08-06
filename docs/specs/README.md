# Spécifications Robine

Robine est un serveur de domotique local-first, performant et extensible. Son cœur est écrit en Rust ; son interface Web est écrite en React.

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
| infrastructure | [0001-protocol-adapters](infrastructure/0001-protocol-adapters.md) | contrats d'adaptation des protocoles et connecteurs |
| api | [0001-http-realtime-api](api/0001-http-realtime-api.md) | API locale HTTP et flux temps réel |
| frontend | [0001-web-console](frontend/0001-web-console.md) | console React d'administration et de contrôle |
| operations | [0001-observability-recovery](operations/0001-observability-recovery.md) | journalisation, métriques, sauvegarde et reprise |

## Règles de lecture

Les sections **Critères d'acceptation** sont vérifiables en tests automatisés ou en tests de recette. Les sections **Décisions ouvertes** ne bloquent pas le développement du noyau mais doivent être tranchées avant d'implémenter la partie concernée.
