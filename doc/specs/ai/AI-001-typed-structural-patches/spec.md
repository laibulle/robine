# AI-001 — Patches structurels typés

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `ai`

## Objet

Permettre à une IA ou un outil de modifier le programme par intention
structurelle plutôt que par remplacement textuel fragile.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Modèle

Le compilateur expose la famille canonique `Syntax<Kind, Phase>` de LANG-002.
Un patch typé cible une vue `Syntax<Kind, Typed>` contenant :

- identité stable des nœuds ;
- symboles résolus ;
- types, effets et ownership ;
- commentaires et positions source ;
- origine des expansions ;
- dépendances sémantiques.

Une opération portant uniquement sur le source PEUT également transporter les
projections lue ou expansée nécessaires au rendu. Elle NE DOIT PAS lire un
symbole, type, effet ou ownership depuis une phase qui ne l’a pas établi. La
phase et la version de chaque précondition font partie du patch.

Un patch cible identités et préconditions :

```text
replace node N
expected version V
preserve public_api, effects, domains
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

- LANG-002 possède la famille `Syntax<Kind, Phase>` ;
- LANG-004 définit scopes, expansion et provenance ;
- DX-001 fournit snapshots, identités et revérification incrémentale ;
- TOOL-001 applique les patches depuis les clients ;
- ARCH-002 vérifie les politiques affectées.

## Compatibilité et migration

La version 0.2.0 remplace `Syntax<T>` par une vue explicitement phasée. Les
patches sérialisés antérieurs doivent déclarer la phase de leurs préconditions
et remplacer toute propriété spéciale `realtime` par la préservation générale
des domaines, ou être refusés ; ce changement est ABI-breaking pour le
protocole.

## Tests de conformité

Un patch n’est applicable que si :

- les phases de ses vues et préconditions correspondent ;
- sa base correspond ou peut être rebasée sans ambiguïté ;
- les invariants syntaxiques sont respectés ;
- le programme affecté typecheck ;
- les politiques d’architecture passent ;
- les propriétés demandées sont préservées ;
- les capacités nouvelles sont approuvées.

Une IA NE PEUT PAS déclarer elle-même qu’une propriété est préservée ; le
compilateur ou les validations fournissent l’évidence.

La suite négative DOIT inclure un patch qui demande types ou symboles depuis
une vue antérieure à la phase typée.

## Questions ouvertes

- Durée de conservation et stratégie de rebase des identités après branches
  divergentes prolongées.
