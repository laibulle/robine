# 0001 — Serveur MCP Robine

## Objectif

Robine expose un serveur Model Context Protocol (MCP) local afin qu'un agent ou un assistant IA puisse comprendre l'état de la maison, expliquer les automatisations et, lorsqu'il est explicitement autorisé, demander des actions domotiques.

Le MCP n'est pas un second cœur de domotique. C'est un adaptateur : il appelle les mêmes cas d'utilisation que l'API HTTP et ne lit jamais SQLite, le bridge Hue ou les secrets directement.

## Transport et compatibilité

Le serveur implémente MCP **Streamable HTTP**, révision de protocole `2025-11-25`, sur le point d'entrée unique `POST` et `GET` `/mcp`. Les messages sont JSON-RPC 2.0 UTF-8. Le transport HTTP+SSE historique et `stdio` ne font pas partie de la distribution Robine V1.

`robine-mcp-http` expose le transport via Actix Web ; `robine-mcp-tools` porte l'adaptation de chaque primitive MCP vers les commandes et requêtes de l'application. Les détails du protocole MCP et les types Actix ne franchissent pas ces crates.

Le serveur déclare ses capacités au cours de l'initialisation et retourne listes de tools, resources et prompts dans un ordre déterministe. Les évolutions de protocole MCP sont évaluées explicitement avant mise à jour : aucune nouvelle primitive expérimentale n'est activée implicitement.

## Frontières de sécurité

MCP donne à un modèle la possibilité de demander des actions sur une maison. La politique est donc restrictive par défaut :

- le serveur est lié à `localhost` par défaut ; l'exposition sur le LAN exige HTTPS, une configuration explicite et une règle de pare-feu dédiée ;
- tout appel valide un en-tête `Origin` lorsqu'il est présent ; une origine inconnue est refusée ;
- toute requête exige un jeton Bearer local, lié au serveur MCP, à durée de vie limitée et conservé par le client hors de l'URL ;
- les jetons portent des scopes `robine:read`, `robine:control`, `robine:automation:write` et `robine:admin` ; `robine:read` seul est le défaut ;
- les résultats ne contiennent jamais secrets, clé Hue, token, chemin local ni payload brut de protocole ;
- la journalisation d'audit enregistre l'identifiant non secret du jeton, outil, empreinte des arguments normalisés et résultat, sans secret ni valeur domotique brute.

La mise en œuvre HTTP d'autorisation évoluera vers OAuth 2.1/PKCE avant toute exposition non locale ou distribution à des tiers. Le jeton local V1 ne peut être créé que depuis une session administrateur Robine, avec choix des scopes et date d'expiration.

## Outils V1

Les outils lisent des vues stables de l'application, ou soumettent une intention de commande. Leur schéma d'entrée est strict, versionné et ne comporte pas de champ libre interprété comme code.

| Outil | Scope | Effet |
|---|---|---|
| `robine.home.summary` | `read` | résumé de santé, pièces, appareils indisponibles et alertes |
| `robine.devices.list` | `read` | appareils et entités filtrés/paginés |
| `robine.entities.get` | `read` | capacités et état courant d'une entité explicite |
| `robine.history.query` | `read` | historique borné par entité, propriété et période |
| `robine.automations.list` | `read` | définitions, statut et dernière exécution |
| `robine.automation.explain` | `read` | trace d'une exécution et explication structurée |
| `robine.command.request` | `control` | demande de commande sur une entité explicite |
| `robine.automation.simulate` | `read` | simulation Flow sans effet de bord |
| `robine.automation.set-enabled` | `automation:write` | active ou désactive une automatisation explicite |

Les outils de lecture sont non destructifs. `robine.command.request` et `robine.automation.set-enabled` sont annotés comme potentiellement destructifs, utilisent une clé d'idempotence et sont audités. Ils ne ciblent jamais une pièce, une étiquette ou « toutes les lumières » en V1 : l'appelant doit fournir un `EntityId` ou `FlowId` opaque unique.

Une commande retourne l'acceptation et un `command_id`; sa confirmation effective se consulte avec `robine.entities.get` ou `robine.history.query`. Le serveur ne prétend pas qu'une lumière est allumée avant un état rapporté.

## Ressources et prompts

Les ressources fournissent du contexte en lecture seule :

- `robine://home/summary` ;
- `robine://devices/{device_id}` ;
- `robine://entities/{entity_id}` ;
- `robine://automations/{flow_id}` ;
- `robine://automation-runs/{run_id}`.

Elles retournent la même projection autorisée que les outils de lecture et sont soumises aux mêmes scopes. Les abonnements à des ressources et les notifications MCP persistantes ne sont pas activés en V1 : les agents interrogent les ressources ou utilisent les outils. Le flux WebSocket de l'API produit reste le seul canal de push d'état temps réel.

Les prompts V1 sont `robine.explain-home-status` et `robine.explain-automation-run`. Ils ne font que préparer un contexte et ne déclenchent aucune commande.

## Autorisation d'actions

Le scope `robine:control` n'est pas une permission implicite donnée à tous les agents. Lors de la création du jeton, l'administrateur choisit une politique :

- `read-only` : seuls les outils de lecture sont exposés ;
- `confirm-each` : le client MCP doit présenter un `approval_id` à usage unique, créé dans une app Robine après confirmation humaine ;
- `allow-listed` : les commandes sont autorisées seulement pour les `EntityId` et verbes explicitement listés dans le jeton, avec plafond de durée et de fréquence.

`confirm-each` est la valeur par défaut lorsqu'un scope d'écriture est demandé. Un `approval_id` est lié au jeton MCP, à l'outil, aux arguments JSON normalisés et à une expiration courte ; toute différence invalide l'approbation. Il est consommé atomiquement par SQLite avant l'appel au cas d'utilisation et toute tentative, acceptée ou refusée, est auditée. Les actions d'administration et de sauvegarde ne sont jamais exposées comme outils MCP en V1.

### Parcours `confirm-each` implémenté

1. La session administrateur crée `POST /api/v1/auth/mcp-tokens` avec `scopes: ["robine:read", "robine:control"]`. La réponse contient le bearer une seule fois et un `token_id` non secret.
2. Après confirmation humaine dans le client Robine, la session administrateur crée `POST /api/v1/auth/mcp-approvals` avec ce `token_id`, le nom de l'outil et les arguments **sans** `approval_id`. L'approbation dure cinq minutes par défaut (30 secondes à une heure).
3. Le client MCP soumet exactement les mêmes arguments, plus `approval_id`. Le serveur retire ce seul champ, calcule l'empreinte canonique, consomme l'approbation une fois, puis transmet la commande au cas d'utilisation.

### Parcours `allow-listed` implémenté

L'administrateur peut émettre `POST /api/v1/auth/mcp-tokens` avec une
`write_policy` `allow_listed`, une liste non vide de couples `EntityId`/propriétés
et un `max_commands_per_hour` entre 1 et 3 600. Les identifiants d'entité sont
validés comme UUID avant que le jeton ne soit émis. Une commande ne demande pas
`approval_id` lorsqu'elle correspond exactement à cette liste ; le quota est
réservé atomiquement dans SQLite dans une fenêtre UTC d'une heure, puis l'appel
emprunte le même cas d'utilisation que toute autre commande. Chaque acceptation
et chaque dépassement sont audités. Une liste blanche n'autorise jamais la
modification d'automatisations : celles-ci restent en `confirm-each`.

## Limites et fiabilité

- Chaque appel dispose de limites de taille d'entrée, temps d'exécution, pagination et résultats retournés.
- Les requêtes de lecture utilisent les ports de requête ; elles ne maintiennent pas de transaction SQLite ouverte.
- Les appels d'écriture passent par le même writer, les mêmes validations et les mêmes quotas que les apps natives.
- Une erreur MCP traduit l'erreur métier en JSON-RPC structuré sans exposer de détail d'infrastructure.
- Les outils n'acceptent pas de texte Flow, SQL, URL ou expression arbitraire en V1 ; la définition d'une automatisation se modifie uniquement par les parcours API validés ultérieurs. Seule l'activation/désactivation explicitement listée plus haut est autorisée.

## Découpage Rust

```text
robine-mcp-types       # schémas de tools/resources/prompts et mapping d'erreurs
robine-mcp-tools       # handlers MCP -> cas d'utilisation, sans Actix
robine-mcp-http        # Streamable HTTP, JSON-RPC et auth HTTP sur Actix
```

`robine-mcp-types` dépend de types de contrat purs, jamais de l'application. `robine-mcp-tools` dépend de ports applicatifs. `robine-mcp-http` dépend du transport MCP, de l'authentification et de `robine-mcp-tools`, mais ne connaît pas SQLite ni Hue.

## Critères d'acceptation

- Un client MCP conforme découvre les outils, ressources et prompts sur `/mcp` et peut appeler les outils de lecture avec un jeton `read`.
- Une requête sans jeton, avec scope insuffisant ou avec origine invalide est refusée avant tout appel applicatif.
- Un jeton `read` ne voit aucun outil de contrôle dans `tools/list`.
- Une demande de commande sans `approval_id` conforme à la politique est refusée et auditée sans atteindre Hue.
- Une commande MCP confirmée suit exactement le même chemin, quotas, journal et événements qu'une commande depuis une app Apple.
- Un résultat MCP ne contient jamais de secret connu injecté dans un test.
- Le serveur reste fonctionnel si le serveur MCP est désactivé ou si un client MCP se comporte mal.
