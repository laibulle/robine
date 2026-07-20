# TYPE-004 — Effets et capacités

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `types`

## Objet

Rendre visibles les interactions avec le monde et contrôler l’autorité d’un
programme sans dépendance ambiante.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Lignes d’effets

Une signature contient une ligne d’effets :

```text
load : Path -> Result<Bytes, IOError> ! {FileSystem.Read}
```

Une ligne peut être polymorphe :

```text
map : (A -> B ! E) -> Vector<A> -> Vector<B> ! (E | {Allocate})
```

L’absence d’effet signifie que le résultat dépend uniquement des arguments et
que l’évaluation n’observe ni ne modifie le monde.

Une ligne d’effets peut contenir :

- des effets observables, comme `FileSystem.Read`, `Network` ou `Random` ;
- des effets de contrôle, comme `Blocking` ou `Suspend` ;
- des effets de ressource, comme `Allocate`.

Un effet de ressource peut préserver la transparence référentielle tout en
restant nécessaire au contrôle de coût et de domaine. Le qualifier de
« pur » dans une optimisation NE DOIT PAS effacer sa présence dans la ligne
d’effets tant que ce coût n’a pas été éliminé ou déplacé.

### Effets standards

Le noyau réserve au minimum :

- `FileSystem.Read`, `FileSystem.Write` ;
- `Network`;
- `Clock`, `Random`;
- `Process`, `Thread`;
- `Blocking`;
- `Suspend`;
- `Allocate`;
- `Unsafe`;
- `Platform`;
- effets d’état bornés propres aux domaines.

Les sous-effets peuvent porter des paramètres, par exemple un chemin ou un
domaine réseau.

### Axes distincts

Un effet, une capacité et un domaine d’exécution sont trois axes distincts :

- un effet décrit une opération réalisée ;
- une capacité autorise cette opération ;
- un domaine restreint les effets et services admissibles localement.

`Blocking` et `Allocate` sont des effets. `realtime`, `responsive` et `ui` sont
des domaines de RUN-004. Un contrat public NE DOIT PAS encoder un domaine comme
un effet ni appeler un effet une capacité.

### Capacités

Un effet décrit ce qui arrive ; une capacité décrit qui autorise cette action.

```text
capability Weather {
    forecast(Location) -> Task<Forecast, WeatherError>
}
```

L’application accorde les capacités à sa racine de composition. Une
bibliothèque NE DOIT PAS obtenir une capacité par variable globale, environnement
implicite ou singleton caché.

### Handlers

Un handler fournit une interprétation d’un effet. Les handlers de test, de
plateforme et de production partagent le même contrat.

Un handler PEUT éliminer un effet lorsqu’il le traite complètement. Les effets
non traités remontent dans la signature.

### Effacement

Un handler statiquement connu PEUT être inliné. Les effets ne nécessitent pas
une allocation ou une machine dynamique lorsque leur interprétation est
résolue à la compilation.

## Diagnostics et erreurs

Un effet non autorisé DOIT afficher sa chaîne de provenance :

```text
process -> load_preset -> read_file
requires FileSystem.Read
realtime process forbids FileSystem.Read
```

## Sécurité, confidentialité et ressources

Les capacités sont non forgeables. Leur sérialisation est interdite sauf type
de délégation explicitement conçu. Une capacité restreinte NE PEUT PAS être
élargie par une bibliothèque.

## Interactions

- `realtime` limite les effets admissibles selon RT-001 ;
- RUN-001 définit l’exposition et l’élimination de `Allocate` ;
- RUN-004 définit les domaines séparément des effets ;
- les packages déclarent les capacités maximales selon PKG-002 ;
- ARCH-002 peut interdire des effets par domaine ;
- les FFI non analysables portent `Unsafe` et, si nécessaire, `Blocking`.

## Compatibilité et migration

La version 0.2.0 distingue explicitement effets observables, effets de contrôle,
effets de ressource, capacités et domaines. Elle ajoute `Allocate` aux
signatures qui construisent un stockage et aligne les capacités asynchrones sur
`Task<T,E>`. Une interface qui encodait un domaine comme effet, imbriquait
inutilement `Result` dans une tâche ou omettait une allocation non éliminée
doit être migrée ; ce changement est source-breaking.

## Tests de conformité

La suite de conformité DOIT couvrir :

- propagation et traitement d’un effet observable ;
- conservation de `Allocate` par une transformation référentiellement pure ;
- élimination prouvée de `Allocate` ;
- rejet d’un domaine encodé comme effet ;
- rejet d’un effet présenté comme capacité ;
- capacité asynchrone utilisant `Task<T,E>` sans doubler `Result` ;
- non-élargissement et non-sérialisation d’une capacité.

## Questions ouvertes

- Syntaxe finale de l’union de lignes d’effets après décision de LANG-002.
