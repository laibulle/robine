# 0001 — Console Web Leptos

## Objectif

Fournir une interface locale réactive pour visualiser, contrôler et administrer Robine, sans dupliquer la logique métier du serveur. La console est une SPA Leptos compilée en WebAssembly et servie comme asset statique par Actix Web.

## Écrans V1

- **Tableau de bord** : entités épinglées, indisponibilités et dernières automatisations.
- **Appareils** : recherche, filtres, détail des entités et renommage.
- **Entité** : état, commandes permises, historique court et statut de dernière commande.
- **Automatisations** : liste, création/édition déclarative, activation, historique et simulation.
- **Adaptateurs** : santé, statut de connexion et configuration non secrète.
- **Système** : version, sauvegarde, restauration et diagnostics exportables.

## Architecture de la console

- `robine-web` utilise Leptos avec rendu côté client (CSR) en V1. Le SSR et l'hydratation sont explicitement hors périmètre.
- Les contrats JSON HTTP et WebSocket sont définis dans `robine-api-contract`, crate Rust sans dépendance à Actix ni Leptos, partagée par `robine-api-http` et `robine-web`.
- Les composants Leptos transforment les DTO de l'API en état de présentation. Les entités du domaine et les types SQLite ne sont jamais compilés dans le navigateur.
- Le code spécifique navigateur est isolé derrière de petites abstractions WebAssembly ; l'interface privilégie HTML, CSS et SVG. Une interopération JavaScript est autorisée uniquement lorsqu'un besoin navigateur n'a pas d'équivalent WASM mature, et doit être encapsulée dans un crate dédié.
- La compilation WebAssembly est versionnée avec le serveur. Actix sert le bundle avec des noms de fichiers immuables et cacheables ; `index.html` reste non cacheable afin de référencer la version courante.

## Principes d'interface

- Leptos consomme exclusivement l'API HTTP et le WebSocket documentés.
- L'état serveur est mis en cache côté client mais reste revalidable ; le navigateur ne décide jamais de l'état final d'une commande.
- Toute commande affiche les états `en attente`, `confirmée`, `échouée` ou `expirée` et offre un message d'erreur actionnable.
- Une déconnexion temps réel est visible, avec resynchronisation automatique sans rechargement de page.
- L'interface est utilisable au clavier, possède des libellés accessibles et ne dépend pas de la couleur seule pour transmettre un état.

## Sécurité navigateur

Les jetons ne sont pas placés dans l'URL. La console applique une politique de contenu restrictive, échappe les données d'appareil et ne rend pas de HTML issu d'un payload de protocole.

## Critères d'acceptation

- Un changement d'état externe apparaît dans l'interface sans rechargement.
- Après une perte puis un retour de réseau local, l'interface retrouve une vue cohérente.
- Une action refusée par le serveur est rendue visible sans optimisme persistant.
- Les parcours de contrôle de base sont navigables au clavier et annoncés aux lecteurs d'écran.
