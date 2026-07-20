# RT-001 — Domaine audio temps réel

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `realtime`

## Objet

Permettre un moteur DSP natif dont l’usage mémoire et les opérations bloquantes
sont vérifiés statiquement, sans imposer un borrow checker global.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Contrat `realtime`

Une fonction temps réel déclare une deadline, une fréquence ou un budget de
bloc et les ressources préallouées qu’elle utilise.

```text
realtime process<N>(
    in AudioBlock<N>,
    inout AmpState,
    out AudioBlock<N>
) deadline 1ms
```

Son graphe d’appels complet DOIT être admis selon RUN-004 et :

- NE DOIT PAS porter `Allocate` selon RUN-001 ;
- NE DOIT PAS porter `Blocking`, `Suspend`, I/O ou création de tâche selon
  TYPE-004 ;
- NE DOIT PAS déclencher collecte, croissance de collection, verrou, attente
  conditionnelle, panic ou unwinding ;
- NE DOIT appeler qu’une FFI certifiée par FFI-001 ;
- DOIT posséder des bornes statiques pour boucles, récursion, tailles, cibles
  indirectes et primitives de synchronisation.

Une opération bornée sur un stockage entièrement préalloué suit RUN-001 et peut
être admise comme effet d’état borné. Elle NE DOIT PAS agrandir, remplacer,
récupérer ou libérer dynamiquement ce stockage dans le callback.

### Mémoire

Les buffers, lignes de délai, convolueurs et tables sont préparés dans une
phase non temps réel. Les paramètres `inout` et `out` donnent un accès exclusif
limité à l’appel.

Les tailles dynamiques doivent posséder une borne vérifiée à la préparation.

### Temps d’exécution

Le compilateur analyse :

- boucles et bornes ;
- appels indirects ;
- tailles de buffers ;
- branches dépendantes de données ;
- instructions et kernels matériels admissibles.

Une preuve cycle par cycle de WCET matériel n’est pas obligatoire, mais la
preuve statique d’un comportement borné est obligatoire pour le domaine
`realtime`.

Une entrée temps réel publie séparément :

- `verified-bounds` : absence d’opération non bornée et borne abstraite de
  travail et de mémoire ;
- `measured-deadline(profile)` : `verified-bounds` complété par une campagne de
  mesure définie sur un profil matériel et logiciel ;
- `verified-deadline(profile)` : borne de deadline démontrée par une méthode
  statique ou formelle documentée pour ce profil.

Une release audio conforme DOIT posséder `verified-bounds` et une preuve de
deadline acceptée par son profil, mesurée ou vérifiée. Un artefact
`unverified` PEUT être compilé comme candidat de développement, mais NE DOIT
PAS être installé comme point d’entrée `realtime` ni présenté comme garantie
temps réel.

### Paramètres

Les changements depuis UI ou réseau passent par une structure bornée. Le DSP
peut lisser un paramètre sans allocation :

```text
AudioParam<Gain, smoothing: 5ms>
```

### Plateforme

Le backend audio fournit priorité de thread, taille de bloc, horloge et
callbacks natifs. Les différences de plateforme sont visibles dans un profil
de validation, pas dans l’algorithme DSP pur.

## Diagnostics et erreurs

Une fonction temps réel ne lève pas d’exception. Les erreurs récupérables
utilisent une variante bornée ou un drapeau de télémétrie. Une violation
impossible à traiter applique une politique préparée : silence, bypass,
dernière sortie valide ou arrêt du flux.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- TYPE-004 définit les effets interdits et les effets d’état bornés ;
- TYPE-005 définit `inout`, `out`, ownership et raffinements ;
- RUN-001 possède la règle d’allocation et de stockage préalloué ;
- RUN-004 possède le domaine `realtime` et son admission ;
- RUN-005 exclut préemption et services non bornés du chemin temps réel ;
- RT-002 définit les communications, observations et échanges de graphe ;
- DATA-002 définit les vues et layouts préparés ;
- FFI-001 possède la certification des appels étrangers.

## Compatibilité et migration

La version 0.2.0 remplace l’interdiction ambiguë des seules allocations « non
bornées » par l’interdiction de l’effet `Allocate`, et sépare preuve de bornes,
mesure de deadline et preuve de deadline. `verified-memory` devient
`verified-bounds`. Un artefact seulement `unverified` n’est plus admissible
comme callback temps réel ; ce changement est source-breaking pour son profil
de déploiement.

## Tests de conformité

La suite temps réel DOIT détecter allocation, verrou, appel bloquant, buffer
débordant, dépassement mesuré et FFI non certifiée. Elle couvre également :

- réservation bornée dans un stockage préalloué admise ;
- croissance du même stockage rejetée ;
- installation d’un artefact `unverified` rejetée ;
- `verified-bounds` sans revendication de deadline ;
- mesure et preuve de deadline distinguées.

## Questions ouvertes

- Niveau minimal de couverture statistique accepté pour
  `measured-deadline(profile)`.
- Profils exigeant obligatoirement `verified-deadline(profile)`.
