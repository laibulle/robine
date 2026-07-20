# TOOL-001 — Service de langage et protocole structurel

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `tooling`

## Objet

Garantir que terminal, éditeur, REPL, notebook, debugger et IA utilisent le même
moteur sémantique au lieu de réimplémenter le langage.

## Service

Le service de langage est une façade sur DX-001. Il fournit :

- diagnostics incrémentaux ;
- complétion et signatures ;
- navigation et références ;
- rename et refactorings ;
- formatage ;
- types, effets et ownership au curseur ;
- graphe de dépendances ;
- exécution ciblée via DX-002 ;
- patches AI-001.

## Protocole

Les réponses sont structurées, versionnées et corrélées à :

- snapshot de document ;
- version de module ;
- cible et profil ;
- configuration de features ;
- niveau de compilation.

Une réponse calculée sur un snapshot ancien ne peut être appliquée sans rebase.

## Formatage

Le formatter officiel produit une forme canonique et idempotente. Il préserve
commentaires et identités structurelles autant que possible.

La décision de LANG-002 inclut la capacité du formatter à traiter du source
incomplet.

## Refactorings

Un refactoring est un patch structurel soumis aux mêmes validations qu’AI-001.
Rename respecte scopes, imports, code généré et frontières de sérialisation
explicitement marquées.

## Debug

Les builds de développement conservent une correspondance entre code machine,
versions, IR et source. Le debugger sait qu’une pile peut contenir plusieurs
versions d’une même fonction.

## Extensibilité

Les plugins utilisent des APIs publiques avec capacités séparées. Ils ne
chargent pas des modules internes du compilateur et ne modifient pas la
sémantique du langage.

## CLI

La CLI est un client du même service. Une commande interactive et l’action
équivalente de l’éditeur DOIVENT produire diagnostics et résultats compatibles.

## Télémétrie

La télémétrie est opt-in, documentée et sans source ni secret par défaut. Les
performances du service peuvent être profilées localement sans transmission.
