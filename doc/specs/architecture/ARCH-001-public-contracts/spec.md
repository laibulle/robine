# ARCH-001 — Contrats publics et artefacts d’interface

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `architecture`

## Objet

Obtenir compilation séparée, stabilité d’API et documentation sans déclaration
dupliquée dans un fichier header.

## Source unique

Une fonction publique déclare sa signature une seule fois, avec son
implémentation ou comme contrat à implémenter :

```text
public process(
    in AudioBlock<N>,
    inout AmpState,
    out AudioBlock<N>
) -> Unit ! Realtime
```

La déclaration couvre :

- types et polymorphisme ;
- effets et capacités ;
- ownership et multiplicité ;
- contrats ;
- disponibilité et dépréciation ;
- ABI lorsqu’elle est exportée nativement.

## Artefact d’interface

Le compilateur produit un artefact machine contenant :

- symboles exportés ;
- types normalisés ;
- contraintes et effets ;
- identités de champs et variantes ;
- layouts ABI requis ;
- documentation structurée ;
- empreinte source et sémantique.

Cet artefact n’est pas édité par l’humain. Il remplace les headers et permet à
DX-001 de typer un consommateur sans charger l’implémentation.

## Stabilité

Une modification de corps qui conserve l’interface ne change pas l’empreinte
sémantique publique. Une modification d’effet, ownership, contrat ou ABI est
une modification d’interface, même si les types de paramètres semblent
identiques.

## Interfaces abstraites

Un protocole ou une capacité peut être déclaré sans implémentation. Ce cas
n’est pas une duplication : le contrat possède plusieurs fournisseurs ou
traverse une frontière externe.

## Documentation

La documentation publique est générée depuis l’artefact et les exemples
validés. Elle indique effets, erreurs, garanties, disponibilité et coûts
pertinents.

## Compatibilité

Le toolchain compare deux artefacts et classe :

- ajout compatible ;
- rupture source ;
- rupture d’effet ;
- rupture de contrat ;
- rupture ABI ;
- changement de comportement documenté.

## Fichiers

Un projet PEUT séparer physiquement contrats et implémentations pour
organisation, mais le compilateur garantit une définition canonique unique de
chaque symbole. L’inclusion textuelle et les déclarations divergentes sont
impossibles.
