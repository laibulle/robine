# AI-001 — Patches structurels typés

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `ai`

## Objet

Permettre à une IA ou un outil de modifier le programme par intention
structurelle plutôt que par remplacement textuel fragile.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Modèle

Le compilateur expose une représentation `Syntax<T>` versionnée contenant :

- identité stable des nœuds ;
- symboles résolus ;
- types, effets et ownership ;
- commentaires et positions source ;
- origine des expansions ;
- dépendances sémantiques.

Un patch cible identités et préconditions :

```text
replace node N
expected version V
preserve public_api, effects, realtime
```

### Opérations

Le protocole minimal comprend :

- insérer, supprimer, remplacer, déplacer ;
- renommer un symbole avec ses références ;
- ajouter ou modifier import ;
- extraire fonction ou type ;
- ajouter test, contrat ou implémentation ;
- transformer un pattern ou pipeline.

Les opérations sont indépendantes de la syntaxe source canonique.

### Diff humain

Tout patch structurel possède un rendu source canonique et un résumé :

- intention ;
- symboles modifiés ;
- changements d’API, effets et dépendances ;
- tests ajoutés ou affectés ;
- risques et validations.

### Conflits

Deux patches sur nœuds indépendants peuvent fusionner. Un conflit sémantique
est signalé même si les lignes diffèrent. Un rebase utilise identités,
résolution de symboles et préconditions, jamais uniquement les offsets texte.

### Format

Le protocole est sérialisable, versionné et testable sans modèle IA. Les
éditeurs et refactorings classiques utilisent la même API.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

L’IA reçoit une vue filtrée par capacités. Les secrets et modules non autorisés
ne sont pas inclus dans le contexte structurel.

## Interactions

Aucune interaction normative supplémentaire n’est déclarée.

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

Un patch n’est applicable que si :

- sa base correspond ou peut être rebasée sans ambiguïté ;
- les invariants syntaxiques sont respectés ;
- le programme affecté typecheck ;
- les politiques d’architecture passent ;
- les propriétés demandées sont préservées ;
- les capacités nouvelles sont approuvées.

Une IA NE PEUT PAS déclarer elle-même qu’une propriété est préservée ; le
compilateur ou les validations fournissent l’évidence.

## Questions ouvertes

Aucune à ce stade.
