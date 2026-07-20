# FFI-003 — Interopérabilité objet Swift et Kotlin

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `interop`

## Objet

Définir une interopérabilité bidirectionnelle avec Swift sur plateformes Apple
et Kotlin sur Android sans introduire l’héritage ou l’objet universel dans le
modèle Robine. La présente spec couvre projections de types, handles étrangers,
ARC, références JNI, threading, erreurs, async, callbacks, façades générées et
frontières de performance.

## Non-objectifs

- fournir une UI commune indépendante des plateformes ;
- garantir l’ABI interne de Swift ou la disposition des objets ART ;
- importer toute hiérarchie de classes comme sous-typage Robine ;
- autoriser des appels objet dans une boucle DSP ou kernel ;
- sérialiser implicitement un graphe d’objets de plateforme ;
- définir l’interopérabilité Kotlin/Native comme identique à Kotlin/Android ;
- exposer JNI, Objective-C ou une ABI C comme API métier écrite manuellement.

## Spécification normative

### Profils de plateforme

Le profil `swift-apple` produit au minimum :

- bibliothèque native Robine statique ou dynamique compatible avec la cible ;
- module ABI minimal généré ;
- façade Swift générée ;
- métadonnées de disponibilité, ownership et threading ;
- artefact empaquetable par les outils Apple.

Le profil `kotlin-android` produit au minimum :

- bibliothèque native par ABI Android déclarée ;
- couche JNI générée ;
- façade Kotlin empaquetable en AAR ;
- règles de conservation des références JVM/ART ;
- métadonnées d’API Android et d’exécuteur.

Un profil Kotlin/Native, desktop JVM ou autre backend PEUT être ajouté, mais il
DOIT déclarer ABI, GC, threading, exceptions et packaging séparément. La
compatibilité Kotlin source ne suffit pas à prétendre que les runtimes sont
équivalents.

### Principe de frontière

Une API Robine possède un contrat canonique indépendant de Swift et Kotlin. Les
façades de plateforme sont générées depuis ce contrat selon ARCH-001.

Le générateur NE DOIT PAS modifier :

- types métier ;
- effets ;
- ownership ;
- annulation ;
- ordre des événements ;
- politique d’erreur ;
- garanties de domaine.

Il PEUT choisir une représentation hôte idiomatique lorsque la projection est
sans perte. Toute projection avec perte ou ambiguïté exige un wrapper nominal
ou une conversion explicite.

La couche ABI minimale n’est pas éditée manuellement. Le source de vérité reste
le contrat Robine et les métadonnées étrangères importées.

### Projection de Robine vers les hôtes

La projection standard est :

| Robine | Swift | Kotlin/Android |
|---|---|---|
| record valeur | `struct` ou wrapper valeur | data class ou value class admissible |
| variante fermée | `enum` à valeurs associées ou wrapper scellé | sealed interface/class et data classes |
| type opaque | classe finale à handle | classe finale à handle natif |
| ressource | classe possédée/fermable | classe possédée, `AutoCloseable` si pertinent |
| protocole existentiel | protocol | interface |
| `Option<T>` | optionnel si projection injective | nullable si projection injective |
| `Result<T,E>` | résultat nominal ou `throws` autorisé | résultat nominal ou exception autorisée |
| `Task<T,E>` | fonction `async` | fonction `suspend` |
| `Stream<T,E>` | `AsyncSequence` ou adaptateur | `Flow` ou adaptateur |
| acteur | façade à messages ou actor compatible | façade à messages ou service compatible |
| buffer/vue | span, buffer ou handle natif | buffer direct ou handle natif |

Une projection optionnelle n’est injective que si la valeur `null` étrangère ne
peut pas également représenter un membre valide de `T`. Les `Option<Option<T>>`
et distinctions similaires utilisent un wrapper nominal.

Une variante qui ne peut pas être représentée fidèlement par une enum Swift ou
hiérarchie Kotlin utilise une façade nominale conservant tag, payload et cas
inconnus requis par son contrat d’évolution.

Une classe hôte générée pour un type opaque NE DOIT PAS exposer son handle
numérique comme identité ou API publique mutable.

### Projection des objets étrangers vers Robine

Une classe, instance de SDK ou objet de plateforme importé devient
conceptuellement :

```text
ForeignObject<Platform, ForeignType, Ownership, Executor>
```

Cette notation est sémantique. Le handle est opaque et ne devient pas un record
structurel.

Le contrat d’un objet étranger contient au minimum :

- identité de type et disponibilité ;
- ownership : borrowed, retained, consumed ou globalement ancré ;
- nullabilité ;
- exécuteur ou thread requis ;
- mobilité entre threads ;
- méthodes, propriétés et constructeurs accessibles ;
- erreurs, callbacks et réentrance ;
- cycle de vie et politique de libération.

Lire les propriétés d’un objet pour construire une valeur Robine est une
opération explicite de snapshot. Le snapshot ne conserve pas l’identité vivante
de l’objet sauf identifiant durable déclaré.

Une méthode étrangère peut utiliser la notation receveur de LANG-005. Le
service de langage DOIT signaler qu’il s’agit d’un dispatch étranger et montrer
thread, effets, disponibilité et ownership.

### Swift et ARC

Un handle Swift ou Objective-C déclare si Robine :

- emprunte l’objet pour la durée d’un appel ;
- le retient au-delà de l’appel ;
- consomme une propriété transférée ;
- conserve une référence faible.

Le wrapper généré équilibre retain/release ou les opérations équivalentes. Une
optimisation NE DOIT PAS supprimer une rétention nécessaire à un callback,
une tâche ou un passage de thread.

Une référence empruntée Swift NE DOIT PAS être stockée dans un acteur,
continuation ou ressource dont la durée dépasse le borrow.

Les cycles entre closure Robine et objet Swift DOIVENT être évités par une
politique explicite : capture faible, fermeture déterministe, annulation ou
rupture de cycle.

Une opération susceptible de produire des objets autoreleased DOIT s’exécuter
dans un contexte de pool compatible ou déléguer cette responsabilité à la
façade Swift.

Le profil NE DOIT PAS dépendre directement d’un layout interne de classe Swift.
Une interop Swift résiliente utilise la façade compilée avec le toolchain
compatible et l’ABI minimale déclarée.

### Kotlin, ART et JNI

Un objet Kotlin/Java importé reste géré par ART. Le code natif NE DOIT PAS
conserver l’adresse interne de l’objet ou d’un champ à travers un appel JNI.

La couche générée distingue :

- référence locale limitée à l’appel ou frame JNI ;
- référence globale avec libération déterminée ;
- référence globale faible ;
- handle natif possédé par une façade Kotlin.

Une référence locale NE DOIT PAS être conservée dans un acteur, callback
asynchrone ou thread natif après le retour JNI.

Un thread natif appelant Kotlin DOIT être attaché selon le profil puis détaché
selon sa politique. L’attachement NE DOIT PAS être effectué dans un domaine
`realtime`.

Les classes, méthodes et champs JNI fréquemment utilisés DEVRAIENT être résolus
et mis en cache à l’initialisation du module, avec validation de version. Une
lookup JNI par élément dans une boucle bulk est non conforme au profil de
performance standard.

Une exception Kotlin/Java en attente DOIT être détectée et convertie avant tout
autre appel JNI non autorisé. Elle NE DOIT PAS traverser la frontière native
comme unwinding.

### Méthodes, propriétés et surcharge

Le générateur normalise chaque opération étrangère vers un symbole stable
contenant :

- propriétaire nominal ;
- nom et surcharge ;
- paramètres, nullabilité et génériques projetables ;
- mutabilité du receveur ;
- effets ;
- disponibilité ;
- convention d’erreur ;
- exécuteur.

Une propriété étrangère devient une lecture et, si disponible, une écriture
distinctes dans le contrat. Le sucre de propriété NE DOIT PAS cacher I/O,
allocation, blocage ou changement d’exécuteur.

Une surcharge qui reste ambiguë après typage Robine exige une qualification ou
un wrapper généré. Le compilateur NE DOIT PAS choisir par ordre de déclaration
ou conversion avec perte non demandée.

Les paramètres par défaut hôtes sont matérialisés par des façades ou surcharges
générées. Leur valeur et version DOIVENT être connues du contrat ; Robine NE
DOIT PAS supposer qu’une valeur par défaut compilée dans un autre module reste
identique.

### Protocoles, interfaces et delegates

Un protocole Swift ou une interface Kotlin PEUT être importé comme protocole
Robine lorsque ses opérations, variance, threading et ownership sont
représentables.

Une implémentation Robine exportée utilise un adaptateur hôte généré. Cet
adaptateur :

- conserve le handle Robine ;
- traduit appels et erreurs ;
- respecte le thread requis ;
- gère sa durée de vie ;
- empêche les appels après fermeture ;
- documente la réentrance.

Une hiérarchie de classes étrangère NE DOIT PAS devenir une hiérarchie Robine.
Les relations nécessaires à un SDK sont conservées dans la façade hôte.

Une sous-classe générée est autorisée uniquement lorsqu’un framework exige
l’héritage. Elle DOIT lister les méthodes overridées, appels `super`, threading
et état hôte conservé. Le code métier reçoit une interface ou capacité
compositionnelle.

### Closures et callbacks

Un callback traversant la frontière déclare :

- direction ;
- durée de vie ;
- nombre d’appels ;
- thread ou exécuteur ;
- synchronisme ;
- possibilité de réentrance ;
- ownership des arguments et résultat ;
- politique après annulation ou fermeture.

Une closure Robine retenue par Swift ou Kotlin possède un token de cycle de vie.
La façade DOIT libérer ce token exactement une fois lorsque le contrat se
termine.

Un callback synchrone NE DOIT PAS être transformé en callback asynchrone sans
changer le contrat. Un callback asynchrone NE DOIT PAS conserver des borrows
limités à l’appel d’origine.

La réentrance dans un acteur en cours d’exécution passe par sa mailbox, sauf
primitive explicitement déclarée réentrante et vérifiée.

### Erreurs

Les erreurs Robine traversent la frontière par :

- valeur résultat nominale ;
- code et payload ABI ;
- `throws` Swift généré ;
- exception Kotlin générée ;
- annulation distincte.

La projection vers exception n’est autorisée que si le type d’erreur, sa
causalité et les erreurs inconnues restent récupérables. Sinon la façade expose
un résultat nominal.

Une exception Swift, Objective-C, Kotlin ou Java est capturée dans la couche
hôte et convertie vers une erreur Robine déclarée. Aucun unwinding étranger NE
DOIT traverser la bibliothèque native.

Une faute de programmation hôte ne doit pas être présentée comme erreur métier
si le contrat la classe comme abort, violation de précondition ou faute
étrangère non récupérable.

### Async, annulation et streams

Une fonction Robine `Task<T,E>` PEUT devenir `async throws` en Swift ou
`suspend` en Kotlin lorsque la projection d’erreur est définie.

La façade conserve les quatre issues de `TaskOutcome<T,E>` définies par
RUN-002 :

- `Succeeded(T)` complète normalement ;
- `Failed(E)` utilise la projection d’erreur déclarée ;
- `Cancelled` utilise l’annulation hôte sans devenir une erreur métier `E` ;
- `RuntimeFault` devient une faute Robine structurée distincte ou termine la
  façade selon sa politique de faute publiée.

Une façade NE DOIT PAS convertir silencieusement annulation ou faute runtime en
une valeur de `E`.

L’annulation hôte est propagée au scope Robine. L’annulation Robine termine ou
annule la primitive hôte lorsque son API le permet. Si l’opération étrangère
n’est pas interruptible, son travail résiduel suit RUN-002 et RUN-005.

Une continuation hôte est complétée exactement une fois. Une completion
tardive après annulation est ignorée ou journalisée selon le contrat, jamais
livrée à une tâche déjà terminée.

Un stream applique une contre-pression ou une stratégie de buffer bornée. Une
conversion vers `Flow` ou `AsyncSequence` NE DOIT PAS introduire une queue
illimitée.

Les deadlines utilisent une horloge et une unité explicitement traduites. Une
deadline ne devient pas un simple timeout décoratif perdu à la frontière.

### Threading et cycle de vie

Les contraintes suivantes sont des propriétés de type ou d’effet :

- main thread / MainActor Apple ;
- main looper Android ;
- thread confiné ;
- sendable ;
- thread-safe ;
- callback sur exécuteur fourni.

Un appel depuis le mauvais exécuteur est rejeté statiquement lorsque possible.
Sinon le wrapper retourne une erreur ou programme un dispatch asynchrone
explicitement prévu.

Un wrapper NE DOIT PAS effectuer un dispatch synchrone caché vers le thread UI,
car il peut bloquer ou provoquer un deadlock.

Les événements de cycle de vie plateforme sont convertis vers le modèle hôte
de l’application. Suspendre une application invalide ou met en pause les
opérations selon leur contrat ; cela ne constitue pas une garantie de
continuité du scheduler.

### UI native

Les types de vue, navigation et interaction restent Swift/Kotlin et suivent
UI-001.

Le code partagé Robine peut fournir :

- état de domaine ;
- événements ;
- commandes ;
- reducers ;
- validations ;
- tâches et streams.

La façade UI transforme ces valeurs en mises à jour natives. Elle NE DOIT PAS
exporter une hiérarchie virtuelle commune de widgets pour simuler l’identité
des SDK.

Les snapshots transmis à l’UI sont immuables ou possédés selon leur contrat.
Une vue hôte ne reçoit pas un borrow vers un état Robine susceptible d’être
déplacé après l’appel.

### Frontières de performance

Un appel Swift ou JNI possède un coût de frontière. Le générateur DOIT
favoriser :

- opérations bulk ;
- buffers contigus ou stridés ;
- handles opaques ;
- batchs de commandes ;
- snapshots compacts ;
- streams bornés.

Une API critique NE DEVRAIT PAS effectuer un appel étranger par élément,
échantillon audio, pixel ou cellule tensorielle.

Les buffers suivent DATA-002. Une vue zéro copie est autorisée seulement si
layout, alignement, durée de vie, cohérence et ownership sont compatibles.

Sur Android, un buffer direct ou handle matériel PEUT éviter la copie ; le
contrat NE DOIT PAS supposer qu’un objet Kotlin ordinaire fournit une adresse
stable.

Sur Apple, un buffer natif ou de framework PEUT être emprunté ou partagé si sa
durée de vie et sa synchronisation sont déclarées. Un tableau Swift ordinaire
NE DOIT PAS être supposé stable ou contigu au-delà de la garantie fournie par
la façade.

### Domaines critiques

Un appel objet Swift/Kotlin est interdit depuis `realtime` sauf wrapper
spécifiquement certifié par FFI-001. ARC, JNI, dispatch UI, exceptions,
allocation hôte et callbacks généraux ne sont pas certifiés par défaut.

Le chemin DSP utilise des paramètres et buffers préparés hors callback. UI et
plateforme communiquent avec lui par les files bornées de RT-002.

Un kernel NPU ou CPU ne reçoit pas d’objet Swift/Kotlin. Il reçoit buffers,
formes, constantes et événements de synchronisation.

Un appel bloquant utilise un worker `isolated` ou une API asynchrone selon
FFI-001. Le marquage `suspend` ou `async` d’une façade n’efface pas un blocage
interne non déclaré.

### Génération et audit

Le générateur produit de manière déterministe :

- couche ABI ;
- façades Swift/Kotlin ;
- adaptateurs de protocoles et callbacks ;
- métadonnées d’ownership et threading ;
- source maps ;
- tests de round-trip de types ;
- rapport des conversions et éléments `unsafe`.

Chaque symbole généré renvoie au contrat Robine ou étranger qui l’a produit.

Le rapport de build contient :

- objets et méthodes accessibles ;
- global/weak references JNI ;
- opérations ARC ;
- dispatchs de thread ;
- copies et materialisations ;
- callbacks retenus ;
- exceptions converties ;
- APIs non projetables ;
- code manuel ou `unsafe` restant.

Une façade personnalisée PEUT compléter le générateur. Elle devient une
frontière auditée et NE DOIT PAS modifier les fichiers générés directement.

## Diagnostics et erreurs

Le compilateur ou générateur DOIT diagnostiquer :

- méthode ou propriété indisponible sur la cible ;
- projection nullable non injective ;
- surcharge ambiguë ;
- référence Swift/JNI conservée au-delà de sa durée ;
- handle utilisé après fermeture ;
- appel depuis le mauvais exécuteur ;
- objet non transférable envoyé entre threads ;
- exception étrangère non convertie ;
- callback dont la durée de vie est inconnue ;
- cycle de rétention probable ;
- dispatch synchrone UI caché ;
- appel objet depuis `realtime` ;
- boucle comportant un appel étranger par élément ;
- demande zéro copie incompatible ;
- hiérarchie étrangère non projetable comme protocole.

Un diagnostic montre le contrat source, le wrapper généré concerné et la
correction sûre : copie, snapshot, retain, weak, dispatch asynchrone, batch,
worker isolé ou adaptateur manuel.

## Sécurité, confidentialité et ressources

Un objet étranger conserve l’autorité de ses méthodes. L’importer NE DOIT PAS
accorder l’accès à d’autres objets, services ou APIs de plateforme non déclarés.

Les handles sont non forgeables. Un entier provenant d’une entrée externe NE
PEUT PAS devenir un handle natif sans validation par son propriétaire.

Les références globales JNI, retains Swift, callbacks, buffers partagés et
tâches en vol participent aux budgets de ressources et apparaissent dans le
profiler de fuites.

Les snapshots excluent secrets et champs privés sauf capacité explicite. Une
description, un log ou une exception hôte NE DOIT PAS révéler automatiquement
le contenu d’un type secret Robine.

La couche ABI valide nullabilité, tailles, offsets, tags, alignement et version
avant de construire une valeur sûre. Une faute dans un wrapper `unsafe` reste
hors de la garantie d’isolation en processus de RUN-005.

## Interactions

- LANG-005 définit valeurs, identité, protocoles et notation receveur ;
- TYPE-003 définit records, variantes et protocoles ;
- TYPE-004 définit effets et capacités de plateforme ;
- TYPE-005 définit ownership, borrows et mobilité ;
- RUN-001 définit destruction des ressources ;
- RUN-002 définit tâches, annulation et deadlines ;
- RUN-003 définit acteurs et callbacks par mailbox ;
- RUN-005 définit isolation et travail résiduel ;
- DATA-001 définit snapshots et schémas ;
- DATA-002 définit buffers, vues, layouts et zéro copie ;
- RT-002 définit la frontière avec l’audio ;
- ARCH-001 fournit le contrat canonique ;
- UI-001 conserve une UI native ;
- FFI-001 définit ABI, blocage, callbacks et certification temps réel.

## Compatibilité et migration

La version 0.2.0 projette explicitement les quatre issues de `TaskOutcome` et
sépare annulation, faute runtime et erreur métier. Une façade qui mélangeait
ces issues doit régénérer son API ; ce changement est source-breaking pour
l’hôte.

Une modification du contrat Robine est classée par ARCH-001 avant projection.
Chaque façade ajoute sa classification source Swift et Kotlin ainsi que son
impact ABI natif.

Ajouter une valeur par défaut, une surcharge ou une variante peut être
compatible dans Robine mais ambigu ou source-breaking sur un hôte. Le
générateur DOIT détecter cette divergence.

Changer borrowed vers retained peut être source-compatible mais modifie coût
et cycle de vie. Changer retained vers borrowed est source-breaking pour les
consommateurs qui stockent le handle.

Une nouvelle exigence de thread, disponibilité, copie ou dispatch est une
modification de contrat.

Les façades générées sont versionnées avec le contrat et le toolchain de
plateforme. Un ancien wrapper NE DOIT PAS charger silencieusement une ABI
incompatible.

## Tests de conformité

La suite de conformité DOIT couvrir :

- projection d’un record vers Swift et Kotlin puis round-trip ;
- variante avec payload et exhaustivité des cas ;
- type opaque projeté en classe finale sans exposition du handle ;
- `Option<T>` nullable injectif et `Option<Option<T>>` nominal ;
- résultat nominal et projection contrôlée vers exceptions ;
- tâche vers `async`/`suspend` avec succès, erreur et annulation ;
- projection distincte de `Succeeded`, `Failed`, `Cancelled` et
  `RuntimeFault` ;
- stream vers `AsyncSequence`/`Flow` avec contre-pression bornée ;
- protocole/interface implémenté dans chaque direction ;
- snapshot explicite d’un objet étranger ;
- refus de sérialiser directement un objet vivant ;
- borrow Swift valide et conservation interdite ;
- retain/release équilibrés y compris en cas d’erreur ;
- cycle closure/objet rompu par la politique déclarée ;
- référence JNI locale non conservée après retour ;
- référence globale créée et libérée exactement une fois ;
- référence faible collectée sans use-after-free ;
- attachement et détachement d’un thread natif ;
- exception JNI convertie avant appel suivant ;
- validation d’une API absente sur une version de plateforme ;
- surcharge ambiguë exigeant qualification ;
- callback synchrone, asynchrone, réentrant et annulé ;
- completion tardive après annulation ;
- main thread/MainActor et main looper correctement imposés ;
- refus du dispatch synchrone UI caché ;
- batch de valeurs sans appel étranger par élément ;
- buffer compatible zéro copie et fallback par copie visible ;
- refus d’adresse stable sur objet ART ordinaire ;
- sous-classe SDK confinée dans l’adaptateur hôte ;
- refus d’un objet plateforme dans `realtime` et un kernel ;
- paramètres audio transmis par queue bornée ;
- détection de fuite de retain, global ref, callback et tâche ;
- compatibilité de façade vérifiée entre deux versions ;
- déterminisme et provenance complète du code généré.

## Alternatives rejetées

Projeter toute valeur Robine en classe est rejeté : cela impose identité,
allocation et dispatch à des records et variantes qui n’en ont pas besoin.

Importer l’héritage Swift/Kotlin comme héritage Robine est rejeté : les
relations de sous-typage, stockage et cycle de vie des plateformes ne sont pas
le modèle métier portable.

Marshaler automatiquement un graphe d’objets est rejeté : identité, cycles,
lazy properties, thread et coût deviendraient implicites.

Appeler JNI ou Swift élément par élément est rejeté pour les chemins critiques :
les APIs bulk et buffers décrivent mieux le coût.

Une ABI Swift interne ou adresse d’objet ART supposée stable est rejetée : les
façades et handles doivent utiliser les contrats officiellement supportés par
la cible.

## Questions ouvertes

- Niveau d’interop direct Swift conservé en plus de la façade ABI minimale.
- Profil distinct pour Kotlin/Native et desktop JVM.
- Projection canonique des variantes évolutives vers Swift.
- Abstraction standard commune aux buffers Apple et Android partageables.
- Politique de génération des sous-classes exigées par les frameworks UI.
