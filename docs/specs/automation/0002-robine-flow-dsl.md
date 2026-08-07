# 0002 — Robine Flow DSL

## Objectif

Robine Flow est le DSL déclaratif des automatisations Robine. Il doit être assez expressif pour les scénarios d'une maison, agréable à lire pour une personne qui aime Elixir ou Lisp, et suffisamment restreint pour être validé, simulé, visualisé et exécuté de manière déterministe.

Il ne constitue pas un langage de programmation généraliste : pas de fonctions utilisateur, récursion, macros, boucles non bornées, accès au réseau, système de fichiers, processus, FFI, ni évaluation dynamique.

## Principes

- **AST canonique** : une définition est stockée sous forme d'arbre versionné ; le texte et les éditeurs visuels natif ou Web sont deux vues de ce même arbre.
- **Syntaxe homoiconique** : le texte utilise des S-expressions. Chaque forme est visible, imbriquée et se transforme directement en nœud d'AST.
- **Lecture fluide** : verbes et options par mots-clés s'inspirent d'Elixir (`:to`, `:brightness`, `:mode`) ; les séquences sont explicites et sans ponctuation superflue.
- **Typage avant activation** : une règle ne peut être sauvegardée active que si ses références, unités, capacités, options et bornes sont valides.
- **Effets contrôlés** : seules les formes d'action peuvent déclencher une commande ou changer la configuration d'automatisation. Une expression ne produit aucun effet de bord.
- **Explicable** : chaque évaluation, branche et action génère une étape de trace lisible par l'interface.

## Exemple

Allumer l'entrée sur mouvement si elle est sombre, puis l'éteindre deux minutes après la fin du mouvement :

```clojure
(flow
  (meta :name "Éclairer l'entrée" :mode :restart :max-runtime 10m)

  (on (state-changed (entity "ent_motion_entry") :motion :from false :to true))

  (when
    (< (state (entity "ent_lux_entry") :illuminance) 20%))

  (do
    (command (entity "ent_light_entry") :turn-on :brightness 40%)
    (await (state-changed (entity "ent_motion_entry") :motion :to false))
    (wait 2m)
    (command (entity "ent_light_entry") :turn-off)))
```

Les identifiants d'exemple sont opaques. L'éditeur affiche le nom courant de l'entité ; le nom n'est jamais utilisé pour résoudre une commande à l'exécution.

## Représentations et cycle de vie

```text
éditeur visuel <-> AST Flow JSON <-> rendu texte Flow
                         |
                      validation
                         |
                 plan d'exécution immuable
                         |
                  exécution + trace
```

L'AST JSON est la source de vérité persistée. Le serveur conserve facultativement le texte source et les commentaires comme métadonnées d'édition, mais ne les exécute jamais directement. Tout changement — visuel, import JSON ou texte — passe par le parseur, le validateur et une normalisation avant persistance.

Le rendu texte est déterministe : une même version d'AST produit le même texte formaté. Ainsi, le mode expert et l'éditeur graphique ne divergent pas.

## Syntaxe V1

### Lexique

- Les symboles sont en kebab-case : `state-changed`, `turn-on`, `max-runtime`.
- Les mots-clés commencent par `:` : `:from`, `:to`, `:mode`.
- Les chaînes sont UTF-8 entre guillemets et utilisent les échappements JSON.
- Les booléens sont `true` et `false`; `nil` représente une absence explicite.
- Les nombres n'ont pas d'unité implicite. Les unités acceptées V1 sont `%`, `ms`, `s`, `m`, `h`, `d`, `°C`, `W`, `Wh`, `kWh`.
- Les commentaires commencent par `;` et courent jusqu'à la fin de la ligne. Ils ne participent pas à la sémantique.

Grammaire compacte :

```ebnf
flow        = "(" , "flow" , meta? , inputs? , trigger , guard? , body , ")" ;
meta        = "(" , "meta" , { keyword , literal } , ")" ;
inputs      = "(" , "inputs" , { input } , ")" ;
trigger     = "(" , "on" , trigger-expression , ")" ;
guard       = "(" , "when" , boolean-expression , ")" ;
body        = "(" , "do" , { action } , ")" ;
expression  = literal | reference | "(" , symbol , { expression | keyword , expression } , ")" ;
```

La grammaire décrit la forme. La liste blanche de formes et le système de types définissent les programmes valides.

### Forme racine

Un fichier ou une saisie contient exactement une forme `(flow ...)`, dans cet ordre :

1. `(meta ...)`, optionnelle ;
2. `(inputs ...)`, optionnelle et réservée aux modèles réutilisables ;
3. `(on ...)`, obligatoire ;
4. `(when ...)`, au plus une fois ;
5. `(do ...)`, obligatoire.

`meta` accepte en V1 : `:name`, `:description`, `:mode` (`:single`, `:restart`, `:queue`), `:max-runs`, `:max-runtime`, `:ignore-self` et `:enabled`.

La politique est appliquée atomiquement par SQLite avant toute commande : `single`
retourne une exécution `skipped` s'il en existe déjà une, `restart` annule les
exécutions actives ou en file et en conserve la trace, et `queue` persiste une
FIFO. Pour `queue`, `:max-runs` (1 à 32) borne le total des exécutions actives
et en attente ; une arrivée au-delà de la borne est tracée comme `skipped`.

## Types

Les types de Flow sont déterminés à partir du registre de capacités, pas de conventions de nommage :

| Famille | Exemples | Règle |
|---|---|---|
| scalaires | `Bool`, `Int`, `Decimal`, `String` | aucune conversion implicite de chaîne vers nombre |
| valeurs dimensionnées | `Percentage`, `Temperature`, `Power`, `Energy`, `Duration` | comparaison et arithmétique seulement entre unités compatibles |
| temporels | `Instant`, `LocalTime`, `Weekday` | les horaires sont évalués avec un fuseau explicite |
| références | `EntityRef<capability>`, `AreaRef`, `LabelRef` | une référence doit exister et avoir le type demandé |
| structurels | `Option<T>`, `List<T>` bornée, `Map<String, T>` bornée | jamais de structure récursive ni d'itération libre |
| événement | `Event<T>` | seulement dans la portée du déclencheur actif |

Une valeur `unavailable`, `invalid` ou absente ne se convertit pas en `false`, `0` ou chaîne vide. Toute opération qui la rencontre retourne `nil`; une condition `nil` ne passe pas. L'auteur doit utiliser `available?`, `present?` ou une valeur de repli explicite.

Les unités sont converties par le type-checker avant comparaison. Par exemple, `20°C` peut être comparé à une température en Fahrenheit ; `20` ne peut pas l'être.

## Références et modèle de données

`(entity "ent_...")` crée une référence vers une entité stable. Le type-checker vérifie les propriétés et commandes contre les capacités connues :

```clojure
(state (entity "ent_lux_entry") :illuminance)
(command (entity "ent_light_entry") :turn-on :brightness 40%)
```

Les sélecteurs d'organisation sont possibles là où l'action les autorise : `(area "area_entry")` et `(label "label_night")`. Au moment de l'exécution, ils sont résolus en ensemble stable d'entités, inclus dans la trace. Une action de groupe est atomique du point de vue de la règle, mais chaque commande physique possède son propre résultat.

Les modèles réutilisables déclarent des entrées typées :

```clojure
(inputs
  (input :motion (entity-ref :capability :motion))
  (input :light (entity-ref :capability :light))
  (input :timeout (duration :default 2m :min 0s :max 30m)))
```

Une instance de modèle remplace ces entrées par des valeurs validées avant compilation. Elle possède ensuite son propre `FlowId` et sa propre version : une mise à jour de modèle propose une migration, elle ne modifie jamais silencieusement une automatisation existante.

## Expressions pures V1

| Catégorie | Formes |
|---|---|
| état et événement | `state`, `attribute`, `event`, `available?`, `present?` |
| logique | `all`, `any`, `not`, `if` |
| comparaison | `=`, `!=`, `<`, `<=`, `>`, `>=`, `between?`, `changed?` |
| valeurs | `+`, `-`, `min`, `max`, `clamp`, `coalesce` |
| temps | `now`, `time-between?`, `weekday?`, `sun` |
| collections bornées | `contains?`, `count`, `one?`, `every?` |

`if` est une expression et évalue une seule branche. Il ne sert pas à faire du contrôle d'action ; celui-ci utilise `choose`.

Les expressions sont sans mutation. Il n'existe pas de variable globale. `let` est permis uniquement pour nommer une valeur locale dans la même expression ou action ; ses liaisons sont immuables et lexicales :

```clojure
(let ((lux (state (entity "ent_lux_entry") :illuminance)))
  (all (available? lux) (< lux 20%)))
```

## Déclencheurs V1

| Forme | Rôle |
|---|---|
| `(state-changed ref :property key :from value? :to value?)` | changement d'état normalisé d'une entité |
| `(event :type name :where predicate?)` | événement de domaine ou d'appareil autorisé |
| `(schedule :at "HH:MM" :weekdays [... ] :timezone "Europe/Paris")` | planification civile locale |
| `(any-of trigger...)` | déclenchement sur la première source correspondante |

Pour les changements d'état, `:from` et `:to` sont optionnels mais au moins un filtre ou une propriété est requis. `event` ne peut cibler que des types déclarés exposables par le cœur ou un adaptateur ; un payload brut de protocole ne peut jamais être déclencheur.

Lors d'une ambiguïté d'heure d'été, `schedule` s'exécute une fois avec la première occurrence. Lors d'une heure locale inexistante, son occurrence est ignorée et l'événement est inscrit dans la trace opérationnelle.

`schedule` accepte `(schedule :at "HH:MM" :weekdays [mon tue wed thu fri] :timezone "IANA/Zone")`. Les crochets sont une liste Flow et sont normalisés en parenthèses par le formatter. L'horaire est évalué chaque minute locale et dédupliqué par date, heure, fuseau et branche ; il peut être composé avec `event` ou `state-changed` dans `any-of`.

## Actions V1

Une action produit un résultat `succeeded`, `failed`, `skipped`, `cancelled` ou `timed-out`. Chaque résultat est inclus dans la trace.

| Forme | Sémantique |
|---|---|
| `(command ref verb options...)` | demande une commande validée ; succès seulement sur la politique de confirmation demandée |
| `(wait duration)` | attend une durée positive bornée |
| `(await trigger :timeout duration?)` | suspend jusqu'à un événement correspondant, persistable après redémarrage |
| `(choose predicate then-actions else-actions?)` | choisit exactement une séquence d'actions |
| `(parallel :join :all\|:any actions...)` | exécute des branches bornées en concurrence |
| `(retry action :times n :backoff duration)` | réessaie une action idempotente avec des bornes explicites |
| `(activate flow-ref)` / `(deactivate flow-ref)` | modifie l'état d'une autre automatisation autorisée |
| `(audit :message string :data expression?)` | ajoute une information structurée à la trace |

`do` exécute les actions dans l'ordre. `parallel` est limité à 32 branches et `retry` à 10 tentatives au total (`:times` inclut la première tentative). En V1, `retry` enveloppe une unique action idempotente non suspendante — `command`, `activate`, `deactivate` ou `audit` — et persiste l’index de la prochaine tentative avant chaque backoff. Une commande possède un identifiant d'idempotence dérivé de `(RunId, action-path, attempt)`.

L'action `command` accepte `:confirm :transport` (défaut), `:reported` ou `:none`. Avec `:reported`, elle attend l'état correspondant jusqu'au timeout de l'action ; l'absence de confirmation n'est jamais interprétée comme un succès.

Les actions de configuration utilisent une référence explicite : `(deactivate
(flow "<FlowId>"))` ou `(activate (flow "<FlowId>"))`. La cible doit exister
à la validation ; à l'exécution, la mutation passe par le même cas d'usage que
l'API, est idempotente si l'état demandé est déjà atteint et produit une étape
de trace `automation_changed`.

## Sémantique d'exécution

1. Un événement persistant correspond au déclencheur.
2. Le moteur crée un `RunId`, mémorise la chaîne de causalité et applique la politique de concurrence.
3. La garde est évaluée sur un snapshot cohérent de l'état courant ; son résultat est tracé.
4. Les actions sont compilées en plan immuable puis exécutées. Les lectures faites pendant une action observent l'état à cet instant et la trace en garde la valeur.
5. Les attentes, délais et tentatives enregistrent un point de reprise durable.
6. La fin, l'annulation ou l'échec produit un résultat terminal et libère la politique de concurrence.

Une exécution ne peut pas déclencher indéfiniment sa propre règle. Chaque événement porte `correlation_id`, `causation_id` et profondeur. `:ignore-self` vaut `true` par défaut ; au-delà de la profondeur globale définie par le runtime, l'exécution est bloquée avec un diagnostic explicite.

### Garde causale V1

Le runtime V1 persiste une réservation atomique `(FlowId, correlation_id)` avant toute exécution événementielle. Une commande issue d'un Flow réutilise la même corrélation dans ses événements `requested`, `dispatched`, `confirmed`, `failed` et `expired`; un même Flow ne peut donc pas reconsommer sa propre chaîne, y compris après redémarrage ou après un `wait`. La chaîne est limitée à 32 exécutions distinctes et les réservations sont conservées 30 jours pour couvrir les rejeux du journal.

Le consommateur interne du runtime conserve séparément son curseur dans SQLite.
Après un redémarrage ou un retard du canal de diffusion, il rejoue le journal
persisté dans l’ordre à partir de ce curseur, puis déduplique les notifications
directes déjà vues. Une base sans curseur initialise celui-ci à la fin du
journal : l’activation initiale ne rejoue donc jamais l’historique d’avant
l’installation du moteur.

### Attentes persistantes V1

`(await (event :type "…") :timeout duration?)`, `(await (state-changed (entity "…") :property "…" :to value?) :timeout duration?)` et leurs compositions `(await (any-of …) :timeout duration?)` sont compilés en déclencheurs versionnés, enregistrés avec le plan et la position de reprise. SQLite les exclut du scheduler de délais tant que le timeout n'est pas atteint ; chaque événement ou état déjà persisté est comparé au déclencheur, puis reprend exactement la première action suivante. Sans timeout, la suspension reste durable après redémarrage. Le timeout reprend le plan sans prétendre qu'un événement a été reçu. Les déclencheurs racine et d'attente `(any-of …)` combinent les branches `event` et `state-changed`.

## Validation et diagnostics

La validation se fait à trois niveaux :

1. **Syntaxe** : parenthèses, littéraux, positions des mots-clés et une unique forme racine.
2. **Structure et types** : formes autorisées, options connues, références, unités, ports de commande et bornes de ressources.
3. **Déploiement** : disponibilité des capacités et autorisation de l'appelant à contrôler les entités ciblées.

Un diagnostic contient un code stable, une sévérité, un message, une plage de texte quand elle existe, et un chemin JSON Pointer vers le nœud AST. Le frontend peut donc souligner une erreur textuelle et sélectionner le bloc visuel correspondant.

Une automatisation active ne peut pas contenir d'erreur. Les avertissements, notamment une référence temporairement indisponible ou une action large par label, exigent une confirmation explicite dans l'interface.

## Format de persistance

Le store conserve au minimum :

```json
{
  "dsl": "robine-flow",
  "dsl_version": 1,
  "flow_id": "flow_01...",
  "revision": 4,
  "ast": { "type": "flow", "children": [] },
  "source": "(flow ...)",
  "source_hash": "sha256:..."
}
```

Le serveur rejette une version de DSL inconnue en exécution. Une migration est une transformation AST versionnée, testée et réversible par conservation de la révision antérieure. L'AST JSON est documenté par schéma JSON pour l'API, mais le DSL n'est pas du JSON : sa syntaxe experte reste Flow.

## Découpage Rust

Le DSL reste découpé en petits crates :

```text
robine-flow-ast        # nœuds, types et version du DSL, sans I/O
robine-flow-syntax     # lexer, parseur, formatter et diagnostics de source
robine-flow-check      # validation structurelle et de types via ports applicatifs
robine-flow-plan       # compilation AST validé -> plan d'exécution
robine-flow-runtime    # interprétation du plan ; dépend des ports d'automatisation
```

`robine-flow-ast` et `robine-flow-syntax` ne dépendent ni de SQLite, ni d'un runtime async, ni d'un protocole. `robine-flow-runtime` ne connaît les appareils qu'au travers de `CommandDispatcher`, `StateRepository`, `Scheduler` et `AuditLog` de l'application.

## Critères d'acceptation

- Le texte de l'exemple se parse, est validé avec un registre compatible, puis est rendu à l'identique après formatage.
- L'éditeur visuel et l'éditeur texte produisent le même AST normalisé pour une même règle.
- `(< 20% 21%)` est valide ; `(< 20 21%)` produit une erreur de type localisée.
- Une référence à une commande non supportée est refusée avant toute exécution d'adaptateur.
- Une exécution interrompue durant `wait` ou `await` reprend après redémarrage sans répéter les actions déjà confirmées.
- Une simulation compile le même plan après résolution de la garde et de `choose`, puis l'interprète avec un gateway sans effet de bord. Elle retourne donc la même trace de décisions, de branche et d'attente que l'exécution réelle, sans déclencher de commande ni modifier une automatisation.
- Un Flow ne peut pas introduire une boucle, une I/O arbitraire ou une action non bornée par simple texte.
