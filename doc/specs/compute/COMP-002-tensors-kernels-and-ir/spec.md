# COMP-002 — Tenseurs, kernels et IR de calcul

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `compute`

## Objet

Définir un graphe de calcul portable, typé par forme et précision, abaissable
vers CPU vectoriel, CPU matriciel et NPU.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Tenseurs

```text
Tensor<Shape, Element, Layout>
```

La forme peut contenir dimensions constantes ou paramètres bornés. Le layout
est logique dans l’API ; le compilateur peut utiliser un layout interne
différent si les frontières retrouvent la représentation contractuelle.

Les vues partagent un buffer avec offsets et strides vérifiés. Une vue mutable
exige unicité ou preuve de non-aliasing.

### Kernel

Un kernel est :

- pur du point de vue applicatif ;
- sans I/O ;
- borné en mémoire pour une forme donnée ;
- déterministe lorsque son contrat numérique ou profil l’exige ;
- sinon soumis uniquement aux variations déclarées par ce contrat, chaque
  résultat satisfaisant `Accepts_C` selon COMP-004 ;
- composé d’opérations portables ou d’intrinsics profilés.

Il ne contient ni acteur, ni allocation générale, ni exception, ni fonction
indirecte non résolue.

### Opérations

Le noyau inclut :

- élémentaires et broadcast ;
- réductions ;
- contractions, matmul et convolutions ;
- gather/scatter bornés ;
- reshape, transpose, slice et concat ;
- contrôle tensoriel structuré ;
- quantize/dequantize ;
- primitives de sparsité documentées.

Les opérations de haut niveau sont préférées aux micro-instructions afin de
permettre fusion, tiling et placement.

### Pipeline IR

```text
AST typé
→ graphe tensoriel sémantique
→ IR structurée (boucles, contractions, réductions)
→ tiling, fusion, bufferisation
→ variantes de cible
→ code machine ou commande backend
```

L’IR sémantique DOIT être sérialisable, versionnée et indépendante du backend.
Elle DEVRAIT pouvoir importer/exporter des graphes standards lorsque leur
sémantique est compatible.

### Autodiff

La différentiation avant ou arrière est une transformation explicite de l’IR.
Chaque opération différentiable déclare sa règle, ses besoins de sauvegarde et
sa stabilité numérique.

Le compilateur PEUT recomputer plutôt que stocker un intermédiaire selon un
budget mémoire/énergie.

### Fusion

La fusion NE DOIT PAS modifier les résultats au-delà du contrat numérique. Le
profiler doit montrer les intermédiaires éliminés et les copies restantes.

### Fallback

Une opération portable possède une interprétation de référence, utile aux
tests différentiels. Une extension fabricant DOIT fournir une garde de
capacité et un chemin alternatif ou déclarer la cible obligatoire.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- TYPE-005 définit formes, unités et ownership des buffers ;
- DATA-002 distingue layout logique et représentation physique ;
- COMP-001 définit le profil matériel et le noyau portable ;
- COMP-003 définit placement, spécialisation et fallback ;
- COMP-004 définit `Accepts_C` et la précision ;
- CPL-001 abaisse l’IR vers les backends ;
- FFI-002 importe les graphes de modèles.

## Compatibilité et migration

La version 0.2.0 remplace le déterminisme implicite de tout kernel par le
déterminisme exigé par son contrat ou profil, et interdit toute variation non
déclarée. Un kernel variable sans contrat COMP-004 devient non conforme ; ce
changement est semantic-breaking.

## Tests de conformité

La suite de conformité DOIT couvrir :

- kernel déterministe sous contrat `strict` ;
- variations déclarées satisfaisant `Accepts_C` ;
- rejet d’une source de non-déterminisme absente du contrat ;
- vues aliasées, formes bornées et absence d’I/O ;
- interprétation CPU des opérations portables.

## Questions ouvertes

- Sous-ensemble initial des opérations et du contrôle tensoriel structuré.
- Format d’échange stable de l’IR sémantique.
