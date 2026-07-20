# FFI-001 — ABI natives et SDK de plateforme

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `interop`

## Objet

Interopérer avec C, Rust et SDK natifs sans rendre leurs règles de mémoire,
threading et erreurs implicites.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Déclaration

Une fonction étrangère déclare :

- convention d’appel et symbole ;
- types ABI exacts ;
- ownership de chaque pointeur/handle ;
- nullabilité ;
- durée des borrows ;
- threading requis ;
- effets, blocage et callbacks ;
- version ou disponibilité de plateforme ;
- stratégie d’erreur.

Une déclaration incomplète est `unsafe`.

### Wrappers

Les générateurs de bindings produisent :

1. couche ABI minimale ;
2. wrapper sûr qui valide tailles, nullabilité et ownership ;
3. types Robine idiomatiques ;
4. rapport des éléments restés unsafe.

Le code généré est déterministe et inclus dans la provenance du build.

### C

Les headers peuvent être consommés comme entrée du générateur, mais ne
deviennent pas le modèle de contrat interne de Robine. Les macros non
traduisibles exigent une constante générée ou un shim explicite.

### Rust

L’interop utilise une ABI stable explicitement exportée, pas l’ABI Rust
interne. Les panics NE DOIVENT PAS traverser la frontière.

### Blocage

Une FFI déclarée `blocking` est interdite depuis `ui`, `responsive` non isolé
et `realtime`. Elle s’exécute dans un worker `isolated` ou via une API
asynchrone.

Une FFI certifiée `realtime` fournit preuve ou profil de conformité : aucune
allocation cachée, verrou non borné ou callback imprévisible.

### Callbacks

Un callback possède durée de vie, thread et politique de réentrance. Une
closure ne peut être libérée tant que le fournisseur étranger peut l’appeler.

### SDK mobiles

Les métadonnées Apple/Android sont traduites en disponibilité, nullabilité et
exécuteur requis. Une API absente sur une version cible est un cas typé, pas un
échec de symbole tardif.

### Audit

Le toolchain liste toutes les frontières unsafe, leurs appelants et les domaines
d’exécution qui peuvent les atteindre.

## Diagnostics et erreurs

Codes, `errno`, exceptions plateforme et résultats natifs sont convertis à une
variante d’erreur. Aucun unwinding étranger ne traverse le runtime Robine.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

Aucune interaction normative supplémentaire n’est déclarée.

## Compatibilité et migration

Les changements de cette spec suivent la classification de META-001. Aucun mécanisme supplémentaire de migration n’est défini.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
