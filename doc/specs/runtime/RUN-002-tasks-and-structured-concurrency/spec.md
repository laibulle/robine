# RUN-002 — Tâches et concurrence structurée

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `runtime`

## Objet

Représenter les travaux finis et asynchrones sans tâche orpheline ni
propagation implicite des échecs.

## Types

```text
Task<T, E>
Scope
Cancellation
Deadline
```

Une `Task` produit exactement un résultat `T` ou une erreur `E`, sauf
annulation ou faute du runtime explicitement représentée.

`Promise` writable n’est pas une abstraction publique générale. Seul le
producteur autorisé peut compléter la tâche.

## Scopes

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

## Annulation

L’annulation est coopérative et transitive vers les enfants, sauf branche
marquée `shielded`. Les ressources acquises sont libérées avant que la tâche
soit considérée terminée.

Une région `realtime` ne peut ni suspendre ni attendre une tâche.

## Erreurs

Les politiques standards sont :

- `fail_fast` : première erreur annule les frères ;
- `collect` : attend tous les résultats ;
- `supervise` : délègue la politique à un superviseur ;
- `race` : conserve le premier résultat admissible et annule le reste.

La politique DOIT être déductible du code, jamais d’un réglage global.

## Deadlines

Une deadline est propagée avec le contexte de tâche. Une opération qui sait
qu’elle ne peut plus la respecter DEVRAIT échouer avant d’effectuer un travail
inutile.

Une deadline ne constitue une garantie que si RUN-004 a accepté les ressources
du domaine.

## Placement

Une tâche décrit concurrence et durée de vie, pas processeur. Le scheduler peut
la placer sur CPU, moteur de calcul ou worker isolé selon ses effets.

## Conformité

Tests obligatoires :

- aucune tâche vivante après sortie de scope ;
- annulation hiérarchique ;
- libération des ressources ;
- déterminisme de chaque politique d’erreur ;
- absence d’attente dans `realtime`.
