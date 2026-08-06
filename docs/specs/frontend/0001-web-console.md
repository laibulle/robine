# 0001 — Console Web React

## Objectif

Fournir une interface locale réactive pour visualiser, contrôler et administrer Robine, sans dupliquer la logique métier du serveur.

## Écrans V1

- **Tableau de bord** : entités épinglées, indisponibilités et dernières automatisations.
- **Appareils** : recherche, filtres, détail des entités et renommage.
- **Entité** : état, commandes permises, historique court et statut de dernière commande.
- **Automatisations** : liste, création/édition déclarative, activation, historique et simulation.
- **Adaptateurs** : santé, statut de connexion et configuration non secrète.
- **Système** : version, sauvegarde, restauration et diagnostics exportables.

## Principes d'interface

- React consomme exclusivement l'API HTTP et le WebSocket documentés.
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
