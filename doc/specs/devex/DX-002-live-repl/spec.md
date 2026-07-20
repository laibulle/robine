# DX-002 — REPL vivant et image de programme

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `devex`

## Objet

Faire du REPL une interface complète vers un programme vivant, et non un shell
qui relance des fragments isolés.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Connexion

Un REPL se connecte à une image locale ou distante selon une capacité
d’administration explicite. Il connaît module courant, cible, versions de code
et ressources inspectables.

Il peut :

- évaluer une expression dans un contexte ;
- définir ou redéfinir un symbole ;
- inspecter valeurs, types, effets et source ;
- appeler des fonctions et injecter des messages ;
- exécuter tests et benchmarks ciblés ;
- observer des taps et traces ;
- demander plans mémoire et calcul ;
- préparer une transaction de reload.

### Évaluation

Une expression est compilée par DX-001 avec les mêmes règles que le source.
Le REPL NE DOIT PAS utiliser une sémantique d’interpréteur différente.

Les résultats précédents sont accessibles par identifiants stables de session,
pas uniquement par variables globales implicites.

### Redéfinition

Une définition redéfinissable possède une version. Les nouveaux appels
utilisent la version publiée ; un appel en cours termine avec sa version
d’origine.

Les closures et valeurs dépendant d’une ancienne forme conservent leur version
ou exigent une migration selon DX-003.

### `tap`

`tap(value)` publie une observation non bloquante vers des consommateurs
bornés. Dans `realtime`, la publication utilise RT-002 et peut perdre des
événements avec compteur.

### Reproductibilité

Une session peut être exportée comme notebook contenant :

- expressions source ;
- versions de modules ;
- entrées capturées ou références ;
- résultats sérialisables ;
- effets non reproductibles signalés.

Les modifications utiles DOIVENT pouvoir être matérialisées comme patches
source afin de ne pas rester uniquement dans l’image.

### UX minimale

Le protocole REPL est structuré et indépendant d’un terminal. Éditeur,
notebook, interface graphique et IA utilisent la même API.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Une session possède des capacités propres. Lire une valeur ne confère pas
automatiquement le droit de déclencher ses effets. Les secrets sont masqués ou
non inspectables selon leur type.

Un REPL de production est désactivé par défaut, authentifié, audité et limité
par architecture.

## Interactions

- DX-001
- DX-003
- RT-002

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
