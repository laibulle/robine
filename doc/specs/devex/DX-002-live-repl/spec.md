# DX-002 — REPL vivant et image de programme

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `devex`

## Objet

Faire du REPL une interface complète vers un programme vivant, et non un shell
qui relance des fragments isolés.

## Connexion

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

## Évaluation

Une expression est compilée par DX-001 avec les mêmes règles que le source.
Le REPL NE DOIT PAS utiliser une sémantique d’interpréteur différente.

Les résultats précédents sont accessibles par identifiants stables de session,
pas uniquement par variables globales implicites.

## Redéfinition

Une définition redéfinissable possède une version. Les nouveaux appels
utilisent la version publiée ; un appel en cours termine avec sa version
d’origine.

Les closures et valeurs dépendant d’une ancienne forme conservent leur version
ou exigent une migration selon DX-003.

## `tap`

`tap(value)` publie une observation non bloquante vers des consommateurs
bornés. Dans `realtime`, la publication utilise RT-002 et peut perdre des
événements avec compteur.

## Sécurité

Une session possède des capacités propres. Lire une valeur ne confère pas
automatiquement le droit de déclencher ses effets. Les secrets sont masqués ou
non inspectables selon leur type.

Un REPL de production est désactivé par défaut, authentifié, audité et limité
par architecture.

## Reproductibilité

Une session peut être exportée comme notebook contenant :

- expressions source ;
- versions de modules ;
- entrées capturées ou références ;
- résultats sérialisables ;
- effets non reproductibles signalés.

Les modifications utiles DOIVENT pouvoir être matérialisées comme patches
source afin de ne pas rester uniquement dans l’image.

## UX minimale

Le protocole REPL est structuré et indépendant d’un terminal. Éditeur,
notebook, interface graphique et IA utilisent la même API.
