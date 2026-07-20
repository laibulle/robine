# PKG-002 — Sécurité des paquets et chaîne logicielle

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `packages`

## Objet

Empêcher qu’une bonne expérience de dépendances reproduise scripts
d’installation arbitraires, autorité ambiante et bundles opaques.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Autorité nulle par défaut

Installer, résoudre ou compiler un package ne lui accorde ni réseau, ni
filesystem arbitraire, ni processus, ni secrets.

Toute étape nécessitant une capacité déclare :

- capacité exacte ;
- phase ;
- justification ;
- entrées et sorties ;
- reproductibilité attendue.

Une augmentation de capacité lors d’une mise à jour exige consentement
explicite.

### Scripts

Il n’existe pas de `postinstall` implicite. Un générateur autorisé s’exécute
dans une sandbox hermétique, avec outputs capturés et hachés.

Un binaire précompilé doit être relié à sa source, sa cible et sa provenance.

### Budgets

Le manifeste peut limiter :

- nombre de dépendances transitives ;
- taille d’artefact ou bundle ;
- duplication de versions ;
- temps de compilation ;
- surface de capacités ;
- code natif ou unsafe ;
- maintenance minimale exigée par politique d’organisation.

### Provenance

Chaque artefact publié contient ou référence :

- source ;
- identité du builder ;
- toolchain ;
- dépendances ;
- attestations ;
- licences ;
- signature du registre ou éditeur.

Le client vérifie hachage avant usage. Une signature ne remplace pas l’analyse
des capacités.

### Vulnérabilités

L’audit distingue :

- dépendance présente ;
- code effectivement accessible ;
- capacité requise pour exploiter ;
- artefact final affecté ;
- correctif disponible.

Le graphe de compilation permet de prioriser sans déclarer automatiquement
inoffensif du code non atteint dans une configuration différente.

### Plugins

Un plugin d’outil s’exécute avec capacités séparées de celles du programme. Il
NE PEUT PAS accéder aux secrets du build par défaut.

### Caches

Les artefacts locaux et distants sont adressés par contenu. Ils sont considérés
non fiables jusqu’à validation de leur identité, format et provenance.

### Rapport de release

La release publie SBOM, capacités, tailles, code unsafe/FFI, exceptions de
politique et reproductibilité.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

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
