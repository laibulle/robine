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

L'app conserve un cache de présentation et le dernier curseur confirmé par serveur. Il comprend les appareils, pièces, états déjà affichables, habitudes et activité récente normalisée, mais jamais un jeton ni un payload de protocole. Au lancement ou après une reconnexion, elle restaure ce cache pour une interface immédiate, ouvre le WebSocket puis rejoue ou resynchronise les données nécessaires.

L'accueil charge aussi une fenêtre bornée d'activité récente avec `GET
/api/v1/events?tail=N` et les automatisations courantes. Il n'interprète jamais
le payload d'un adaptateur : il affiche seulement le type d'événement normalisé
et sa date.

Un état local potentiellement ancien est marqué comme tel. Hors connexion, aucune commande ne reste en file pour être exécutée plus tard sans confirmation explicite de l'utilisateur. Le serveur reste la source de vérité pour l'état des appareils et les automatismes.

Une commande acceptée reste explicitement en attente dans l'interface. Les
événements `command.confirmed`, `command.failed` et `command.expired` lèvent
cette attente et donnent un retour distinct, même lorsqu'aucun nouvel état
rapporté ne suit.

Les habitudes affichées à l'accueil peuvent être mises en pause ou reprises.
L'app conserve leur source Flow intacte et envoie seulement la nouvelle valeur
`enabled` au `PATCH /api/v1/automations/{id}` ; aucune exécution locale ou
édition implicite du DSL n'est autorisée par ce contrôle rapide.

La vue native « Habitudes » donne accès à la source Flow en lecture et appelle
`POST /api/v1/automations/{id}/simulate` pour une simulation. Le résultat,
les diagnostics et la trace affichés sont ceux du moteur Rust : l’app ne
reconstruit ni le plan ni les branches Flow et la simulation ne commande aucun
appareil.

La même vue propose l’historique borné des exécutions réelles avec
`GET /api/v1/automations/{id}/runs?limit=20`. Chaque entrée présente l’état,
l’instant enregistré et les étapes de trace du runtime, sans exposer de détail
de protocole ni de secret d’adaptateur.

Sur macOS, cette vue ouvre aussi un éditeur de texte Flow. L'enregistrement
reste un `PATCH` atomique sur le serveur ; les erreurs de syntaxe ou de
validation sont montrées sans modifier la dernière version persistée. iOS et
iPadOS conservent volontairement la lecture et la simulation dans cette étape
V1.

Un parcours guidé macOS couvre le premier modèle quotidien : à une heure locale
donnée, allumer (avec luminosité) ou éteindre une lumière sélectionnée. Il rend
le Flow visible avant création, avec une référence d'entité stable et le fuseau
IANA de l'appareil. Il reste un générateur de source Flow soumis au même
`POST /api/v1/automations` que l'éditeur expert.

Sur macOS, le panneau Sauvegarde appelle `POST /api/v1/backups` et affiche le
manifeste reçu (fichier, date, taille et début de checksum). Il rappelle que les
secrets sont exclus. La restauration ne se fait pas à travers l'API vivante :
elle reste une opération de maintenance qui doit d'abord arrêter les
adaptateurs et l'écrivain SQLite.

Le panneau Diagnostic macOS lit `GET /health`, signale les adaptateurs
dégradés, puis détaille `GET /api/v1/adapters` (état, détail, dernière
observation). Il propose une resynchronisation explicite du bridge Hue avec
`POST /api/v1/adapters/hue/synchronize`. Il ne masque pas une indisponibilité :
le message serveur est affiché et les autres surfaces restent utilisables.

## Expérience par plateforme

Sur iPhone, l'app optimise le contrôle rapide : accueil par pièces, favoris, commandes immédiates et retour clair de confirmation. Sur iPad, la même information peut être affichée simultanément en navigation et détail.

Sur macOS, l'app propose une navigation dense, plusieurs fenêtres et les parcours experts : automatisations, trace d'exécution, édition visuelle/textuelle de Flow, appairage et sauvegarde. Le texte Flow reste une vue de l'AST versionné validé par le serveur.

Le premier parcours d'administration partagé permet déjà de créer des pièces,
d'affecter les entités encore sans pièce et d'ajouter un bridge Philips Hue. Il
effectue une découverte locale et récupère le certificat TLS présenté par le
bridge choisi au cours d'une unique connexion d'association, sans redirection.
Cette confiance à la première utilisation est explicitement bornée à ce bridge
local et complétée par l'appui physique exigé par Hue. L'app affiche une
empreinte courte avant de demander cette confirmation, puis transmet le PEM et
son SHA-256 dérivé. Le serveur contrôle l'empreinte avant l'épinglage ; ni le
certificat ni l'empreinte ne sont ajoutés au cache de présentation. La clé
d'application Hue demeure exclusivement dans le trousseau du serveur.

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
