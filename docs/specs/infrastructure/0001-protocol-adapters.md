# 0001 — Adaptateurs de protocoles

## Objectif

Intégrer des protocoles hétérogènes sans les faire fuiter dans le domaine ou dans les cas d'utilisation.

## Règle d'architecture

Un protocole est de l'infrastructure. Ses trames, clients, SDK, identifiants, temporisations, erreurs et stratégies de reconnexion sont confinés dans un crate `robine-protocol-*`. Aucun type de protocole ne traverse un port applicatif.

## Contrat d'adaptateur

Chaque adaptateur implémente les capacités suivantes :

- démarrer et s'arrêter de façon idempotente ;
- découvrir ou restaurer ses appareils connus ;
- traduire une annonce ou une trame en découverte, disponibilité ou état canonique ;
- recevoir une commande validée, l'encoder puis confirmer au minimum son résultat de transport ;
- exposer son état de santé, sa version et des diagnostics non sensibles ;
- déclarer les capacités effectivement supportées.

L'adaptateur n'écrit jamais directement dans SQLite et ne publie pas vers l'API. Il appelle les cas d'utilisation du cœur via les ports entrants.

## Intégrations V1

La première intégration de production est le bridge Philips Hue local, spécifiée dans [0002 — Bridge Philips Hue](0002-philips-hue-bridge.md). Robine s'adresse au bridge par son API locale ; il ne parle pas Zigbee directement en V1.

MQTT est la prochaine intégration envisagée. Zigbee direct et Matter sont explicitement reportés : ils suivront le même contrat canonique, sans jamais définir le modèle de domaine.

### MQTT

Le crate MQTT gère connexion, souscriptions, reconnexion, qualité de service et mapping configurable des topics/payloads. Il traite les données de découverte et de disponibilité comme des événements externes non fiables jusqu'à validation.

### Zigbee et Matter

Ces adaptateurs encapsulent leur bibliothèque radio ou contrôleur. La découverte peut être lente et continue : elle ne doit pas bloquer le démarrage de l'API ni du moteur d'automatisation.

## Résilience et sécurité

- Une erreur d'adaptateur ne fait pas tomber le runtime ni les autres adaptateurs.
- Les reconnexions appliquent un backoff borné avec gigue.
- Les secrets (mot de passe MQTT, clés radio) sont référencés par une abstraction de secret et ne sont jamais écrits dans les logs ou l'API.
- Les payloads externes ont une taille maximale et sont validés avant désérialisation métier.

## Critères d'acceptation

- Un adaptateur factice couvre la découverte, le rapport d'état, la commande et une panne transitoire.
- La désactivation d'un adaptateur rend ses entités indisponibles sans les supprimer.
- Un mauvais payload MQTT ne peut ni faire tomber le processus ni modifier l'état courant.
- Le domaine et l'application compilent sans dépendance MQTT, Zigbee ou Matter.
