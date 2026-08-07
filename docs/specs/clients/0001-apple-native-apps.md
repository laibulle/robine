# 0001 — Apps Apple natives

## Objectif

Les apps iOS et macOS constituent l'expérience quotidienne de Robine. Elles offrent le contrôle par pièce, la consultation d'état et d'historique, ainsi que l'administration adaptée à chaque plateforme, tout en gardant le serveur Rust comme unique autorité métier.

Les apps sont écrites en Swift avec SwiftUI, dans `apps/apple/`, hors workspace Cargo. Elles partagent un socle client Swift et des vues adaptées à iPhone, iPad et macOS.

Le package fournit `RobineApp`, une cible exécutable SwiftUI commune. Son premier
écran associe une URL locale HTTPS et un jeton ; le jeton est stocké dans le
trousseau et l'URL non secrète dans les préférences. HTTP n'est toléré que sur
loopback pour le développement local.

Le langage visuel et interactionnel commun est défini dans [ux/0001 — Interface cocooning autour de Robine](../ux/0001-cocooning-husky-ui.md).

## Périmètre V1

| Surface | iOS / iPadOS | macOS |
|---|---|---|
| contrôle quotidien par pièce | oui | oui |
| détail d'entité, commandes et historique | oui | oui |
| alertes d'indisponibilité et dernières automatisations | oui | oui |
| édition visuelle de Flow et simulation | lecture/simulation | oui |
| appairage et diagnostics avancés | guidage minimal | oui |
| sauvegarde/restauration | non | oui, avec confirmation explicite |

La publication de notifications distantes et le contrôle hors réseau local sont hors périmètre V1. Quand une app iOS est en arrière-plan ou arrêtée, Robine ne promet pas de maintenir un WebSocket actif ; elle resynchronise à son retour au premier plan.

## Architecture client

```text
RobineApp (iOS / macOS)
  ├── RobineUI             # vues SwiftUI et design system
  ├── RobineClient          # HTTP, WebSocket, auth, reprise et DTO générés
  └── RobineFeature         # état de présentation et parcours par plateforme
                 |
        HTTPS REST + WSS
                 |
            serveur Robine
```

`RobineClient` utilise `URLSession` pour les requêtes HTTP et le WebSocket. Il applique le protocole défini par l'API : `subscribe`, curseur `after`, déduplication par `id`, `ack` indicatif et resynchronisation HTTP après `resync_required`.

Les DTO Swift viennent des contrats OpenAPI et JSON Schema publiés par le serveur. Le client ne réimplémente ni les invariants métier, ni l'interpréteur Flow, ni un accès direct au store. Toute commande passe par l'API et est suivie de son événement de résultat.

## État local et réseau

L'app conserve un cache de présentation et le dernier curseur confirmé par serveur. Au lancement ou après une reconnexion, elle restaure ce cache pour une interface immédiate, ouvre le WebSocket puis rejoue ou resynchronise les données nécessaires.

L'accueil charge aussi une fenêtre bornée d'activité récente avec `GET
/api/v1/events?tail=N` et les automatisations courantes. Il n'interprète jamais
le payload d'un adaptateur : il affiche seulement le type d'événement normalisé
et sa date.

Un état local potentiellement ancien est marqué comme tel. Hors connexion, aucune commande ne reste en file pour être exécutée plus tard sans confirmation explicite de l'utilisateur. Le serveur reste la source de vérité pour l'état des appareils et les automatismes.

## Expérience par plateforme

Sur iPhone, l'app optimise le contrôle rapide : accueil par pièces, favoris, commandes immédiates et retour clair de confirmation. Sur iPad, la même information peut être affichée simultanément en navigation et détail.

Sur macOS, l'app propose une navigation dense, plusieurs fenêtres et les parcours experts : automatisations, trace d'exécution, édition visuelle/textuelle de Flow, appairage et sauvegarde. Le texte Flow reste une vue de l'AST versionné validé par le serveur.

L'accessibilité repose sur les contrôles natifs SwiftUI, des libellés explicites, des états non seulement colorés et des valeurs vocalisables (luminosité, température, disponibilité et résultat de commande).

## Sécurité

La connexion est locale et chiffrée. Les apps respectent les politiques de transport Apple et ne désactivent pas globalement la validation TLS. L'identité du serveur est vérifiée lors de l'association ; les identifiants de session sont conservés dans le trousseau système, jamais dans une URL ou dans le cache de présentation.

## Critères d'acceptation

- Une même app partagée compile pour iOS et macOS, avec navigation adaptée à chaque plateforme.
- Un changement d'état Hue fait dans l'application Hue apparaît dans l'app Robine sans polling normal lorsque celle-ci est active.
- Après une interruption réseau, l'app reprend le flux depuis son curseur ou effectue une resynchronisation cohérente.
- Une commande affiche la différence entre acceptation, confirmation rapportée, échec et expiration.
- Une app iOS revenue au premier plan ne présente pas son cache comme un état temps réel avant resynchronisation.
- Les fixtures de contrat JSON sont lues à l'identique par le serveur et le client Swift.
