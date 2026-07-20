# DATA-002 — Données logiques et layouts physiques

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `data`

## Objet

Séparer le modèle logique des records, variantes et collections de leur
représentation physique afin de permettre valeurs inline, AoS, SoA, AoSoA,
tiling, SIMD et buffers CPU/NPU sans modifier le code métier. La présente spec
définit les contrats de layout, vues, conversions, stabilité d’adresse,
aliasing et observabilité des copies.

## Non-objectifs

- imposer un unique layout optimal ;
- promettre qu’une conversion ou un transfert est toujours évitable ;
- exposer les détails internes d’un backend NPU ;
- autoriser une optimisation à changer égalité, ordre ou précision numérique ;
- définir les schémas de sérialisation de DATA-001 ;
- rendre toute valeur adressable ou compatible ABI ;
- sélectionner la syntaxe source des annotations de layout.

## Spécification normative

### Modèle logique et représentation

Un type logique décrit :

- champs et variantes ;
- types et raffinements ;
- égalité et identité éventuelle ;
- ownership et mutabilité ;
- comportement de pattern matching.

Un layout physique décrit :

- ordre, offsets et alignements ;
- séparation ou entrelacement des champs ;
- taille et padding ;
- représentation des tags ;
- strides, tiling et largeur de bloc ;
- espace mémoire et cohérence ;
- stabilité d’adresse.

Deux valeurs de même type logique sont observablement équivalentes même si
leurs layouts physiques diffèrent, sauf lorsqu’un contrat public rend le layout
observable.

Une déclaration de record ou variante NE DOIT PAS fixer implicitement :

- allocation heap ;
- en-tête objet ;
- pointeur d’identité ;
- ordre ABI des champs ;
- padding stable ;
- représentation AoS ;
- largeur de tag.

### Familles de layouts

Le système de layout représente au minimum :

```text
auto
inline
aos
soa
aosoa<Block>
packed
aligned<N>
strided<...>
tiled<...>
external<Contract>
```

Ces noms sont sémantiques et ne choisissent pas leur syntaxe finale.

`auto` autorise le compilateur à sélectionner et transformer la représentation
à l’intérieur des frontières non observables.

`external<Contract>` fixe une représentation fournie par une ABI, un SDK, un
fichier mappé, un protocole, un moteur graphique ou un accélérateur.

`packed` NE DOIT PAS être supposé correctement aligné pour les accès natifs. Le
backend doit générer des accès admissibles ou refuser la combinaison cible.

### Descripteurs de buffers et vues

Les APIs de données contiguës ou stridées exposent conceptuellement :

```text
Buffer<Element, Layout, MemorySpace, Ownership>
View<Element, Layout, MemorySpace, Access>
```

Cette notation est indépendante de la syntaxe et des noms définitifs de la
bibliothèque.

Un `Buffer` possède ou partage explicitement un stockage. Une `View` décrit une
fenêtre bornée par :

- propriétaire ou ancre de durée de vie ;
- offset ;
- forme ;
- strides ;
- alignement connu ;
- droit de lecture ou écriture ;
- espace mémoire.

Construire une vue DOIT vérifier statiquement ou dynamiquement ses bornes,
alignement et durée de vie.

Deux vues mutables simultanées exigent une preuve de non-chevauchement ou une
primitive d’aliasing explicite. Une vérification dynamique de chevauchement
DOIT être visible dans le plan de coût et est interdite dans `realtime` si elle
n’est pas bornée.

### AoS, SoA et reconstruction logique

Une collection logique de records PEUT être représentée :

- comme suite de records complets en AoS ;
- comme colonnes de champs en SoA ;
- comme blocs de colonnes en AoSoA ;
- comme layout tuilé adapté à un kernel.

L’accès à un élément SoA produit une valeur logique ou une vue de ligne. Il NE
DOIT PAS prétendre fournir l’adresse d’un record AoS contigu inexistant.

Une modification de champ à travers une vue de ligne mutable met à jour la
colonne correspondante et respecte l’exclusivité de TYPE-005.

Les opérations bulk DEVRAIENT travailler directement sur colonnes ou tuiles
lorsque cela évite reconstruction, gather ou copie.

### Adresse et identité

Une valeur n’a pas d’adresse stable par défaut. Une optimisation PEUT la
déplacer, l’inline, la décomposer en colonnes ou l’éliminer.

Demander une adresse stable :

- épingle le stockage concerné ;
- fixe une durée de vie ;
- limite les transformations de layout ;
- produit un borrow ou handle conforme à TYPE-005 ;
- apparaît dans le rapport d’optimisation.

Une adresse NE DOIT PAS servir d’identité métier selon LANG-005 sans contrat
d’identité explicite.

Après réallocation, retile ou conversion, les pointeurs et vues antérieurs sont
invalides sauf si le contrat du propriétaire garantit leur stabilité.

### Sélection de layout

Le choix automatique tient compte au minimum :

- profil CPU, largeur vectorielle et alignements ;
- cache et topologie mémoire ;
- ordre et fréquence des parcours ;
- champs lus ou écrits ensemble ;
- cardinalité et bornes ;
- coûts de gather/scatter ;
- contraintes temps réel ;
- ABI et plateformes traversées ;
- transferts CPU/NPU ;
- taille de code et nombre de variantes.

Un build scellé DOIT enregistrer le layout choisi pour chaque frontière où ce
choix affecte performances, ABI ou compatibilité.

Une sélection adaptive au runtime PEUT exister à une frontière de container
explicitement adaptative. Elle NE DOIT PAS changer le layout d’un stockage
pendant qu’une vue, adresse ou opération étrangère l’utilise.

Le programme PEUT exiger un layout. Une exigence impossible sur la cible
produit un diagnostic, pas un fallback silencieux vers un layout incompatible.

### Transformations de layout

Une transformation entre layouts est :

- une vue lorsque seuls offsets, strides ou interprétation changent sans copie ;
- une matérialisation lorsqu’un nouveau stockage est produit ;
- un transfert lorsqu’elle change d’espace mémoire ;
- une conversion lorsqu’elle change représentation, précision ou encodage.

Le compilateur NE DOIT PAS présenter une matérialisation, un transfert ou une
conversion comme une vue.

Toute opération de ce type DOIT apparaître dans :

- l’IR de domaines ;
- le plan mémoire ;
- le profiler ;
- les effets d’allocation et de suspension pertinents ;
- la comptabilité d’énergie lorsqu’elle existe.

Deux transformations consécutives PEUVENT être fusionnées ou supprimées si la
représentation contractuelle aux frontières et le contrat numérique sont
préservés.

### Zéro copie

Une frontière peut déclarer `zero-copy` uniquement lorsque :

- les deux côtés acceptent le même layout et alignement ;
- l’espace mémoire est partagé ou importable ;
- la cohérence cache et la synchronisation sont définies ;
- l’ownership et la durée de vie sont compatibles ;
- aucune conversion de précision ou d’encodage n’est requise.

Si l’une de ces conditions manque, une exigence `require zero-copy` échoue.
Une préférence `prefer zero-copy` PEUT utiliser une copie explicitement
rapportée.

Le transfert d’un buffer exclusivement possédé entre acteurs d’un même
processus suit RUN-005 et NE DOIT PAS copier son contenu lorsque les conditions
de représentation sont compatibles.

### Records et variantes

Un record non exposé par ABI PEUT être :

- entièrement éliminé ;
- scalar-replaced ;
- passé en registres ;
- stocké inline ;
- séparé en champs.

Une variante PEUT utiliser tag explicite, niche disponible ou représentation
spécialisée. La stratégie choisie NE DOIT PAS rendre un état invalide
constructible par du code Robine sûr.

Une niche provenant d’un pointeur, d’un nombre ou d’un tag étranger ne peut être
utilisée si l’ABI externe réserve ou observe autrement cette valeur.

La réflexion de layout exige une capacité ou un profil qui fixe la
représentation. La réflexion logique sur champs et variantes ne fixe pas leurs
offsets physiques.

### Génériques et spécialisation

Un container générique NE DOIT PAS imposer boxing universel à ses éléments.
Une release scellée DEVRAIT spécialiser tailles, alignements et opérations des
types concrets lorsqu’un budget de taille de code l’autorise.

Une représentation uniforme PEUT être conservée à une frontière existentielle,
dynamique ou plugin. Son coût DOIT être limité à cette frontière et rapporté.

Le compilateur PEUT partager une spécialisation entre types dont layout et
opérations requises sont compatibles, sans confondre leurs identités de type
publiques.

### CPU, SIMD et NPU

Une transformation vectorisée respecte :

- alignement ou chemin de prologue nécessaire ;
- absence d’aliasing prouvée ;
- contrat numérique de COMP-004 ;
- bornes de boucle ;
- stratégie de reste.

Un layout destiné au NPU décrit espace mémoire, formes, strides, précision et
contraintes de tiling acceptées par COMP-002.

Le passage CPU vers NPU suit COMP-003. Un layout NPU ne devient pas le layout
de tout le modèle métier uniquement pour éviter une conversion locale.

Un plan PEUT conserver plusieurs variantes de layout si leur coût de mémoire
et de synchronisation est admis. Il DOIT montrer quelle variante est
canonique, dérivée ou reconstruite.

### Temps réel

Un chemin `realtime` utilise uniquement des layouts, buffers, vues et
conversions admis avant son entrée.

Il NE DOIT PAS :

- sélectionner adaptativement un nouveau layout ;
- allouer pour matérialiser une conversion ;
- déplacer un buffer utilisé par le callback ;
- effectuer une synchronisation NPU non bornée ;
- exécuter une vérification de layout non bornée.

Une vue temps réel porte les bornes et alignements validés pendant la phase de
préparation. Les accès dans le callback restent bornés selon RT-001.

### ABI, sérialisation et interopérabilité

Un layout ABI public est distinct d’un layout optimisé privé. Le compilateur
génère une conversion ou prouve leur compatibilité.

Un artefact d’interface qui expose un layout contient au minimum :

- taille et alignement ;
- offsets et types ABI ;
- représentation des tags ;
- endianness si pertinente ;
- règles de padding ;
- convention d’évolution ;
- cibles compatibles.

La sérialisation DATA-001 ne dépend pas du padding ou de l’ordre mémoire privé.
Elle utilise identités de champs et variantes du schéma.

FFI-001 et FFI-003 NE DOIVENT PAS emprunter un pointeur vers une valeur dont le
layout ou la stabilité ne satisfait pas leur contrat.

### Rapport de layout

Le toolchain fournit pour une valeur, collection ou frontière :

```text
logical type
chosen layout
memory space
size and alignment
ownership
views and aliases
copies/materializations
transfers/conversions
pinned constraints
vectorization decision
prevented optimizations
```

Chaque choix automatique important DOIT être relié à son profil et à la
frontière qui le retient.

Une revendication de performance DEVRAIT distinguer temps de calcul, cache
misses, octets déplacés, conversions et synchronisations.

## Diagnostics et erreurs

Le compilateur DOIT diagnostiquer :

- layout exigé non supporté par la cible ;
- adresse demandée pour une représentation décomposée sans matérialisation ;
- vue hors bornes ou désalignée ;
- vues mutables susceptibles de se chevaucher ;
- changement de layout avec borrow ou opération étrangère active ;
- exigence zéro copie impossible ;
- ABI incompatible par taille, alignement, tag ou endianness ;
- conversion cachée interdite dans `realtime` ;
- spécialisation empêchée par réflexion ou existential ;
- accès AoS demandé à une représentation uniquement SoA.

Le diagnostic DEVRAIT proposer vue, matérialisation, copie, épinglage ou
adaptateur ABI selon le besoin réel et annoncer leur coût.

## Sécurité, confidentialité et ressources

Tout calcul d’offset, taille, stride et capacité utilise une arithmétique
vérifiée. Un overflow ou une multiplication de forme invalide est rejeté avant
accès mémoire.

Une vue NE confère pas davantage de droits que son propriétaire. Une vue
lecture ne peut produire une vue mutable ; une vue limitée ne peut élargir ses
bornes.

Les buffers partagés avec un accélérateur ou une plateforme conservent leurs
restrictions de confidentialité et de durée de vie jusqu’au signal explicite
de fin d’utilisation.

Les materialisations et variantes de layout participent aux budgets mémoire.
Le runtime NE DOIT PAS maintenir indéfiniment des copies dérivées sans
politique de rétention.

Le padding d’un layout exporté DOIT être initialisé ou exclu de toute copie
susceptible de révéler des données antérieures.

## Interactions

- LANG-005 distingue valeurs, identité et ressources ;
- TYPE-003 définit records, variantes et protocoles ;
- TYPE-005 définit ownership, borrows, formes et alignements logiques ;
- RUN-001 définit allocation, régions et destruction ;
- RUN-005 définit transfert de buffers entre acteurs ;
- DATA-001 sépare schéma sérialisé et mémoire ;
- RT-001 contraint les layouts temps réel ;
- COMP-001 définit espaces CPU et NPU ;
- COMP-002 définit tenseurs, vues et layout logique de kernels ;
- COMP-003 choisit placement et transferts ;
- COMP-004 définit le contrat numérique ;
- CPL-001 effectue spécialisation et bufferisation ;
- FFI-001 fixe les ABI natives ;
- FFI-003 fixe les frontières Swift/Kotlin.

## Compatibilité et migration

Changer un layout privé `auto` est compatible tant que résultats, coûts
contractuels et frontières observables restent valides.

Changer un layout public ABI est ABI-breaking. Ajouter un champ à un record
peut être source-compatible dans un schéma mais reste ABI-breaking pour une
représentation fixe.

Épingler une adresse, exiger zéro copie ou exposer une réflexion physique
réduit les optimisations disponibles et modifie le contrat de coût.

Une version de toolchain qui choisit un layout automatique différent DOIT
invalider les caches de code et artefacts qui incorporent ce layout. Les
données sérialisées restent régies par DATA-001.

## Tests de conformité

La suite de conformité DOIT couvrir :

- même résultat logique en AoS, SoA, AoSoA et layout tuilé ;
- pattern matching identique sur records et variantes représentés différemment ;
- absence d’en-tête objet obligatoire pour une valeur inline ;
- scalar replacement d’un record privé ;
- accès colonne sans reconstruction de chaque record SoA ;
- modification d’un champ à travers une vue de ligne unique ;
- rejet de deux vues mutables chevauchantes ;
- vue read-only partagée valide ;
- invalidation des vues après reallocation ou retile ;
- adresse stable empêchant une transformation incompatible ;
- distinction entre identité métier et adresse ;
- vue stride-only sans copie ;
- matérialisation AoS vers SoA rapportée ;
- fusion de deux conversions compatibles ;
- succès et échec d’une exigence zéro copie ;
- fallback `prefer zero-copy` avec copie visible ;
- ABI fixe comparée sur toutes les cibles déclarées ;
- rejet d’une niche incompatible avec une ABI étrangère ;
- sérialisation identique depuis plusieurs layouts privés ;
- vectorisation avec alignement, aliasing et boucle de reste ;
- transfert CPU/NPU et coût de layout dans le profiler ;
- refus d’une matérialisation cachée dans `realtime` ;
- padding exporté sans fuite de mémoire ;
- invalidation correcte des caches après changement de layout ;
- rapport expliquant chaque copie, épinglage et optimisation empêchée.

## Alternatives rejetées

AoS universel est rejeté : il couple le modèle logique à un parcours et
gaspille cache et bande passante pour les calculs par colonnes.

SoA universel est rejeté : il pénalise accès par entité, petites structures,
interop objet et certains algorithmes irréguliers.

Un layout objet heap universel est rejeté : il impose pointeurs, headers,
indirections et GC aux valeurs qui n’en ont pas besoin.

Une transformation silencieuse de layout aux frontières est rejetée : une
copie ou conversion cachée rend coûts, temps réel et consommation énergétique
incompréhensibles.

## Questions ouvertes

- Syntaxe des contraintes et préférences de layout.
- Seuils standards entre AoS, SoA et AoSoA par profil matériel.
- Ensemble minimal de layouts stables dans l’ABI Robine publique.
- Représentation des vues de lignes SoA dans les APIs génériques.
- Politique de versionnement des layouts NPU fournis par les plateformes.
