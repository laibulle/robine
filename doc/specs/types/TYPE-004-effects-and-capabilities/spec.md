# TYPE-004 — Effets et capacités

- Statut : **Draft**
- Version : **0.1.0**
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
map : (A -> B ! E) -> Vector<A> -> Vector<B> ! E
```

L’absence d’effet signifie que le résultat dépend uniquement des arguments et
que l’évaluation n’observe ni ne modifie le monde.

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

### Capacités

Un effet décrit ce qui arrive ; une capacité décrit qui autorise cette action.

```text
capability Weather {
    forecast(Location) -> Task<Result<Forecast, WeatherError>>
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
- les packages déclarent les capacités maximales selon PKG-002 ;
- ARCH-002 peut interdire des effets par domaine ;
- les FFI non analysables portent `Unsafe` et, si nécessaire, `Blocking`.

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
