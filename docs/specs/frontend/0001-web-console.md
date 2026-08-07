# 0001 — Console Web minimale

## Objectif

Offrir une console Web locale de secours, accessible sans installer l'app native. Elle sert à amorcer Robine, appairer les intégrations, diagnostiquer le serveur et récupérer l'administration. Elle n'est pas l'interface quotidienne de la maison.

La console est une SPA Leptos compilée en WebAssembly et servie comme asset statique par Actix Web.

Son langage visuel et interactionnel suit [ux/0001 — Interface cocooning autour de Robine](../ux/0001-cocooning-husky-ui.md), sans étendre son périmètre fonctionnel minimal.

## Périmètre V1

- **Amorçage** : création de l'accès administrateur local et réglages réseau de base.
- **Intégrations** : découverte, appairage du bridge Hue, état de santé, resynchronisation et import explicite des pièces/zones suggérées.
- **Système** : version, sauvegarde, restauration, diagnostics exportables et journal d'audit.
- **Récupération** : liste des appareils/entités, état courant et commande simple si l'app native n'est pas disponible.
- **Expert** : lecture des automatisations et de leurs traces ; édition Flow seulement si l'éditeur natif macOS n'est pas disponible.

Tableau de bord riche, contrôle quotidien par pièce et expériences mobiles ne font pas partie de cette console. Ils relèvent des apps Apple définies dans [clients/0001 — Apps Apple natives](../clients/0001-apple-native-apps.md).

## Architecture

- `robine-web` utilise Leptos avec rendu côté client (CSR). Le SSR et l'hydratation sont hors périmètre.
- Les contrats JSON HTTP et WebSocket sont définis dans `robine-api-contract`, crate Rust sans dépendance à Actix ni Leptos.
- Les composants Leptos transforment les DTO API en état de présentation. Les entités du domaine et les types SQLite ne sont jamais compilés dans le navigateur.
- Le code spécifique navigateur est isolé derrière de petites abstractions WebAssembly. Une interopération JavaScript est autorisée uniquement lorsqu'un besoin navigateur n'a pas d'équivalent WASM mature et doit être encapsulée.
- La compilation WebAssembly est versionnée avec le serveur. Actix sert le bundle avec des noms de fichiers immuables et cacheables ; `index.html` reste non cacheable afin de référencer la version courante.

## Principes d'interface

- Leptos consomme exclusivement l'API HTTP et le WebSocket documentés. Avant
  d'ouvrir le flux navigateur, il échange le bearer en mémoire contre une
  session HttpOnly, `SameSite=Strict`, limitée à dix minutes et au chemin
  `/api/v1/stream` ; le bearer n'apparaît donc ni dans l'URL WebSocket ni dans
  le JavaScript du handshake.
- L'état serveur est mis en cache côté navigateur mais reste revalidable ; le navigateur ne décide jamais de l'état final d'une commande.
- Toute commande affiche les états `en attente`, `confirmée`, `échouée` ou `expirée` avec un message actionnable.
- Une déconnexion temps réel est visible, avec resynchronisation automatique sans rechargement de page.
- Le flux est mono-session : démarrer une nouvelle écoute invalide la chaîne de
  reconnexion précédente, et l'utilisateur peut l'arrêter explicitement. Aucun
  callback d'un ancien WebSocket ne peut réouvrir un abonnement fermé.
- L'interface est utilisable au clavier, possède des libellés accessibles et ne dépend pas de la couleur seule pour transmettre un état.

## Sécurité navigateur

Les jetons ne sont pas placés dans l'URL. La console applique une politique de contenu restrictive, échappe les données d'appareil et ne rend pas de HTML issu d'un payload de protocole.

## Critères d'acceptation

- Une installation neuve peut appairer un bridge Hue entièrement depuis la console locale.
- Un administrateur peut sauvegarder, restaurer et exporter un diagnostic sans app native.
- Après une perte puis un retour de réseau local, l'interface retrouve une vue cohérente.
- Une action refusée par le serveur est rendue visible sans optimisme persistant.
- Les parcours d'amorçage et de récupération sont navigables au clavier et annoncés aux lecteurs d'écran.
