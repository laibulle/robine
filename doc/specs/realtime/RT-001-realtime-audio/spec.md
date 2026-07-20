# RT-001 — Domaine audio temps réel

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `realtime`

## Objet

Permettre un moteur DSP natif dont l’usage mémoire et les opérations bloquantes
sont vérifiés statiquement, sans imposer un borrow checker global.

## Contrat `realtime`

Une fonction temps réel déclare une deadline, une fréquence ou un budget de
bloc et les ressources préallouées qu’elle utilise.

```text
realtime process<N>(
    in AudioBlock<N>,
    inout AmpState,
    out AudioBlock<N>
) deadline 1ms
```

Son graphe d’appels complet NE DOIT PAS contenir :

- allocation ou libération non bornée ;
- collecte mémoire ;
- verrou ou attente conditionnelle ;
- entrée-sortie ;
- suspension, `await` ou création de tâche ;
- FFI non certifiée ;
- croissance de collection ;
- panic ou unwinding.

## Mémoire

Les buffers, lignes de délai, convolueurs et tables sont préparés dans une
phase non temps réel. Les paramètres `inout` et `out` donnent un accès exclusif
limité à l’appel.

Les tailles dynamiques doivent posséder une borne vérifiée à la préparation.

## Erreurs

Une fonction temps réel ne lève pas d’exception. Les erreurs récupérables
utilisent une variante bornée ou un drapeau de télémétrie. Une violation
impossible à traiter applique une politique préparée : silence, bypass,
dernière sortie valide ou arrêt du flux.

## Temps d’exécution

Le compilateur analyse :

- boucles et bornes ;
- appels indirects ;
- tailles de buffers ;
- branches dépendantes de données ;
- instructions et kernels matériels admissibles.

Une preuve statique de WCET complet n’est pas obligatoire pour la conformité
initiale. Une release DOIT toutefois distinguer :

- `verified-memory` ;
- `measured-deadline(profile)` ;
- `unverified`.

`unverified` ne peut être présenté comme garantie temps réel.

## Paramètres

Les changements depuis UI ou réseau passent par une structure bornée. Le DSP
peut lisser un paramètre sans allocation :

```text
AudioParam<Gain, smoothing: 5ms>
```

## Plateforme

Le backend audio fournit priorité de thread, taille de bloc, horloge et
callbacks natifs. Les différences de plateforme sont visibles dans un profil
de validation, pas dans l’algorithme DSP pur.

## Conformité

La suite temps réel DOIT détecter allocation, verrou, appel bloquant, buffer
débordant, dépassement mesuré et FFI non certifiée.
