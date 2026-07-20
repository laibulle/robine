# LANG-001 — Principes de conception

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `language`

## Objet

Fixer les contraintes qui permettent d’arbitrer les fonctionnalités de Robine.
Ces principes ne remplacent pas une sémantique, mais une spec qui les viole
DOIT justifier ce choix.

## Non-objectifs

Robine ne cherche pas :

- à exécuter sans coût du code arbitrairement dynamique ;
- à partager une UI identique entre toutes les plateformes ;
- à compiler magiquement tout paquet Python ou JavaScript ;
- à prouver automatiquement toute propriété mathématique ;
- à fournir une abstraction universelle de concurrence ;
- à maximiser le nombre d’abstractions architecturales.

## Spécification normative

### Principes normatifs

#### Garanties locales

Une garantie forte DOIT pouvoir être demandée sur le domaine concerné sans
imposer son coût à tout le programme. L’ownership strict, le temps réel, la
préemption équitable et le calcul tensoriel sont des contrats locaux.

#### Une seule sémantique

Les profils `script`, `responsive`, `realtime`, `kernel` et `ui` partagent les
mêmes valeurs, types, modules et erreurs. Ils NE DOIVENT PAS devenir des
langages incompatibles.

#### Données avant objets

Les records, variantes, collections et fonctions sont les abstractions par
défaut. Une identité mutable ou un dispatch dynamique DOIT être explicite.

#### Coûts explicables

Le compilateur DOIT pouvoir expliquer :

- les allocations et copies ;
- les dispatchs dynamiques ;
- les points de préemption ;
- les transferts CPU/NPU ;
- les contrôles de contrats conservés à l’exécution ;
- les dépendances incluses dans un artefact.

#### Zéro runtime obligatoire

Un programme qui n’utilise ni acteurs, ni GC, ni hot reload, ni réflexion NE
DOIT PAS embarquer ces services. « Zéro overhead » signifie que les abstractions
statiquement résolues peuvent être effacées ; il ne signifie pas qu’une
fonctionnalité active est gratuite.

#### Programme vivant, release scellée

Le développement favorise l’indirection versionnée, l’inspection et la
recompilation incrémentale. La release scellée spécialise, inline et retire ces
indirections lorsque l’évolution dynamique n’est plus requise.

#### Architecture exécutable

Les frontières de dépendance, d’effet, de privilège et de propriété DOIVENT
pouvoir être vérifiées par les outils. Un diagramme n’est jamais une source
normative.

#### Sobriété mesurée

La latence, la mémoire, l’énergie et la qualité sont des dimensions de
performance. Les affirmations énergétiques DOIVENT être reliées à une mesure,
un profil matériel et une unité fonctionnelle.

### Critère de retrait

Une fonctionnalité DEVRAIT être retirée ou restreinte si elle empêche
durablement deux des objectifs suivants : compilation incrémentale rapide,
diagnostics locaux, spécialisation native, sécurité des capacités,
interopérabilité ou compréhension humaine du coût.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- META-001 définit le processus qui applique ces principes ;
- TYPE-004 et TYPE-005 séparent effets, capacités et garanties locales ;
- RUN-004 et RUN-005 définissent domaines et runtime synthétisé ;
- CPL-001 définit scellement et équivalence des étages ;
- PKG-001 et PKG-002 définissent projet, autorité et chaîne logicielle.

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
