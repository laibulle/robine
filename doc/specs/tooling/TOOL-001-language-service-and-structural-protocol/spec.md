# TOOL-001 — Service de langage et protocole structurel

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `tooling`

## Objet

Garantir que terminal, éditeur, REPL, notebook, debugger et IA utilisent le même
moteur sémantique au lieu de réimplémenter le langage.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Service

Le service de langage est une façade sur DX-001. Il fournit :

- diagnostics incrémentaux ;
- complétion et signatures ;
- navigation et références ;
- rename et refactorings ;
- formatage ;
- types, effets, capacités, domaine et ownership au curseur ;
- graphe de dépendances ;
- exécution ciblée via DX-002 ;
- patches AI-001.

### Protocole

Les réponses sont structurées, versionnées et corrélées à :

- snapshot de document ;
- version de module ;
- cible et profil ;
- configuration de features ;
- niveau de compilation.

Une réponse calculée sur un snapshot ancien ne peut être appliquée sans rebase.

### Formatage

Le formatter officiel produit une forme canonique et idempotente. Il préserve
commentaires et identités structurelles autant que possible.

La décision de LANG-002 inclut la capacité du formatter à traiter du source
incomplet.

### Refactorings

Un refactoring est un patch structurel soumis aux mêmes validations qu’AI-001.
Rename respecte scopes, imports, code généré et frontières de sérialisation
explicitement marquées.

### Debug

Les builds de développement conservent une correspondance entre code machine,
versions, IR et source. Le debugger sait qu’une pile peut contenir plusieurs
versions d’une même fonction.

### Extensibilité

Les plugins utilisent des APIs publiques avec capacités séparées. Ils ne
chargent pas des modules internes du compilateur et ne modifient pas la
sémantique du langage.

### CLI

La CLI est un client du même service. Une commande interactive et l’action
équivalente de l’éditeur DOIVENT produire diagnostics et résultats compatibles.

### Télémétrie

La télémétrie est opt-in, documentée et sans source ni secret par défaut. Les
performances du service peuvent être profilées localement sans transmission.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- DX-001
- DX-002
- AI-001
- LANG-002
- TYPE-004 sépare effets et capacités ;
- RUN-004 fournit le domaine et les variantes ;
- ARCH-001 fournit le contrat public affiché.

## Compatibilité et migration

La version 0.2.0 ajoute capacités et domaine aux réponses sémantiques. Les
clients doivent distinguer ces champs au lieu de les reconstruire depuis la
ligne d’effets ; ce changement est ABI-breaking pour le protocole.

## Tests de conformité

La suite de conformité DOIT couvrir :

- affichage séparé du type, des effets, capacités et domaine ;
- variante de domaine reliée à sa définition ;
- réponse périmée refusée sans rebase ;
- résultats compatibles entre CLI et éditeur ;
- source map multi-version dans le debugger.

## Questions ouvertes

- Compatibilité du protocole avec des clients ignorant un nouveau champ
  sémantique.
