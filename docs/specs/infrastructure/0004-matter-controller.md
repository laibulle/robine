# 0004 — Contrôleur Matter local

## Objectif

Permettre à Robine de contrôler localement des appareils Matter sur Wi‑Fi, Ethernet ou Thread, tout en isolant la complexité du contrôleur, du commissioning et des fabrics du cœur Rust.

Matter est un protocole de contrôle sur IP ; Thread est un transport maillé éventuel, pas une intégration indépendante à implémenter par Robine V1.

## Architecture sidecar

Le contrôleur Matter s'exécute dans un processus séparé, `robine-matterd`, supervisé par `robine-runtime`. `robine-integration-matter` est le seul adaptateur qui lui parle au travers d'un RPC local versionné et authentifié. Le cœur ne lie ni SDK Matter, ni stack Thread, ni clé de fabric.

```text
apps Apple / API
       |
robine-application
       |
robine-integration-matter
       |
  RPC local privé et versionné
       |
robine-matterd  <---->  appareils Matter / réseau Thread
```

Le sidecar est arrêté et redémarré indépendamment. Son indisponibilité place uniquement l'adaptateur Matter en `degraded`; le serveur Robine, SQLite, Hue, MQTT et les automatisations restent actifs.

## Fabric, identité et secrets

Robine possède sa propre fabric Matter. Chaque appareil est identifié par `(fabric_id, node_id, endpoint_id)` dans l'infrastructure, puis par les identifiants opaques Robine dans le domaine.

Les clés opérationnelles, certificats, données de commissioning, certificats d'appareil et clés Thread sont des secrets du contrôleur. Elles sont chiffrées dans son magasin privé, référencées par Robine sans être exposées au store, au MCP, à l'API ou aux diagnostics. Une restauration de fabric nécessite une sauvegarde de secrets chiffrée et une confirmation administrative explicite.

## Portée fonctionnelle V1

| Fonction Matter | Capacités Robine | V1 |
|---|---|---|
| On/Off | `switch` | oui |
| Level Control | luminosité en pourcentage | oui |
| Color Control | couleur et température de couleur si supportées | oui |
| capteurs usuels | température, humidité, contact, mouvement | oui |
| prises et énergie | interrupteur, puissance, énergie si cluster exposé | oui |
| thermostat/climate | lecture et consignes de base | oui |
| groupes, scènes, bridging avancé | à définir | non |
| OTA firmware | à définir après validation matériel | non |
| contrôle d'un réseau Thread ou border router | hors périmètre | non |

Une découverte ne rend visibles que les endpoints et clusters effectivement pris en charge. Les clusters inconnus sont conservés comme diagnostic infrastructure, sans contaminer le modèle de domaine.

## Commissioning et multi-admin

L'app macOS ou iOS lance un parcours guidé : scanner ou saisir le code d'appairage, choisir le réseau/fabric, puis suivre l'état détaillé renvoyé par le sidecar. Le code est à usage bref et ne figure pas dans les logs ou l'historique d'audit en clair.

V1 privilégie aussi l'ajout d'un appareil déjà contrôlé par Apple Home ou un autre contrôleur via son mécanisme de partage multi-admin/Matter. Cette approche préserve l'usage Apple Home plutôt que de déplacer ou réinitialiser un appareil.

Pour Matter over Thread, Robine exige un Thread border router opérationnel et compatible avec la fabric concernée. L'app affiche ce prérequis et ne prétend pas qu'un logo Thread garantit la compatibilité Matter.

## Synchronisation et commandes

`robine-matterd` informe l'adaptateur des changements d'attribut, de disponibilité et de topologie. L'adaptateur convertit ces notifications en événements canoniques, persiste l'état via les cas d'utilisation et réconcilie l'inventaire à la connexion ou après reprise.

Une commande Robine devient une invocation de cluster/end-point validée par le sidecar. Le succès de transport et le rapport d'attribut sont distincts, conformément au moteur d'état. Le sidecar limite les tentatives, garde les timeouts de protocole et ne bloque jamais l'exécuteur principal.

Les opérations lentes de commissioning ou de lecture d'inventaire sont modélisées comme jobs persistants avec progression. Elles ne bloquent ni l'API Actix, ni les connexions WebSocket, ni les autres adaptateurs.

## Réseau et disponibilité

Le contrôleur ne fait aucune dépendance au cloud. Les appareils Matter Wi‑Fi/Ethernet doivent être accessibles sur le LAN IPv6 ; les appareils Thread transitent par un border router. Les échecs de résolution, d'atteignabilité ou de subscription sont visibles par appareil et par adaptateur.

Après une déconnexion, le sidecar restaure ses subscriptions, l'adaptateur compare l'inventaire et l'état, puis marque les entités de nouveau disponibles seulement après une synchronisation réussie. Les dernières valeurs restent consultables avec leur qualité et leur ancienneté.

## Découpage Rust et processus

```text
robine-matter-contract       # RPC local, jobs et événements normalisés du sidecar
robine-matterd               # contrôleur Matter isolé et son magasin de fabric
robine-integration-matter    # mapping endpoint/cluster -> capacités Robine
```

`robine-matter-contract` est versionné et ne dépend d'aucun type du domaine. Le choix concret de stack Matter dans `robine-matterd` est isolé et doit être évalué contre une matrice de matériel réel avant implémentation. Aucun crate du cœur ne dépend de ce choix.

## Critères d'acceptation

- Le serveur Robine démarre et continue à servir Hue, MQTT, API et Flow lorsque `robine-matterd` est indisponible.
- L'appareil Matter déjà présent dans Apple Home peut être ajouté à la fabric Robine sans casser son contrôle Apple lorsque le périphérique supporte le partage multi-admin.
- Une redécouverte conserve les `DeviceId` et `EntityId` Robine pour le même endpoint Matter.
- Une commande n'est pas signalée comme état confirmé avant le rapport d'attribut correspondant.
- Les secrets de fabric ne sont présents ni dans SQLite, ni dans les réponses API/MCP, ni dans les logs de test.
- Un endpoint ou cluster inconnu ne fait pas échouer la synchronisation des capacités supportées.
- Le commissioning, le redémarrage de sidecar et une perte de Thread/LAN sont testables avec un faux contrôleur déterministe.
