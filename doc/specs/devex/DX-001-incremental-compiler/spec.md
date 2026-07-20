# DX-001 — Compilateur incrémental et compilation étagée

- Statut : **Draft**
- Version : **0.4.0**
- Domaine : `devex`

## Objet

Fournir une réponse interactive rapide sans imposer un interpréteur ou une VM
à la release.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Service de compilation

Le compilateur de développement est un service persistant qui conserve :

- arbres syntaxiques incrémentaux ;
- résolution de noms ;
- contraintes et types ;
- IR par définition ;
- graphe de dépendances ;
- caches de monomorphisation et codegen ;
- versions installées dans les processus connectés.

Chaque résultat est une requête pure ou explicitement dépendante d’une entrée
versionnée. Une modification invalide le sous-graphe minimal.

### Identités stables

Modules, définitions et nœuds significatifs possèdent des identités stables à
travers formatage et éditions locales. Une identité ne dépend pas uniquement de
la position en octets.

### Trois niveaux

#### Immédiat

Parse, typecheck et code natif local avec optimisations minimales. Cette version
est installable dès que ses contrats sont valides.

#### Chaud

En arrière-plan : spécialisation, fusion, vectorisation, inlining et codegen
plus coûteux. La version remplace l’immédiate à un point sûr.

#### Scellé

Build AOT reproductible avec analyse globale autorisée par les interfaces,
suppression des métadonnées de développement et runtime spécialisé.

Les trois niveaux DOIVENT préserver la même sémantique observable selon le
contrat public. Les effets, erreurs et garanties restent identiques.

Lorsque le contrat exige un résultat exact, les trois niveaux produisent le
même résultat. Pour un calcul régi par COMP-004, chaque niveau PEUT produire
une valeur différente uniquement si elle satisfait `Accepts_C` pour le même
contrat numérique `C`. Un changement de contrat, de profil `fast` ou d’unité
d’évaluation est semantic-breaking, pas une optimisation chaude.

### Frontières d’invalidation

Une modification de corps avec interface identique :

- recompile la définition ;
- PEUT optimiser ses appelants chauds ;
- NE DOIT PAS les retyper.

Une modification d’interface invalide les consommateurs de cette interface
seulement. Les interfaces de dépendances sont chargées depuis ARCH-001.

Pour un projet multi-fichiers, le service DOIT conserver les arêtes d’import
entre identités nominales de modules. Une modification de corps dont
l’interface est inchangée NE DOIT retyper que le module modifié. Une
modification d’interface DOIT retyper ce module et ses consommateurs transitifs
et NE DOIT PAS retyper un module hors de ce sous-graphe.

Une modification d’import ou d’identité de module reconstruit le graphe avant
de calculer le sous-graphe invalidé. Un résultat provenant de l’ancien graphe
NE DOIT PAS être publié comme courant.

### Macros et génération

Une macro structurelle, une dérivation ou un elaborator portable suit
LANG-004. Sa transformation est pure et mise en cache par empreinte de toutes
ses entrées sémantiques.

Une transformation qui exige filesystem, réseau, environnement, processus ou
outil externe est une tâche de build selon PKG-002, pas une macro. Elle produit
un artefact capturé et haché qui devient ensuite une entrée ordinaire de la
compilation incrémentale.

Une tâche de build hermétique PEUT être reproductible. L’environnement
hermétique, les capacités, entrées et sorties font partie de sa clé et de sa
provenance.

### Objectifs mesurables

Les budgets de latence sont définis par profil de dépôt et matériel, au minimum :

- édition locale vers diagnostic ;
- édition valide vers code immédiat ;
- warm build sans changement ;
- changement d’interface ;
- build scellé.

Le projet NE DOIT PAS revendiquer « compilation instantanée » sans publier ces
mesures.

### Cache

Les caches ne sont jamais sources de vérité. Leur corruption ou absence peut
ralentir, pas changer le résultat. Les artefacts distants sont vérifiés selon
PKG-002.

## Diagnostics et erreurs

Le service DOIT exposer, pour les tests et le profilage local, l’ensemble des
modules reparsés et retypés par une mise à jour. Une invalidation plus large
que la frontière normative est un échec de conformité mesurable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- ARCH-001
- LANG-003
- LANG-004
- PKG-002
- COMP-004 définit la conformité des résultats numériques ;
- RUN-005 définit l’équivalence des variantes d’exécution.

## Compatibilité et migration

La version 0.4.0 rend observable l’invalidation au niveau des modules. Les
services qui invalidaient tout le projet après chaque édition doivent adopter
le graphe nominal ; ce changement est compatible pour les résultats et modifie
le contrat de performance du service.

La version 0.3.0 définit l’équivalence entre niveaux par le contrat public et
par `Accepts_C` pour le numérique. Un pipeline qui changeait silencieusement de
contrat numérique entre immédiat, chaud et scellé devient non conforme ; ce
changement est semantic-breaking pour ces builds.

La version 0.2.0 sépare les transformations pures de LANG-004 des tâches de
build de PKG-002. Une ancienne macro qui effectue une I/O de compilation doit
devenir une tâche de build déclarée, puis consommer son artefact haché. Ce
changement est source-breaking pour ces macros et compatible pour les
transformations déjà pures.

## Tests de conformité

La suite de conformité DOIT couvrir au moins :

- modification de corps ne retypant que son module ;
- modification d’interface retypant ses consommateurs transitifs seulement ;
- modification d’import reconstruisant le graphe avant publication ;
- égalité des résultats sous contrat exact ;
- conformité différentielle à `Accepts_C` sous chaque mode COMP-004 ;
- identité des effets, erreurs et garanties ;
- rejet d’un changement silencieux de profil numérique.

## Questions ouvertes

- Format commun des preuves différentielles conservées dans les caches chauds.
