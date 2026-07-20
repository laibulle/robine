# UI-003 — Runtime Web et îlots React

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `ui`

## Objet

Proposer un rendu Web compact et sécurisé tout en permettant l’usage progressif
de l’écosystème React.

## Non-objectifs

Le runtime Robine ne cherche pas à réimplémenter immédiatement toutes les
bibliothèques React, ni à promettre qu’un composant arbitraire peut s’exécuter
sans embarquer son runtime.

## Spécification normative

### Runtime natif Robine

Le backend Web peut générer DOM, CSS, événements et hydratation ciblée. Il
DEVRAIT :

- mettre à jour directement les dépendances connues ;
- éviter un virtual DOM global obligatoire ;
- découper le code par route ou capacité ;
- éliminer composants et branches inaccessibles ;
- permettre rendu serveur et reprise client.

### Îlot externe

Un composant React est déclaré par un contrat :

```text
props
events
children policy
client/server requirement
package and version
capabilities
```

La frontière sérialise uniquement les valeurs autorisées. Les callbacks sont
des handles bornés dont le lifecycle suit celui de l’îlot.

### Cohérence d’état

Une valeur possède une source d’autorité. Robine et React NE DOIVENT PAS tous
deux considérer le même état comme mutable sans protocole explicite.

Les mises à jour traversant la frontière sont batchées et observables au
profiler.

### Bundle

Le build rapporte :

- octets par route ;
- dépendances directes et transitives ;
- duplication de versions ;
- code d’adaptation ;
- coût des îlots ;
- capacités client.

Un projet peut définir des budgets qui font échouer la release.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Les packages JavaScript sont soumis à PKG-002. Aucun script d’installation ne
s’exécute implicitement. Les APIs navigateur sensibles passent par capacités
et politiques de contenu.

## Interactions

- PKG-002

## Compatibilité et migration

Une application peut commencer avec une racine React puis déplacer composant
par composant vers le runtime Robine, ou conserver durablement des bibliothèques
spécialisées comme éditeur ou visualisation.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
