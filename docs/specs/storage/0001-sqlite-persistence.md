# 0001 — Persistance SQLite

## Décision

Robine V1 utilise **une unique base SQLite locale** comme stockage persistant principal. Elle contient le registre, les états courants, le journal d'événements, les automatisations Flow, leurs exécutions, les planifications et la configuration non secrète.

Redis, PostgreSQL, InfluxDB, ClickHouse et toute autre base de données sont exclus de la distribution V1. Un cache est en mémoire du processus et n'est jamais une source de vérité.

## Objectifs

- conserver les mutations métier confirmées malgré un arrêt brutal ;
- servir l'état courant avec une lecture indexée et sans relecture du journal ;
- absorber les rapports d'état en rafales sans bloquer l'API ou les adaptateurs ;
- reprendre les automatisations persistantes de manière déterministe ;
- permettre une sauvegarde cohérente pendant que Robine fonctionne ;
- rester installable et exploitable sur une machine locale sans service externe.

## Périmètre et frontières

`robine-application` définit les ports `DeviceRepository`, `StateRepository`, `AutomationRepository`, `Scheduler`, `AuditLog` et les ports de requête associés. Le crate `robine-store-sqlite` est leur unique implémentation V1. Aucun type SQLite, SQL, pool de connexions ou transaction ne traverse la frontière applicative.

Les SDK de protocoles ne se connectent jamais à SQLite. Ils passent par les cas d'utilisation, qui soumettent les mutations au writer du store.

Les secrets ne sont pas stockés dans la base : mots de passe MQTT, clés radio, jetons et matériel cryptographique vivent dans le trousseau du système lorsque disponible, ou dans un magasin chiffré distinct, avec permissions de fichier restrictives. La base ne conserve qu'une référence non sensible à un secret.

## Fichiers et mode SQLite

Par défaut, les données vivent dans un répertoire de données local sous la forme :

```text
robine.sqlite3          # base principale
robine.sqlite3-wal      # journal WAL, géré par SQLite
robine.sqlite3-shm      # index partagé WAL, géré par SQLite
backups/                # snapshots produits par Robine
```

La base est ouverte en mode WAL. Elle doit résider sur un système de fichiers local : un partage NFS, SMB ou un volume synchronisé n'est pas supporté. `foreign_keys` est activé sur chaque connexion. `journal_mode=WAL` est vérifié au démarrage, plutôt qu'appliqué silencieusement à chaque requête.

La durabilité par défaut est `synchronous=FULL`. Une mutation annoncée comme confirmée n'est rendue au cas d'utilisation qu'après le commit SQLite. Une configuration moins stricte est interdite tant qu'un test de coupure d'alimentation sur le matériel cible n'a pas validé explicitement ses garanties.

## Topologie d'accès

SQLite WAL autorise les lecteurs parallèles mais un seul écrivain. Robine rend ce choix explicite :

```text
adaptateurs / API / moteur Flow
              |
       commandes de persistance
              |
      file bornée et instrumentée
              |
       writer SQLite unique
          |             |
       journal      projections
                         |
        pool de lecteurs SQLite
              |
         API / requêtes / UI
```

- Le writer vit sur un thread bloquant dédié ; une API Rust `async` ne doit pas exécuter SQLite directement sur l'exécuteur async.
- Les demandes compatibles sont regroupées en une transaction courte. Les limites initiales sont 256 mutations ou 5 ms, selon la première échéance atteinte.
- Une mutation qui doit être durable avant une réponse peut demander un flush immédiat ; elle reste sérialisée avec les autres écritures.
- La file est bornée. Sa saturation applique une pression de retour aux adaptateurs non critiques et produit une métrique ; elle ne crée pas de mémoire non bornée.
- Les lecteurs n'ouvrent pas de transaction longue. Les listes sont paginées par curseur, et un WebSocket ne conserve jamais une transaction ouverte.
- Les checkpoints WAL sont pilotés en arrière-plan et instrumentés. Une lecture longue qui empêcherait leur progression est signalée comme défaut opérationnel.

## Modèle logique minimal

Les noms ci-dessous décrivent le contrat de stockage ; les migrations peuvent ajouter des colonnes et index sans modifier le sens de ces tables.

| Table / projection | Responsabilité |
|---|---|
| `schema_migrations` | version, checksum et état des migrations appliquées |
| `devices`, `entities`, `entity_capabilities` | registre stable et capacités déclarées |
| `areas`, `floors`, `labels`, `entity_labels` | organisation de la maison |
| `events` | journal append-only de toutes les mutations métier persistées |
| `entity_state` | projection courante, une ligne par propriété d'entité |
| `commands` | demande, acheminement, confirmation et erreur d'une commande |
| `flows`, `flow_revisions`, `flow_runs` | DSL Robine Flow, versions et exécutions |
| `scheduled_jobs` | délais et réveils persistables du moteur Flow |
| `state_rollups` | agrégats temporels par entité, propriété et granularité |
| `adapter_config`, `adapter_health` | configuration non secrète et dernier état connu |
| `audit_log` | opérations administratives et diagnostics structurés |

### Journal d'événements

`events` porte un `sequence` entier strictement croissant, un `event_id` opaque unique, le type, les horodatages de source et d'enregistrement, l'entité éventuelle, les identifiants de corrélation/causalité, la version de schéma de payload et le payload versionné.

`sequence` est l'ordre de lecture global du journal et le curseur des flux temps réel. L'ordre métier par entité est garanti également par `state_version`, stocké dans l'événement et la projection. Un événement n'est jamais modifié ou supprimé individuellement ; la rétention supprime seulement des plages complètes conformément à la politique documentée.

Les champs utilisés pour filtrer sont structurés et indexés. Le payload peut être encodé en binaire versionné pour éviter de faire de SQLite un moteur de documents ; les représentations JSON ne sont construites qu'à la frontière API. Les métadonnées techniques d'adaptateur et l'AST Robine Flow peuvent être stockés comme JSON versionné, mais l'état courant et les index de requête ne sont pas des blobs JSON.

### Projection d'état

`entity_state` est indexée par `(entity_id, property_key)`. Une propriété conserve sa valeur typée, son unité, sa qualité, ses deux horodatages et la dernière `state_version` appliquée. Une valeur numérique indexable est stockée dans une colonne numérique dédiée ; les booléens et valeurs textuelles ne passent pas par une conversion implicite.

L'append de `StateReported`, la mise à jour de `entity_state` et l'enregistrement d'une commande confirmée correspondante appartiennent à la même transaction. Une projection est donc toujours reconstruisible à partir du journal et ne peut pas devenir visible sans événement correspondant.

## Transactions et diffusion

Pour toute mutation métier, le writer suit cet ordre :

1. valide la version et les invariants transmis par le cas d'utilisation ;
2. ajoute l'événement au journal ;
3. met à jour toutes les projections concernées, commandes ou réveils ;
4. commit la transaction ;
5. publie l'événement confirmé vers le flux interne et les abonnés API.

La diffusion n'est jamais une condition du commit. Un client WebSocket peut perdre une notification, mais peut toujours reprendre depuis son dernier `sequence` ou recevoir `resync_required` lorsque ce curseur est sorti de la fenêtre de rétention.

## Historique, agrégation et rétention

Les événements bruts sont conservés pour l'explication et le débogage ; ils ne sont pas conservés indéfiniment par défaut. Un job de compaction, isolé du writer critique, construit des buckets `min`, `max`, `sum`, `count`, `first`, `last` pour les propriétés numériques qui déclarent une politique d'historique.

Politique par défaut, configurable par installation et par propriété :

| Niveau | Rétention par défaut | Usage |
|---|---:|---|
| événements et observations bruts | 30 jours | audit, traces, graphique détaillé |
| agrégats minute | 90 jours | courbes récentes |
| agrégats heure | 2 ans | tendances et énergie |
| agrégats jour | durée de vie de la base | historique long terme |

La compaction ne supprime le brut qu'après validation des agrégats correspondants, et opère par petites transactions interrompables. Une limite d'espace disque configurable prévaut sur cette politique : Robine avertit avant une compaction forcée et consigne la plage retirée dans l'audit.

## Recherche

La recherche par nom, alias, pièce, étiquette ou texte de règle utilise d'abord des index relationnels. Si la recherche plein texte devient nécessaire, elle utilise FTS5 dans le même fichier SQLite, alimenté depuis les tables de contenu. Aucun moteur de recherche externe n'est introduit en V1.

## Sauvegarde et restauration

Une sauvegarde est un snapshot SQLite cohérent obtenu par l'API de sauvegarde en ligne ou `VACUUM INTO`, jamais par copie naïve du fichier principal pendant l'exécution. Elle comprend :

- le snapshot de la base ;
- un manifeste avec version de Robine, version de schéma, date, taille et SHA-256 ;
- une archive des secrets **exclue** par défaut et, si explicitement demandée, chiffrée séparément.

La restauration vérifie le manifeste et la checksum, effectue un snapshot préventif de la base courante, teste l'ouverture et l'intégrité de la base importée, puis applique les migrations compatibles avant de démarrer les adaptateurs. En cas d'échec, Robine restaure le snapshot préventif et ne démarre pas sur un état partiellement restauré.

## Migrations et intégrité

Chaque migration est une unité transactionnelle numérotée, immuable et accompagnée de son checksum. Le démarrage refuse une base dont l'historique de migrations ou l'intégrité SQLite échoue. Les migrations qui transforment un payload Flow ou événementiel créent une nouvelle révision et conservent l'ancienne jusqu'à la réussite de la sauvegarde suivante.

Les clés étrangères, contraintes d'unicité et `CHECK` encodent les invariants de stockage simples. Les invariants métier restent dans le domaine et les cas d'utilisation ; ils ne sont pas dupliqués exclusivement en SQL.

## Observabilité

`robine-store-sqlite` expose au minimum : profondeur et temps d'attente de la file, taille et âge du WAL, durée/erreurs de transaction, taille de base, durée de checkpoint, latence des lectures, nombre de migrations et âge de la dernière sauvegarde vérifiée.

Une base impossible à ouvrir ou dont l'intégrité est compromise met le serveur dans l'état `unhealthy`. Une erreur ponctuelle de lecture ou un checkpoint en retard est `degraded` si les garanties de commit restent satisfaites.

## Découpage Rust

```text
robine-store-model       # structures de projection et contrats de mapping, sans SQLite
robine-store-sqlite      # connexion, migrations, writer, readers et repositories
robine-store-backup      # snapshot, manifeste, vérification et restauration
```

`robine-store-sqlite` utilise une liaison SQLite synchrone, avec le writer dédié décrit plus haut. Le choix de la crate de liaison n'est pas une abstraction métier ; V1 privilégie `rusqlite` pour cet adaptateur. Aucun crate de store ne connaît HTTP, Leptos ou un protocole domotique.

## Critères d'acceptation

- Une mutation confirmée existe à la fois dans `events` et dans ses projections après redémarrage forcé du processus.
- Les lectures d'état et de registre continuent pendant une rafale d'écritures, sans transaction de lecture longue.
- La file atteint sa limite de façon observable et applique une pression de retour plutôt que d'augmenter sans limite.
- Un même `event_id` ne peut pas être écrit deux fois, y compris après une reprise.
- La relecture d'un curseur `sequence` reconstruit la suite des événements sans lacune dans la fenêtre de rétention.
- Une sauvegarde lancée pendant des écritures se restaure en base SQLite cohérente.
- La restauration d'une archive corrompue n'altère pas la base active.
- Les tests de coupure du processus aux limites de transaction ne produisent jamais de projection durable sans événement associé.
