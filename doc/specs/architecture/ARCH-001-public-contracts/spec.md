# ARCH-001 — Contrats publics et artefacts d’interface

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `architecture`

## Objet

Obtenir compilation séparée, stabilité d’API et documentation sans déclaration
dupliquée dans un fichier header.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Source unique

Une fonction publique déclare sa signature une seule fois, avec son
implémentation ou comme contrat à implémenter :

```text
public process(
    in AudioBlock<N>,
    inout AmpState,
    out AudioBlock<N>
) -> Unit ! {}
domain realtime
```

La déclaration couvre :

- types et polymorphisme ;
- effets et capacités ;
- domaine d’exécution ou polymorphisme de domaine ;
- ownership et multiplicité ;
- contrats ;
- disponibilité et dépréciation ;
- ABI lorsqu’elle est exportée nativement.

### Artefact d’interface

Le compilateur produit un artefact machine contenant :

- symboles exportés ;
- types normalisés ;
- contraintes et effets ;
- identités de champs et variantes ;
- layouts ABI requis ;
- documentation structurée ;
- empreinte du contrat public ;
- empreinte distincte de l’implémentation ou de son contenu.

Cet artefact n’est pas édité par l’humain. Il remplace les headers et permet à
DX-001 de typer un consommateur sans charger l’implémentation.

### Stabilité

Une modification de corps qui conserve le contrat ne change pas l’empreinte du
contrat public, mais change l’empreinte d’implémentation. Une modification de
comportement documenté qui ajoute ou retire une garantie publique modifie le
contrat.

Une modification d’effet, domaine, ownership, contrat ou ABI est une
modification d’interface, même si les types de paramètres semblent identiques.

### Interfaces abstraites

Un protocole ou une capacité peut être déclaré sans implémentation. Ce cas
n’est pas une duplication : le contrat possède plusieurs fournisseurs ou
traverse une frontière externe.

### Documentation

La documentation publique est générée depuis l’artefact et les exemples
validés. Elle indique effets, erreurs, garanties, disponibilité et coûts
pertinents.

### Fichiers

Un projet PEUT séparer physiquement contrats et implémentations pour
organisation, mais le compilateur garantit une définition canonique unique de
chaque symbole. L’inclusion textuelle et les déclarations divergentes sont
impossibles.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- DX-001
- TYPE-004 définit effets et capacités ;
- RUN-004 définit les domaines d’exécution ;
- DATA-002 définit les layouts ABI exposés.

## Compatibilité et migration

La version 0.2.0 sépare empreinte contractuelle et empreinte d’implémentation,
et rend le domaine d’exécution explicite dans l’interface. Les anciens
artefacts qui encodaient `Realtime` comme effet doivent migrer leur format ;
ce changement est ABI-breaking.

Le toolchain compare deux artefacts et classe :

- ajout compatible ;
- rupture source ;
- rupture d’effet ;
- rupture de domaine ;
- rupture de contrat ;
- rupture ABI ;
- changement de comportement documenté.

## Tests de conformité

La suite de conformité DOIT couvrir :

- contrat public contenant effets, capacités, domaine et ownership ;
- empreinte contractuelle stable après modification de corps compatible ;
- empreinte d’implémentation modifiée par ce même changement ;
- modification d’une garantie documentée changeant le contrat ;
- classification séparée des ruptures d’effet, domaine, contrat et ABI.

## Questions ouvertes

- Format stable de l’empreinte contractuelle à travers versions de solveur.
