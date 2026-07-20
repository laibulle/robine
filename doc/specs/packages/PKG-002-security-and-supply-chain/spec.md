# PKG-002 — Sécurité des paquets et chaîne logicielle

- Statut : **Draft**
- Version : **0.2.0**
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

Chaque publication, composée du payload canonique et de son enveloppe, contient
ou référence :

- source ;
- identité du builder ;
- toolchain ;
- dépendances ;
- attestations ;
- licences ;
- signature du registre ou éditeur.

Le payload canonique de PKG-001 contient uniquement les éléments de provenance
qui font partie de ses entrées reproductibles ou des références stables par
contenu. Identité du builder, attestations variables, signature et horodatage
sont placés dans l’enveloppe de distribution par défaut.

Chaque attestation et signature DOIT lier cryptographiquement le hachage du
payload canonique. Si une politique incorpore ces données dans le payload, elle
DOIT les déclarer comme entrées du profil et NE DOIT PAS revendiquer une
reproductibilité bit-à-bit entre builders ou signatures différents.

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

- PKG-001 définit payload canonique, enveloppe et lockfile ;
- LANG-002 et LANG-004 séparent macros pures et tâches de build ;
- DX-001 adresse les artefacts et caches par contenu ;
- COMP-003 signe les spécialisations dynamiques ;
- UI-003 applique ces règles aux packages JavaScript ;
- FFI-002 verrouille environnements Python et modèles ;
- STD-001 contraint plugins et extensions.

## Compatibilité et migration

La version 0.2.0 place signatures et attestations variables dans une enveloppe
liée au payload. Les registres doivent conserver et vérifier les deux
empreintes ; ce changement est ABI-breaking pour le format de publication.

## Tests de conformité

La suite de conformité DOIT couvrir :

- package sans autorité ambiante ;
- augmentation de capacité refusée sans consentement ;
- générateur hermétique et outputs hachés ;
- attestation et signature liées au payload canonique ;
- deux signatures différentes sans modification du payload ;
- cache malveillant rejeté après vérification ;
- rapport SBOM, unsafe, FFI et exceptions de politique.

## Questions ouvertes

- Format d’attestation canonique et politique d’horodatage reproductible.
