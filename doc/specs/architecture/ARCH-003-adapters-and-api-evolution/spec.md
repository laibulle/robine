# ARCH-003 — Adaptateurs, composition et évolution d’API

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `architecture`

## Objet

Placer les abstractions aux frontières de changement réelles et gérer leur
évolution sans conteneur d’injection ou compatibilité implicite.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Frontière justifiée

Une capacité, un protocole ou un adaptateur est recommandé lorsqu’au moins une
condition existe :

- système ou processus externe ;
- effet ou privilège différent ;
- ownership ou domaine d’exécution différent ;
- plusieurs implémentations réellement utilisées ;
- version ou rythme de changement indépendant ;
- frontière native, réseau ou stockage.

Une fonction interne pure n’exige pas une interface dédiée pour être testable.

### Composition

La racine de l’application associe capacités et implémentations :

```text
compose Production {
    PresetStore = SQLitePresets(database)
    AudioDevice = PlatformAudio.default
}
```

Les dépendances manquantes ou ambiguës sont des erreurs statiques. En test, une
composition alternative injecte fake ou simulateur avec le même contrat.

Quand une seule implémentation est retenue dans une release, le compilateur
PEUT dévirtualiser et inliner les appels.

### Versions

Types de données, protocoles de messages, APIs et schémas portent des versions
uniquement lorsque la coexistence est nécessaire. La version ne remplace pas
une compatibilité vérifiée.

### Changements

Le diff d’API considère :

- ensemble de valeurs accepté et retourné ;
- effets ajoutés ou retirés ;
- capacités plus larges ;
- ownership ;
- erreurs ;
- deadlines et budgets ;
- représentation ABI ;
- format sérialisé.

### Adaptateurs de compatibilité

Un adaptateur entre versions est du code normal, testable et observable. Il
NE DOIT PAS inventer une valeur absente ou supprimer une erreur sans politique
explicite.

### Hot reload

DX-003 utilise les mêmes règles de compatibilité. Une API compatible au niveau
source mais incompatible en état vivant exige une migration.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- DX-003

## Compatibilité et migration

Une API dépréciée indique remplacement, date ou version de retrait et migration
automatique éventuelle. Le compilateur peut interdire une nouvelle utilisation
tout en autorisant l’ancien code pendant la fenêtre prévue.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
