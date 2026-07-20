# LANG-005 — Programmation data-first et identité explicite

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `language`

## Objet

Définir le modèle de programmation de Robine sans imposer pureté fonctionnelle,
hiérarchie de classes ou mutation globale. Les données et transformations sont
le cas par défaut ; identité, mutation, dispatch dynamique et objets de
plateforme sont des propriétés locales, explicites et payées uniquement aux
frontières qui les demandent.

## Non-objectifs

- choisir la syntaxe source de LANG-002 ;
- imposer un style fonctionnel pur ;
- interdire l’impératif dans une région possédée ;
- faire de toute donnée un acteur, une ressource ou un objet ;
- masquer le modèle objet de Swift, Kotlin ou d’un SDK étranger ;
- définir les layouts physiques couverts par DATA-002 ;
- remplacer les règles de types de TYPE-003 ou d’ownership de TYPE-005.

## Spécification normative

### Principe data-first

Le modèle standard suit l’ordre de décision suivant :

```text
donnée sans identité       → record, variante, collection ou valeur opaque
transformation             → fonction ou fonction multiclause
extension ouverte          → protocole
état concurrent identifiable → acteur
ressource avec cycle de vie → ressource possédée
calcul intensif            → buffer, région transiente ou kernel
objet de plateforme        → handle étranger typé
```

Le compilateur et la documentation DEVRAIENT présenter le mécanisme le plus
simple qui satisfait le besoin sans introduire d’identité ou de dispatch
dynamique superflu.

Une valeur ordinaire NE DOIT PAS acquérir implicitement :

- identité d’adresse ;
- allocation heap ;
- table de méthodes ;
- verrou ;
- collecteur global ;
- finalizer ;
- autorité ambiante.

### Données et transformations

Records, variantes et collections décrivent des valeurs. Les fonctions
décrivent leurs transformations. Une valeur NE DOIT PAS posséder
sémantiquement le code qui sait la transformer.

Une fonction multiclause utilise les patterns de LANG-003 et les types
ensemblistes pour définir son domaine. Le regroupement syntaxique éventuel
d’une fonction près d’un type est une règle d’organisation, pas une fusion du
stockage et du comportement.

Une transformation pure PEUT être composée, mémorisée, fusionnée,
spécialisée ou évaluée partiellement. Ces optimisations NE DOIVENT PAS changer
l’ordre observable des effets défini par LANG-003.

### Choix entre variantes et protocoles

Une variante fermée est le mécanisme standard lorsque l’ensemble des cas
appartient au propriétaire du type et que l’ajout d’un cas doit invalider les
matches exhaustifs.

Un protocole est le mécanisme standard lorsque de nouveaux fournisseurs
doivent pouvoir être ajoutés sans modifier le propriétaire de l’abstraction.

Une implémentation de protocole NE DOIT PAS transformer son type en sous-classe
du protocole. Elle fournit les opérations de TYPE-003 et conserve son propre
modèle de stockage.

Un type existentiel ou un dispatch par table doit être demandé par une
frontière ouverte. Lorsque l’implémentation concrète est connue, le compilateur
DEVRAIT spécialiser ou dévirtualiser l’appel.

Une API publique DOIT indiquer si elle accepte :

- une famille fermée de valeurs ;
- tout type satisfaisant statiquement un protocole ;
- une valeur existentielle avec dispatch runtime ;
- un handle étranger dont le dispatch appartient à une plateforme.

### Encapsulation

L’encapsulation appartient aux modules, aux capacités et aux types opaques.
Un type opaque expose un contrat public sans exposer sa représentation.

Le module propriétaire PEUT :

- construire et valider la valeur ;
- accéder à sa représentation privée ;
- préserver ses invariants ;
- fournir fonctions et implémentations de protocoles.

Un consommateur NE DOIT PAS obtenir l’accès aux champs privés par réflexion,
notation receveur ou sérialisation implicite.

Changer la représentation privée d’un type opaque sans changer son interface
est compatible tant qu’aucun layout, ABI ou schéma public ne la rend
observable.

### Identité

L’identité est distincte de la valeur. Un type portant une identité DOIT
déclarer sa catégorie :

- acteur ;
- ressource possédée ;
- cellule mutable ;
- handle de plateforme ;
- identité durable applicative.

Copier une valeur ne crée pas automatiquement une nouvelle identité. Copier un
handle partage, déplace ou interdit l’accès selon sa multiplicité.

L’égalité de valeurs compare le contenu défini par le type. L’égalité
d’identités compare un identifiant explicite. Une implémentation NE DOIT PAS
utiliser l’adresse mémoire comme égalité publique sans contrat d’identité
stable.

Une valeur sérialisée NE conserve une identité que si son schéma DATA-001
contient un identifiant durable prévu à cet effet.

### Mutation locale

Les bindings et valeurs ordinaires sont immuables par défaut. La mutation est
autorisée dans :

- un `transient` ou une région unique ;
- un paramètre `inout` ;
- l’état privé d’un acteur ;
- une cellule ou primitive atomique explicite ;
- une ressource étrangère dont le contrat autorise la mutation.

Une mutation locale NE DOIT PAS devenir observable avant publication, gel,
envoi de message ou appel étranger explicite.

Lorsque l’ownership est unique, une transformation fonctionnelle PEUT être
abaissée vers mutation en place. Réciproquement, une boucle impérative locale
PEUT produire une valeur immuable à sa frontière.

Le langage NE DOIT PAS exiger la construction de collections persistantes
intermédiaires lorsqu’une fusion ou une région transiente préserve la
sémantique.

Une API critique PEUT exposer directement une boucle, un buffer ou une région
mutable lorsqu’ils expriment mieux son coût. Cette API publie ownership,
aliasing, effets et bornes au lieu de prétendre être pure.

### Héritage et composition

Le profil portable standard NE DOIT PAS fournir d’héritage de stockage ou
d’implémentation entre types Robine.

La réutilisation utilise :

- composition de valeurs ;
- fonctions ;
- protocoles ;
- délégation explicite ;
- génération contrôlée selon LANG-004.

Une relation de sous-typage ensembliste NE DOIT PAS être interprétée comme une
permission d’hériter de champs, constructeurs ou méthodes.

Un SDK étranger PEUT imposer une sous-classe, un delegate ou un proxy.
L’adaptateur généré selon FFI-003 contient cette relation ; elle ne devient pas
une hiérarchie de types métier Robine.

### Notation receveur

La syntaxe canonique PEUT fournir une notation receveur telle que :

```text
value.operation(arguments)
```

Cette notation est uniquement une forme d’appel. Sa résolution DOIT produire
exactement l’un des cas suivants :

- fonction de module visible ;
- opération de protocole ;
- extension explicitement importée ;
- méthode d’un handle étranger.

Le service de langage DOIT afficher la cible résolue et préciser si le dispatch
est statique, spécialisé, par table ou étranger.

Deux extensions également applicables sont une ambiguïté. L’ordre des imports
NE DOIT PAS choisir silencieusement l’une d’elles.

Une notation receveur NE DOIT PAS accorder l’accès aux champs privés,
introduire un `self` implicite non représenté dans le contrat ou changer
l’ownership de l’argument receveur.

### Fonctions et style fonctionnel

Robine conserve :

- fonctions comme valeurs ;
- composition et pipelines ;
- patterns et fonctions multiclause ;
- collections persistantes ;
- transformations pures ;
- inférence polymorphe.

Ces mécanismes NE DOIVENT PAS imposer :

- allocation d’une closure lorsque l’appel est statiquement connu ;
- boxing des arguments ;
- collection intermédiaire après fusion possible ;
- récursion lorsque la boucle est la représentation efficace ;
- dispatch indirect après spécialisation ;
- copie d’une valeur exclusivement possédée.

Une implémentation PEUT conserver la forme fonctionnelle pour debug et
développement puis produire une boucle impérative équivalente dans une release
scellée.

### Coût du dispatch

Le dispatch statique est le défaut. Il ne requiert aucune table runtime.

Un protocole utilisé statiquement PEUT être monomorphisé. Un appel existentiel
conserve une représentation comprenant au minimum la valeur ou son handle et
le moyen de résoudre ses opérations.

Le compilateur DOIT exposer :

- appels dévirtualisés ;
- tables conservées ;
- allocations ou boxing associés ;
- frontières empêchant la spécialisation ;
- taille de code due à la monomorphisation.

Une API NE DOIT PAS employer un type existentiel uniquement pour cacher une
implémentation lorsque l’opacité de module suffit.

### Relation au layout

Un record, une variante ou une implémentation de protocole ne fixe pas son
layout physique par sa seule déclaration.

En l’absence de contrat de layout, DATA-002 PEUT représenter la valeur :

- inline ;
- en registres ou sur stack ;
- dans une disposition AoS, SoA ou tuilée ;
- par une vue empruntée ;
- dans un espace mémoire CPU, partagé ou accélérateur.

Demander une adresse stable, une ABI publique, une représentation étrangère ou
une réflexion de layout réduit ces libertés et DOIT être explicite.

### États, acteurs et ressources

Un état métier sans concurrence PEUT rester une valeur passée entre fonctions.
Il NE DEVRAIT PAS devenir un acteur uniquement pour encapsuler des champs.

Un acteur est justifié par une identité concurrente, une mailbox, une politique
de supervision ou une frontière de scheduling selon RUN-003.

Une ressource est justifiée par une acquisition et une libération
déterministes : fichier, socket, buffer natif, session, objet de plateforme ou
handle matériel.

Un acteur ou une ressource NE DOIT PAS être sérialisé comme un record ordinaire.
Il expose un snapshot, une commande, un identifiant durable ou un adaptateur
explicite.

### Frontières étrangères

Les objets Swift, Kotlin et de SDK conservent leurs identités, cycles de vie,
dispatchs et contraintes de thread selon FFI-003.

Une classe étrangère importée devient un handle opaque, pas un record
structurel. Une conversion vers une valeur Robine exige un snapshot ou schéma
explicite.

À l’export :

- un record PEUT devenir une valeur ou data class hôte ;
- une variante PEUT devenir une enum ou hiérarchie scellée hôte ;
- un protocole PEUT devenir une interface lorsque le dispatch runtime est
  réellement requis ;
- un type opaque ou une ressource PEUT devenir une classe finale de façade.

La projection choisie NE DOIT PAS modifier la sémantique Robine pour imiter les
conventions d’une seule plateforme.

## Diagnostics et erreurs

Le compilateur DOIT diagnostiquer :

- ambiguïté entre opérations de notation receveur ;
- accès à la représentation d’un type opaque ;
- comparaison implicite par identité d’une valeur ;
- copie interdite d’une ressource ou d’un handle ;
- mutation sans accès unique ou domaine propriétaire ;
- implémentations de protocole incohérentes ;
- tentative d’héritage entre types Robine ;
- sérialisation implicite d’un acteur, d’une ressource ou d’un objet étranger ;
- dispatch dynamique introduit alors que le profil l’interdit.

Un diagnostic de performance DEVRAIT proposer le mécanisme responsable :
boxing, existential, allocation, copie, layout figé, appel étranger ou
spécialisation empêchée.

## Sécurité, confidentialité et ressources

Un type opaque NE DOIT PAS exposer par debug, réflexion ou sérialisation des
champs marqués secrets sans capacité d’inspection.

Une ressource, un acteur ou un objet étranger transporte ses capacités. La
notation receveur NE DOIT PAS fournir une autorité supérieure à celle de la
valeur reçue.

La destruction d’une ressource suit RUN-001. Aucun destructeur implicite NE
DOIT bloquer, suspendre ou effectuer une I/O non déclarée.

Le projet PEUT interdire dispatch dynamique, réflexion de layout, héritage
étranger ou handles non `Sendable` dans certains domaines.

## Interactions

- LANG-003 définit expressions, bindings, mutation et pattern matching ;
- LANG-004 limite la génération et les extensions syntaxiques ;
- TYPE-001 définit le sous-typage ensembliste ;
- TYPE-003 définit records, variantes et protocoles ;
- TYPE-004 définit effets et capacités ;
- TYPE-005 définit multiplicité, borrows et `inout` ;
- RUN-001 définit mémoire, transients et destruction ;
- RUN-003 définit acteurs, identité concurrente et supervision ;
- DATA-001 définit sérialisation et identité durable ;
- DATA-002 définit les layouts physiques ;
- CPL-001 définit spécialisation et scellement ;
- FFI-003 définit la projection Swift/Kotlin et les handles étrangers.

## Compatibilité et migration

La version 0.2.0 aligne l’extension des protocoles sur la cohérence globale de
TYPE-003. Une instance locale ou un chevauchement ouvert doit devenir un type
opaque, un adaptateur ou une spécialisation fermée ; ce changement est
source-breaking.

Transformer un record public en ressource ou acteur est source-breaking et
semantic-breaking : égalité, copie, sérialisation et cycle de vie changent.

Transformer un type concret en type opaque peut être source-breaking pour les
consommateurs de sa représentation mais permet ensuite des évolutions privées
compatibles.

Ajouter un appel dynamique à une API auparavant statique modifie son contrat de
coût. Ajouter une implémentation non chevauchante et autorisée par TYPE-003 à
un protocole ouvert est compatible. Un chevauchement ouvert est rejeté ; ouvrir
ou modifier l’arbre de spécialisation est source-breaking lorsque la résolution
publique peut changer.

Une façade objet exportée suit FFI-003. Changer la forme de façade sans changer
le contrat Robine peut néanmoins être source-breaking pour Swift ou Kotlin et
DOIT être classé dans l’artefact de plateforme.

## Tests de conformité

La suite de conformité DOIT couvrir :

- record et variante sans identité ni allocation heap obligatoire ;
- fonction multiclause exhaustive sur une variante fermée ;
- ajout de cas rendant les consommateurs exhaustifs invalides ;
- nouvelle implémentation non chevauchante d’un protocole ouvert ;
- rejet d’une implémentation locale ou chevauchante ;
- dispatch statique monomorphisé et dispatch existentiel conservé ;
- ambiguïté de deux extensions receveur ;
- preuve que l’ordre des imports ne résout pas cette ambiguïté ;
- opacité de représentation à travers modules, debug et sérialisation ;
- égalité de valeur distincte de l’identité ;
- refus de copier une ressource linéaire ;
- mutation locale via `inout` puis publication d’une valeur immuable ;
- fusion d’une chaîne fonctionnelle en boucle sans collection intermédiaire ;
- boucle impérative produisant la même valeur que l’interprétation
  fonctionnelle ;
- rejet de l’héritage entre deux types Robine ;
- confinement d’une sous-classe étrangère dans son adaptateur ;
- acteur utilisé pour concurrence et valeur utilisée sans acteur lorsque
  l’identité n’est pas nécessaire ;
- absence de table de dispatch dans un programme uniquement statique ;
- rapport d’une table, d’un boxing et d’une spécialisation empêchée ;
- conversion explicite d’un objet étranger en snapshot ;
- mapping distinct record, variante, protocole et ressource vers une plateforme.

## Alternatives rejetées

La pureté fonctionnelle globale est rejetée : elle imposerait ownership,
allocation et structures persistantes à des régions qui nécessitent mutation
locale et layout contrôlé.

Le modèle « tout est objet » est rejeté : il confond valeur, identité,
encapsulation, dispatch et allocation.

Le modèle « toute opération est une méthode » est rejeté : il attache les
transformations au propriétaire du stockage et rend l’extension asymétrique.

L’interdiction de la notation receveur est rejetée : l’ergonomie d’appel peut
être conservée sans adopter l’héritage ni l’objet universel.

## Questions ouvertes

- Syntaxe exacte de la notation receveur et des extensions.
- Présence éventuelle de blocs d’implémentation uniquement organisationnels.
- Nom public du type existentiel de protocole.
- Politique standard d’identité pour les snapshots d’acteurs persistants.
