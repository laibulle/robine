# RUN-002 — Tâches et concurrence structurée

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `runtime`

## Objet

Représenter les travaux finis et asynchrones sans tâche orpheline ni
propagation implicite des échecs.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Types

```text
Task<T, E>
TaskOutcome<T, E> =
    Succeeded(T)
  | Failed(E)
  | Cancelled(CancellationReason)
  | RuntimeFault(RuntimeFault)
Scope
Cancellation
Deadline
```

Une `Task` atteint exactement une issue terminale `TaskOutcome<T,E>`. `E`
représente les erreurs déclarées par l’opération ; annulation et faute du
runtime restent des variantes distinctes et NE DOIVENT PAS être injectées
silencieusement dans `E`.

Une opération d’attente retourne cette issue ou la propage par une construction
de contrôle dont la sémantique est équivalente. L’API publique DOIT permettre
de distinguer les quatre cas.

`Promise` writable n’est pas une abstraction publique générale. Seul le
producteur autorisé peut compléter la tâche.

### Scopes

Toute création de tâche appartient à un scope :

```text
scope {
    a = spawn work_a()
    b = spawn work_b()
    await (a, b)
}
```

Quitter un scope DOIT garantir que toutes ses tâches sont terminées ou
annulées et jointes. Une API détachée exige une capacité de service durable et
un propriétaire explicite.

Une tâche est jointe lorsque son issue terminale est connue et qu’elle ne peut
plus reprendre, compléter un callback ni accéder aux borrows de son scope.
Cette garantie ne signifie pas qu’un matériel ou appel étranger non
interruptible a physiquement cessé.

Avant qu’une telle tâche devienne terminale, l’ownership de tout travail
résiduel et de ses buffers DOIT être transféré à un exécuteur durable, borné et
observable selon RUN-005. Le travail résiduel ne peut plus publier son résultat
dans le scope terminé.

### Annulation

L’annulation est coopérative et transitive vers les enfants, sauf branche
marquée `shielded`. Les ressources acquises sont libérées ou transférées
explicitement à un propriétaire de travail résiduel avant que la tâche soit
considérée terminée.

Une région `realtime` ne peut ni suspendre ni attendre une tâche.

### Deadlines

Une deadline est propagée avec le contexte de tâche. Une opération qui sait
qu’elle ne peut plus la respecter DEVRAIT échouer avant d’effectuer un travail
inutile.

Une deadline ne constitue une garantie que si RUN-004 a accepté les ressources
du domaine.

### Placement

Une tâche décrit concurrence et durée de vie, pas processeur. Le scheduler peut
la placer sur CPU, moteur de calcul ou worker isolé selon ses effets.

## Diagnostics et erreurs

Les politiques standards sont :

- `fail_fast` : première erreur annule les frères ;
- `collect` : attend tous les résultats ;
- `supervise` : délègue la politique à un superviseur ;
- `race` : conserve le premier résultat admissible et annule le reste.

La politique DOIT être déductible du code, jamais d’un réglage global.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- RUN-004
- TYPE-004 définit `Suspend`, `Blocking` et les effets de placement ;
- TYPE-005 interdit aux borrows de traverser une fin de scope ;
- RUN-005 possède le travail résiduel et les exécuteurs non interruptibles ;
- FFI-001 contraint les appels bloquants.

## Compatibilité et migration

La version 0.2.0 introduit `TaskOutcome`, sépare annulation et faute de `E`, et
définit la jointure en présence de travail résiduel. Une API qui encodait
l’annulation dans son erreur métier ou déclarait terminée une tâche encore
capable de rappeler son scope doit migrer ; ce changement est source-breaking.

## Tests de conformité

Tests obligatoires :

- les quatre issues terminales de `TaskOutcome` ;
- annulation et faute runtime distinctes de `E` ;
- aucune tâche vivante après sortie de scope ;
- annulation hiérarchique ;
- libération ou transfert explicite des ressources ;
- travail non interruptible transféré sans callback tardif vers le scope ;
- déterminisme de chaque politique d’erreur ;
- absence d’attente dans `realtime`.

## Questions ouvertes

- Surface standard permettant d’inspecter le travail résiduel sans en
  retransférer l’ownership au scope terminé.
