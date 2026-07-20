# UI-002 — Live UI et patches sémantiques

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `ui`

## Objet

Conserver le levier d’un modèle LiveView : état serveur durable, événements
typés et mises à jour compactes, avec équité et contre-pression.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Arbre sémantique

Une vue produit un arbre possédant :

- identités stables ;
- rôle et propriétés ;
- relations d’accessibilité ;
- événements acceptés ;
- frontières de composants ;
- contenu public ou sensible.

Le diff porte sur cet arbre, pas sur une chaîne HTML.

### Patches

Le protocole minimal inclut :

- insertion, suppression et déplacement ;
- modification de texte ou propriété ;
- remplacement de sous-arbre ;
- commandes focus/navigation ;
- accusés de réception et version d’arbre.

Un patch est appliqué uniquement à la version de base attendue. En cas d’écart,
client et serveur négocient replay ou snapshot.

### Sessions

Une session est un acteur RUN-003 avec budgets CPU, heap et mailbox. Les
événements à haute fréquence peuvent être coalescés ; les commandes métier ne
peuvent être perdues sans politique explicite.

La reconnexion utilise un token de session borné, authentifié et révocable.

### Rendu

Le rendu incrémental DEVRAIT recalculer uniquement les dépendances de l’état
modifié. Les fonctions pures sont memoïsables par identité et entrées.

Le serveur NE DOIT PAS conserver un historique non borné de patches.

### Hors ligne

Une vue peut déclarer :

- `online-only`;
- cache en lecture ;
- file locale de commandes idempotentes ;
- synchronisation via protocole de conflit.

Le mode hors ligne n’est pas inféré d’un composant Live UI.

### Mesures

Par session : temps de rendu, taille de patch, profondeur de mailbox, événements
coalescés, mémoire, reconnects et resynchronisations.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Le client est non fiable. Chaque événement est validé contre le protocole,
l’identité utilisateur et l’état courant. Les valeurs sensibles ne sont jamais
incluses dans l’arbre ou les traces par défaut.

## Interactions

- RUN-003

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
