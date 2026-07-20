# COMP-003 — Placement, spécialisation et fallback

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `compute`

## Objet

Choisir un moteur de calcul en tenant compte du coût complet, plutôt que du
seul débit maximal annoncé.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Variantes

Un kernel peut produire :

```text
cpu.scalar
cpu.vector
cpu.matrix
npu.<precision/layout>
```

Les variantes compatibles sont regroupées dans un artefact ou cache de
spécialisation identifié par l’IR, le profil matériel et le contrat numérique.

### Modèle de coût

Le placement estime :

```text
compilation/amortissement
réveil du moteur
transferts et conversions
synchronisation
calcul
pression mémoire
énergie et contrainte thermique
latence de queue
```

Un petit graphe DEVRAIT rester sur le CPU lorsqu’un offload ne rembourse pas
ses coûts fixes.

### Politiques

Le programme peut demander :

- `adaptive` : choix runtime ;
- `prefer npu fallback cpu`;
- `require capability`;
- `minimize latency`;
- `minimize energy within deadline`;
- `deterministic profile`;
- `offline compile only`.

Une préférence n’est pas une obligation. `require` produit une erreur
d’admission lorsque la capacité manque.

### Spécialisation

Les dimensions, layouts et précisions fréquents peuvent être spécialisés.
Une spécialisation dynamique se compile hors chemin critique et utilise la
variante générique en attendant.

Les caches sont bornés, signés selon PKG-002 et invalidés par version de
backend.

### Fallback

Le fallback DOIT :

- produire un résultat satisfaisant `Accepts_C` pour le contrat COMP-004 ;
- signaler une deadline qui ne sera plus tenue ;
- éviter une copie supplémentaire lorsque possible ;
- être observable sans modifier le résultat.

Une bascule après exécution partielle n’est autorisée que si l’opération est
rejouable ou possède un checkpoint défini.

### Reproductibilité

Un profil déterministe fixe variante, précision, algorithme et paramètres du
backend. `adaptive` peut produire des performances et des valeurs différentes,
mais chaque résultat DOIT satisfaire `Accepts_C` pour le même contrat
numérique. Un consommateur exigeant l’égalité utilise un profil déterministe et
un contrat compatible avec cette exigence.

### Profilage

L’outil affiche pour chaque dispatch :

- variante choisie et alternatives ;
- raison du choix ;
- temps de queue, transfert et calcul ;
- octets déplacés ;
- énergie mesurée ou estimée ;
- fallback éventuel.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- COMP-001 définit les capacités matérielles ;
- COMP-002 définit kernels et interprétation de référence ;
- COMP-004 définit `Accepts_C` et les profils numériques ;
- PKG-002

## Compatibilité et migration

La version 0.2.0 définit fallback et adaptation par `Accepts_C`. Un fallback
qui conservait seulement une métrique globale sans satisfaire le contrat
déclaré doit être refusé ou changer de contrat ; ce changement est
semantic-breaking.

## Tests de conformité

La suite de conformité DOIT couvrir :

- plusieurs variantes satisfaisant le même `Accepts_C` ;
- résultats exacts sous profil déterministe strict ;
- adaptation avec valeurs différentes mais conformes ;
- rejet d’un fallback hors contrat ;
- annulation, checkpoint et saturation des caches.

## Questions ouvertes

- Politique standard de stabilité du placement adaptatif entre deux exécutions.
