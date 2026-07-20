# FFI-001 — ABI natives et SDK de plateforme

- Statut : **Draft**
- Version : **0.2.0**
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

Une FFI certifiée `realtime` fournit un contrat versionné et une évidence
d’audit couvrant tous ses chemins admissibles : absence d’allocation cachée,
de collecte, de verrou ou attente non bornés, de callback imprévisible et
d’unwinding.

Une mesure sur profil matériel PEUT compléter cette évidence pour la deadline
et le coût. Elle NE PEUT PAS, à elle seule, démontrer l’absence universelle
d’une allocation, d’un verrou ou d’un callback. Une modification de version,
feature, backend ou dépendance native invalide la certification sauf preuve que
son empreinte de conformité reste identique.

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

- TYPE-004 définit effets, capacités et `Unsafe` ;
- TYPE-005 définit ownership et borrows de frontière ;
- RUN-004 contraint les appels par domaine ;
- RUN-005 définit isolation et code non préemptible ;
- RT-001 consomme la certification `realtime` ;
- DATA-002 définit layouts, alignements et vues ABI ;
- FFI-003 spécialise ces règles pour Swift et Kotlin.

## Compatibilité et migration

La version 0.2.0 interdit de certifier une FFI temps réel par profilage seul et
lie l’évidence à une empreinte de dépendances. Les certifications existantes
uniquement mesurées deviennent insuffisantes ; ce changement est
source-breaking pour leur profil de validation.

## Tests de conformité

La suite de conformité DOIT couvrir :

- wrapper sûr et déclaration incomplète classée `unsafe` ;
- rejet d’une FFI bloquante depuis `ui`, `responsive` et `realtime` ;
- certification temps réel avec contrat et évidence d’audit ;
- rejet d’une certification fondée uniquement sur une mesure ;
- invalidation après changement de dépendance native ;
- durée de vie de callback et conversion d’erreur sans unwinding.

## Questions ouvertes

- Format portable de l’évidence d’audit pour une bibliothèque native certifiée.
