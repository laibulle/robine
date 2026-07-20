# COMP-004 — Précision, énergie, thermique et qualité

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `compute`

## Objet

Faire de la qualité numérique et de l’énergie des contrats mesurables, sans
prétendre les déduire entièrement du source.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Précision

Un tenseur quantifié sépare :

- type mathématique exprimé ;
- type de stockage ;
- type d’accumulation ;
- échelle et zéro ;
- axe, canal ou groupe de quantification ;
- politique d’arrondi et saturation.

Une conversion perdante est explicite ou produite par un outil de quantification
qui émet son rapport de calibration.

### Contrats numériques

Un kernel déclare un mode :

- `strict` : ordre et arrondi prescrits ;
- `bounded(error)` : erreur absolue/relative bornée ;
- `quality(metric >= threshold)` : validé sur protocole de données ;
- `fast(profile)` : transformations agressives et limites fixées par un profil
  versionné.

Le passage à une précision inférieure n’est possible que si le contrat reste
valide.

### Conformité des résultats

Chaque contrat numérique `C` définit une relation testable :

```text
Accepts_C(input, output, evidence)
```

- `strict` accepte uniquement le résultat conforme à l’ordre, aux types,
  arrondis et représentations prescrits ;
- `bounded(error)` accepte un résultat dans la borne annoncée par rapport à la
  référence définie ;
- `quality(metric >= threshold)` accepte l’ensemble de résultats évalué sur
  l’unité, le dataset et le protocole déclarés ;
- `fast(profile)` accepte les comportements explicitement permis par ce profil,
  notamment arrondi, overflow, NaN et reproductibilité.

Deux niveaux de compilation ou deux backends préservent le même contrat
numérique lorsqu’ils utilisent le même `C` et que chacun de leurs résultats
satisfait `Accepts_C`. Cela NE signifie PAS que leurs valeurs sont égales pour
`bounded`, `quality` ou `fast`.

Un consommateur qui exige l’égalité de chaque appel DOIT demander `strict` ou
un profil déterministe qui fixe également variante, algorithme et précision.
Un contrat `quality` au niveau d’un dataset NE DOIT PAS être présenté comme une
borne par entrée.

### Budgets

Un service peut définir :

```text
latency.p95
energy.p95
memory.peak
temperature ceiling
quality floor
```

Chaque budget est associé à :

- profil matériel et logiciel ;
- scénario et unité fonctionnelle ;
- nombre d’échantillons ;
- méthode de mesure ;
- tolérance.

Le compilateur vérifie la forme du contrat ; le benchmark le valide.

### Dégradation

Une politique ordonnée peut réduire batch, résolution, fréquence ou précision.
Elle NE DOIT PAS franchir un plancher de qualité, de sécurité ou de confidentialité.

Un résultat devenu inutile DEVRAIT être annulé avant de consommer calcul et
énergie supplémentaires.

### Énergie

La télémétrie distingue mesure matérielle, estimation calibrée et absence de
donnée. Une estimation NE DOIT PAS être présentée comme mesure.

Le profiler attribue au minimum énergie CPU, moteur de calcul et transferts
lorsque la plateforme les expose.

### Carbone

Le carbone opérationnel combine énergie et intensité du lieu/temps. Le carbone
incorporé du matériel est amorti selon une méthode documentée. Les offsets NE
SONT PAS comptés comme réduction physique du logiciel.

### Gate de release

```text
verify-budget <profile>
```

échoue si une métrique obligatoire dépasse sa limite ou si sa mesure n’est pas
disponible. Un développeur peut déroger avec justification versionnée, jamais
par silence.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- COMP-001 fournit les moteurs et variantes ;
- COMP-002 définit kernels et interprétation de référence ;
- COMP-003 choisit placement, fallback et profil déterministe ;
- DATA-002 contraint layouts et conversions ;
- DX-001 et RUN-005 utilisent `Accepts_C` entre niveaux de compilation ;
- DX-004 définit mesures, preuves et tests différentiels.

## Compatibilité et migration

La version 0.2.0 introduit `Accepts_C`, remplace `fast` sans contexte par
`fast(profile)` et distingue conformité contractuelle d’égalité des valeurs.
Les kernels `fast` doivent fixer un profil versionné ; ce changement est
source-breaking.

## Tests de conformité

La suite de conformité DOIT couvrir :

- résultat unique accepté par `strict` ;
- résultat accepté et refusé par `bounded(error)` ;
- protocole `quality` avec dataset et unité fonctionnelle fixés ;
- rejet d’une revendication par entrée fondée sur une métrique globale ;
- profil `fast` versionné et comportement hors profil ;
- mesure, estimation et absence de télémétrie distinguées ;
- gate de release avec métrique manquante.

## Questions ouvertes

- Références numériques standards par opération portable.
- Composition de deux contrats `bounded` à travers un pipeline fusionné.
