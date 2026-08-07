# 0001 — Moteur d'automatisations

## Objectif

Exécuter localement des règles déclaratives qui réagissent aux événements de Robine et déclenchent des actions contrôlées, traçables et bornées.

## Modèle de règle

Une règle possède un identifiant, un nom, un statut (`enabled` ou `disabled`), un déclencheur, des conditions, des actions et une politique d'exécution.

- Les déclencheurs V1 sont : changement d'état, arrivée d'un événement d'appareil et planification horaire locale.
- Les conditions évaluent l'état courant, le contenu normalisé de l'événement et l'heure locale.
- Les actions V1 sont : demander une commande d'entité, retarder une séquence, activer/désactiver une règle et écrire une entrée d'audit.
- La politique fixe le mode (`single`, `restart`, `queue`), le nombre maximal total d'exécutions actives ou en file et le délai maximal d'exécution.

Les expressions sont déclaratives et sans accès arbitraire au système de fichiers, réseau ou processus. Une règle n'exécute pas de code utilisateur en V1. Leur représentation, leur syntaxe experte et leur vérification sont définies par [0002 — Robine Flow DSL](0002-robine-flow-dsl.md).

## Exécution

Le moteur consomme le journal d'événements après persistance. Chaque exécution possède un `RunId`, un événement déclencheur et une trace des étapes. Le moteur déduplique un événement déjà traité par une même règle, y compris après reprise.

Une règle ne déclenche pas de boucle infinie : la chaîne de causalité est conservée, la profondeur est limitée et une règle peut ignorer les événements qu'elle a indirectement causés.

## Cas d'utilisation

### Créer ou modifier une règle

Valide le schéma, les références d'entité, les capacités et les bornes d'exécution. La nouvelle définition est atomique et versionnée.

### Activer ou désactiver une règle

Une désactivation empêche les nouvelles exécutions. Une exécution déjà commencée suit sa politique : abandon explicite ou achèvement, enregistré dans l'audit.

### Rejouer une règle

Exécute une simulation sur des événements et états fournis, sans envoyer de commande réelle. Le résultat liste les conditions, actions prévues et effets refusés.

## Ports applicatifs

- `AutomationRepository` : définitions, versions et exécutions.
- `Scheduler` : réveils persistables pour les délais et planifications.
- `CommandDispatcher` : même port que les commandes utilisateur.
- `AuditLog` : journal d'exécution consultable.

## Critères d'acceptation

- Une règle désactivée ne peut lancer aucune nouvelle action.
- Le redémarrage pendant une exécution conserve un état de reprise déterministe.
- Une simulation ne modifie ni l'état des entités ni le journal d'événements métier.
- Une boucle causale dépasse la profondeur limite de façon visible et sans saturer le serveur.
