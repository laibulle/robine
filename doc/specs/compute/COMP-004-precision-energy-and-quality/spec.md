# COMP-004 — Précision, énergie, thermique et qualité

- Statut : **Draft**
- Version : **0.1.0**
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
- `fast` : transformations agressives, limites documentées.

Le passage à une précision inférieure n’est possible que si le contrat reste
valide.

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

Aucune interaction normative supplémentaire n’est déclarée.

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
