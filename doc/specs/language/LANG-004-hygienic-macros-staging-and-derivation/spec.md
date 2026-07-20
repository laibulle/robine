# LANG-004 — Macros hygiéniques, dérivation et staging

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `language`

## Objet

Définir une métaprogrammation structurelle qui conserve la puissance
d’abstraction des macros Lisp sans transformer chaque dépendance en extension
privilégiée du compilateur. La présente spec sépare macros syntaxiques,
dérivations typées, elaborators locaux, staging et génération externe ; elle
définit leurs phases, leur hygiène, leur autorité, leur provenance, leur
reproductibilité et leur intégration à l’outillage.

## Non-objectifs

- choisir la ponctuation ou la syntaxe source canonique de LANG-002 ;
- exposer les structures internes mutables du compilateur comme API publique ;
- permettre à un package ordinaire de modifier le reader, le lexer, la
  résolution des noms ou la signification des effets standards ;
- remplacer fonctions, modules, protocoles, handlers d’effets ou généricité
  lorsqu’ils expriment directement l’abstraction ;
- garantir la terminaison de tout métaprogramme procédural sans limite de
  ressources ;
- fournir implicitement un compilateur ou un JIT dans une release scellée.

## Spécification normative

### Familles de métaprogrammation

Robine distingue les mécanismes suivants :

| Mécanisme | Entrée principale | Sortie | Phase |
|---|---|---|---|
| macro structurelle | syntaxe non typée | syntaxe | expansion |
| dérivation | déclaration typée | déclarations | élaboration |
| elaborator local | syntaxe délimitée et contexte attendu | terme vérifiable | élaboration |
| staging typé | valeurs et fragments de code typés | code d’une phase ultérieure | spécialisation |
| tâche de build | ressources externes déclarées | artefacts capturés | build |
| plugin de toolchain | API privilégiée du compilateur | comportement d’outil | hors langage standard |

Une API NE DOIT PAS employer le terme `macro` pour une tâche de build ou un
plugin privilégié.

Avant d’introduire une macro publique, son auteur DEVRAIT vérifier que le besoin
ne peut pas être satisfait par :

- une fonction ou un combinator ;
- un type, un protocole ou un handler d’effet ;
- une dérivation standard ;
- une fonction inline ou une spécialisation ;
- une donnée interprétée explicitement.

Cette recommandation n’interdit pas une macro qui doit contrôler binding,
évaluation, déclaration ou structure syntaxique.

### Pipeline et phases

Le pipeline logique de métaprogrammation est :

```text
source
→ lecture par la grammaire canonique
→ arbre syntaxique stable
→ expansion structurelle
→ résolution des noms
→ typage, effets et ownership
→ dérivation et élaboration typées
→ revérification de la HIR
→ staging et spécialisation
→ Core vérifié
→ génération
```

Chaque définition et dépendance de métaprogramme appartient à une phase
explicite. Une valeur d’exécution n’est pas disponible pendant l’expansion et
une valeur d’expansion n’est pas capturée par le programme généré sans
persistance interphase autorisée.

Les imports de phase d’expansion, d’élaboration, de build et d’exécution sont
distincts dans le graphe de dépendances. Importer une bibliothèque à
l’exécution NE DOIT PAS exécuter ou rendre automatiquement disponible son code
de métaprogrammation.

Le compilateur DOIT détecter les cycles de phase et afficher la chaîne de
dépendances qui les forme. Il NE DOIT PAS résoudre un cycle en observant
l’ordre accidentel des fichiers.

### Modèle structurel

L’API publique expose une famille abstraite :

```text
Syntax<Kind, Phase>
```

La notation est sémantique et non une décision de syntaxe source. Une valeur
`Syntax` conserve au minimum :

- catégorie syntaxique ;
- structure et enfants ordonnés ;
- identités structurelles stables ;
- ensembles de scopes ;
- positions et fragments source associés ;
- commentaires ou trivia nécessaires au rendu ;
- origine utilisateur ou générée ;
- phase et version de représentation.

Les symboles résolus, types, effets et ownership ne sont présents que dans les
vues dont la phase les a établis.

Une macro NE DOIT PAS produire du code par concaténation de texte. La
construction passe par templates structurels, builders versionnés ou
combinateurs de `Syntax`.

La conversion d’une structure de données ordinaire en syntaxe ne lui confère
aucun scope implicite. Elle DOIT recevoir explicitement un contexte de création
et une provenance. Une conversion qui demande le contexte du site d’appel est
une capture intentionnelle régie par la section « Capture intentionnelle ».

La conversion de `Syntax` vers une donnée dépouillée de scopes PEUT exister
pour inspection. Réintroduire cette donnée dans une expansion NE DOIT PAS
restaurer silencieusement les bindings perdus.

### Catégories et invocation

Une macro déclare les catégories syntaxiques qu’elle accepte et produit, par
exemple expression, pattern, type, déclaration ou élément de protocole.

Le parseur DOIT reconnaître une invocation de macro sans exécuter la macro ni
demander au package de modifier le reader global. La ponctuation utilisée pour
cette reconnaissance reste définie par LANG-002.

Une macro d’expression NE PEUT PAS produire directement un module ou modifier
une déclaration voisine. Une macro de déclaration NE PEUT PAS réinterpréter le
texte déjà lu hors de son invocation.

Une invocation est résolue nominalement dans un espace de symboles de
métaprogrammation. Son sens NE DOIT PAS dépendre de l’ordre des imports.

### Macros structurelles

Une macro structurelle est une transformation :

```text
Syntax<InputKind, Expansion>
-> ExpansionResult<OutputKind>
```

La notation décrit son contrat, pas sa syntaxe finale. Une macro structurelle
dispose uniquement :

- de son entrée ;
- de ses paramètres constants explicites ;
- des bindings de métaprogrammation importés ;
- d’un constructeur hygiénique ;
- d’une API de diagnostic ;
- d’un budget d’expansion.

Elle NE DOIT PAS consulter les types attendus, unification, implémentations de
protocoles, disposition mémoire ou état mutable global du compilateur.

Robine DOIT fournir un sous-système déclaratif de patterns et templates pour
les transformations syntaxiques régulières. Une macro procédurale PEUT être
utilisée lorsque cette forme déclarative ne suffit pas ; elle reste soumise aux
mêmes règles d’hygiène, de pureté, de budget et de provenance.

Une expansion dont le résultat contient une invocation de macro est poursuivie
jusqu’à une forme sans invocation expansible ou jusqu’à l’échec d’une limite
déclarée. L’ordre d’expansion DOIT être déterministe pour un même arbre, les
mêmes interfaces et le même profil.

### Hygiène

L’hygiène est obligatoire par défaut. Chaque binding et chaque invocation de
macro introduisent un scope structurel distinct.

Un identifiant provenant d’un fragment utilisateur conserve les scopes de son
site d’usage. Un identifiant introduit par le template d’une macro est résolu
dans le contexte de définition approprié et ne capture pas accidentellement un
binding du site d’usage.

Deux noms textuellement identiques mais dotés de scopes incompatibles NE SONT
PAS le même binding.

Les bindings frais sont produits par l’expandeur. Une macro NE DEVRAIT PAS
fabriquer des noms supposés uniques par préfixe, suffixe aléatoire ou compteur
global.

Le renommage hygiénique NE DOIT PAS altérer le nom présenté à l’utilisateur
dans la forme source, les diagnostics ou la documentation, sauf lorsqu’un nom
généré doit être inspecté explicitement.

### Capture intentionnelle

Une macro qui veut introduire ou référencer un binding du site d’appel utilise
une opération explicite de capture. Cette opération :

- exige la capacité de métaprogrammation `Syntax.Capture` ;
- identifie le fragment ou le scope capturé ;
- enregistre la capture dans la provenance ;
- apparaît dans la signature publique de la macro ;
- PEUT être interdite par la politique du projet.

Une capture NE PEUT PAS viser un binding privé qui n’est pas visible
normalement depuis le site d’appel.

Une macro exportée utilisant la capture intentionnelle DOIT documenter le
binding créé ou attendu, sa durée de vie et les conflits possibles.

### Extension syntaxique et DSL

Une bibliothèque ordinaire NE PEUT PAS remplacer le lexer, ajouter un reader
macro global, changer la précédence globale ou redéfinir une forme du noyau.

Une extension syntaxique PEUT déclarer une grammaire locale si :

- son entrée et sa sortie appartiennent à des catégories versionnées ;
- sa région source est explicitement délimitée ;
- son activation est visible par un import de métaprogrammation ;
- ses conflits sont détectés avant expansion ;
- le formatter, le parseur incrémental et le service de langage disposent de
  sa description déclarative ;
- une forme canonique ou un rendu de repli est défini.

Une grammaire locale NE DOIT PAS modifier l’analyse d’un module qui ne l’importe
pas. Retirer l’import DOIT produire un diagnostic local et ne pas
réinterpréter silencieusement le reste du fichier.

Une extension qui exige un parseur arbitraire, un état global ou l’accès aux
internes du compilateur est un plugin de toolchain, pas une macro portable.

### Dérivations typées

Une dérivation reçoit une vue immuable et versionnée d’une déclaration après :

- résolution de ses noms ;
- normalisation de ses types publics ;
- calcul de ses paramètres et contraintes ;
- validation de ses effets, ownership et attributs accessibles.

Elle PEUT produire des déclarations, implémentations de protocoles, codecs,
tests, documentation structurée ou métadonnées autorisées par son contrat.

Une dérivation NE DOIT PAS :

- lire le corps privé d’une dépendance ;
- modifier la déclaration d’entrée ;
- fabriquer un type ou une preuve considérés valides sans vérification ;
- retirer un effet, une capacité, une obligation d’ownership ou un contrat ;
- ajouter silencieusement une dépendance de package.

Toute sortie de dérivation repasse par résolution, typage, vérification des
effets, ownership, architecture et domaines. Le noyau du compilateur NE DOIT
PAS faire confiance à une HIR déclarée « déjà typée » par la dérivation.

Les noms publics générés et leurs identités DOIVENT être déterministes. Une
dérivation qui produit une déclaration publique modifie l’artefact d’interface
selon ARCH-001.

### Elaborators locaux

Un elaborator traite une forme locale que l’expansion structurelle ne peut pas
exprimer correctement sans type attendu, contexte de binding ou obligation
sémantique.

Il reçoit uniquement une vue contrôlée de :

- la syntaxe de son invocation ;
- la catégorie et le type attendus lorsqu’ils existent ;
- les bindings visibles ;
- les interfaces publiques importées ;
- les contraintes et capacités explicitement accordées.

Il produit un terme ou une déclaration candidate que le vérificateur normal
contrôle. Il NE PEUT PAS écrire directement dans les tables internes de noms,
types, instances, effets ou preuves.

Un elaborator qui demande l’accès complet aux internes du compilateur devient
un plugin privilégié. Un tel plugin :

- NE FAIT PAS partie du langage portable ;
- DOIT être activé explicitement dans le manifeste de toolchain ;
- étend la base de confiance ;
- DOIT publier ses capacités, cibles et versions compatibles.

### Staging typé

Robine représente conceptuellement un fragment de code par :

```text
Code<ValueType, Effects, Ownership, Stage>
```

Cette notation est indépendante de la ponctuation de quotation et de splice.
Le type d’un fragment conserve au minimum son résultat, ses effets, ses
contraintes d’ownership et sa phase d’exécution.

Une quotation construit du code pour une phase ultérieure. Un splice ne peut
insérer qu’un fragment prévu pour la phase et la catégorie attendues.

Une valeur persistant entre phases DOIT être :

- constante et sérialisable selon un contrat stable ;
- un symbole ou type réifié par une API versionnée ;
- ou un paramètre explicitement généré dans la phase suivante.

Une référence vers pile, handle, capacité, acteur, secret ou ressource
d’exécution NE DOIT PAS persister implicitement vers une autre phase.

La composition de fragments NE DOIT PAS dupliquer, supprimer ou réordonner un
effet observable par rapport à la sémantique non étagée. Lorsqu’un splice
réutilise une expression avec effets, le compilateur DOIT introduire un binding
unique préservant l’ordre d’évaluation ou rejeter la transformation.

Un fragment construit avec un type correct est néanmoins revérifié après
composition. Une API de réflexion analytique sur la HIR typée PEUT être
proposée à une dérivation ou un outil, mais elle NE DOIT PAS permettre de
construire une preuve non vérifiée.

L’exécution de code généré au runtime exige une capacité explicite de
compilation dynamique et une cible qui l’autorise. Un artefact `native-closed`
de RUN-005 NE DOIT PAS embarquer compilateur, JIT ou moteur de staging runtime
s’il ne conserve pas cette capacité.

### Pureté, déterminisme et autorité

Une macro structurelle, une dérivation et un elaborator portable sont purs par
défaut. Ils NE DOIVENT PAS accéder directement :

- au filesystem ;
- au réseau ;
- à l’horloge ;
- au hasard non déterministe ;
- aux variables d’environnement ;
- aux processus ;
- aux secrets ;
- à l’état mutable d’une autre expansion.

Leurs entrées complètes DOIVENT être représentées dans la clé de cache :

- version et empreinte du métaprogramme ;
- syntaxe ou déclaration d’entrée ;
- interfaces publiques consultées ;
- paramètres constants ;
- profil de cible explicitement demandé ;
- version de l’API structurelle et du compilateur.

Les données externes, appels d’outils et découvertes de plateforme passent par
une tâche de build selon PKG-002. Cette tâche déclare capacités, entrées,
sorties, environnement et reproductibilité ; son résultat devient ensuite un
artefact ordinaire adressé par contenu.

Un métaprogramme NE DOIT PAS transformer une capacité de compilation en
capacité du programme généré. Les effets et capacités du résultat proviennent
du code généré et sont contrôlés normalement.

### Limites et terminaison

Chaque expansion possède des limites configurées ou profilées pour :

- temps ou travail logique ;
- mémoire ;
- profondeur et nombre d’expansions ;
- taille de sortie ;
- volume de diagnostics ;
- récursion entre métaprogrammes.

Une macro déclarative structurellement récursive PEUT être reconnue comme
terminante. Une macro procédurale générale peut épuiser son budget ; cet
épuisement est une erreur de compilation attribuée à la macro et à son
invocation, pas un blocage indéfini du service de langage.

Le service incrémental DOIT pouvoir annuler une expansion devenue obsolète. Une
annulation NE DOIT PAS publier un résultat partiel dans le graphe sémantique.

Une expansion qui reproduit cycliquement une invocation équivalente DOIT être
détectée avant la limite générale lorsque son identité structurelle permet
cette détection.

### Provenance

Chaque nœud généré conserve une chaîne de provenance comprenant :

- invocation ou déclaration source ;
- définition et version du métaprogramme ;
- phase ;
- fragments utilisateur conservés ;
- fragments introduits ;
- captures intentionnelles ;
- tâche de build d’origine lorsqu’un artefact externe intervient.

Une transformation ultérieure NE DOIT PAS écraser cette chaîne. Elle PEUT la
compacter sans perdre la possibilité d’expliquer l’origine d’un nœud visible.

La documentation d’une déclaration générée DOIT identifier sa dérivation et
permettre de revenir à la déclaration source. Les fichiers générés ne sont pas
présentés comme source canonique lorsqu’ils peuvent être reconstruits.

### Diagnostics et inspection

Le toolchain fournit au minimum les opérations sémantiques suivantes, dont les
noms CLI définitifs restent non normatifs :

```text
expand one
expand recursively
expand typed
trace expansion
explain generated node
diff expansion
```

Une erreur dans une expansion indique en priorité le fragment que l’utilisateur
peut modifier. Elle montre ensuite, selon pertinence :

- l’invocation ;
- la règle ou branche de macro ;
- le fragment généré fautif ;
- la définition du métaprogramme ;
- la pile d’expansion minimale ;
- le type, effet, scope ou phase attendu.

Les noms internes hygiéniques NE DEVRAIENT PAS être le message principal. Une
vue détaillée PEUT les afficher avec leurs scopes.

Le REPL, l’éditeur et l’IA utilisent la même représentation et les mêmes
opérations d’expansion. Une forme évaluée au REPL NE DOIT PAS recevoir une
sémantique de macro différente de celle d’un module.

### Outillage et forme source

Le formatter formate la forme utilisateur et conserve l’invocation lorsqu’elle
est la source canonique. Il NE DOIT PAS remplacer automatiquement une
invocation par toute son expansion.

Rename et navigation suivent les scopes avant et après expansion. Un rename
sur un binding utilisateur modifie les fragments source correspondants, pas les
noms frais internes. Un nœud purement généré renvoie vers la définition ou les
paramètres qui contrôlent sa génération.

Une extension de grammaire locale DOIT fournir suffisamment de structure pour :

- récupération après source incomplet ;
- coloration et navigation ;
- formatage canonique ;
- positions précises ;
- patches structurels AI-001.

Si cette structure est absente ou incompatible, l’extension est refusée dans
le profil d’outillage standard.

### Compilation incrémentale

Une expansion pure est adressée par le contenu de ses entrées complètes. Un
cache valide PEUT éviter son exécution ; l’absence ou la corruption d’un cache
NE DOIT PAS changer son résultat.

Modifier le corps d’une macro invalide ses invocations et leurs consommateurs
sémantiquement affectés. Modifier une dépendance non consultée NE DOIT PAS
invalider l’expansion.

Une dérivation qui ne change pas son artefact d’interface NE DOIT PAS forcer le
retypage des consommateurs externes. Le compilateur PEUT réoptimiser leurs
corps chauds selon DX-001.

Changer une macro pendant une session REPL produit de nouvelles expansions et
versions de définitions. Cela NE réexécute PAS automatiquement les effets du
programme vivant ; les règles de DX-003 s’appliquent aux états déjà installés.

### Artefacts et packages

Une interface publique de métaprogrammation publie :

- catégories d’entrée et de sortie ;
- phase ;
- paramètres constants ;
- usage éventuel de `Syntax.Capture` ;
- limites recommandées ;
- version d’API structurelle ;
- documentation de l’expansion observable.

Le code de métaprogrammation distribué appartient à la chaîne logicielle du
build même lorsqu’il ne figure pas dans l’artefact final. Sa source, son
empreinte, ses dépendances, sa licence et sa provenance suivent PKG-002.

Une macro NE PEUT PAS ajouter silencieusement un package au graphe de
dépendances. Une dérivation qui exige un runtime auxiliaire déclare cette
dépendance dans son contrat et le manifeste doit l’autoriser.

Un projet PEUT limiter ou interdire :

- macros procédurales ;
- captures intentionnelles ;
- extensions de grammaire ;
- plugins privilégiés ;
- temps cumulé de métaprogrammation ;
- dépendances de phase de compilation.

### Coût runtime et scellement

Les macros structurelles et dérivations sont absentes de l’exécution après
expansion, sauf si le programme généré les appelle aussi comme bibliothèques
d’exécution.

Une implémentation NE DOIT PAS revendiquer une macro « sans coût » lorsque son
expansion ajoute allocation, copie, dispatch, synchronisation, code ou données.
Le rapport de compilation relie ces coûts au fragment généré et à l’invocation.

Une release scellée élimine moteur d’expansion, API de réflexion, provenance de
développement et code de métaprogrammation lorsqu’ils ne sont requis par aucun
point d’entrée conservé. Les source maps minimales exigées pour crash reports,
audit ou obligations réglementaires PEUVENT rester dans un artefact séparé.

## Diagnostics et erreurs

Le compilateur DOIT distinguer au minimum :

- catégorie d’entrée ou de sortie incompatible ;
- identifiant hors scope ou capture non autorisée ;
- usage interphase interdit ;
- cycle ou dépassement de budget d’expansion ;
- effet de compilation interdit ;
- accès à une interface privée ;
- code généré mal typé ou non conforme aux effets et ownership ;
- duplication ou réordonnancement illégal d’un effet par staging ;
- conflit de grammaire locale ;
- dépendance ou capacité générée non déclarée ;
- incompatibilité de version de l’API structurelle.

Un diagnostic attribue séparément :

- l’erreur de l’utilisateur dans les arguments ;
- l’erreur de l’auteur du métaprogramme ;
- l’incompatibilité de toolchain ;
- le refus d’une politique de sécurité.

Une erreur provenant exclusivement d’un fragment introduit DOIT pointer
l’invocation comme emplacement primaire et la définition de macro comme cause.

## Sécurité, confidentialité et ressources

Les métaprogrammes de packages sont du code non fiable du build. Ils
s’exécutent dans une sandbox sans autorité ambiante et avec les limites de
ressources de cette spec.

Une valeur `Syntax` peut contenir du source ou des identifiants sensibles. Un
métaprogramme ne reçoit que son entrée et les interfaces nécessaires ; il NE
DOIT PAS pouvoir parcourir arbitrairement les modules privés du projet.

Les caches d’expansion sont validés par empreinte et considérés non fiables
jusqu’à vérification de leur format, version et provenance. Une expansion
chargée depuis un cache distant repasse par les mêmes vérificateurs que celle
calculée localement.

Une macro ne peut pas contourner :

- l’autorité nulle par défaut de PKG-002 ;
- les capacités de TYPE-004 ;
- les règles d’architecture ;
- les restrictions `realtime` ;
- les frontières `unsafe` ;
- les budgets d’artefact et de compilation.

Une expansion pathologique NE DOIT PAS rendre indisponible indéfiniment le
service de langage. Annulation, quotas et isolation du worker de compilation
DOIVENT permettre de reprendre les autres requêtes.

## Interactions

- LANG-002 définit les exigences de syntaxe et le modèle public `Syntax<T>` ;
- LANG-003 définit bindings, ordre d’évaluation, patterns et modules ;
- TYPE-004 vérifie effets et capacités du code généré ;
- TYPE-005 vérifie ownership et persistance interphase ;
- ARCH-001 versionne les déclarations publiques générées ;
- DX-001 met en cache et invalide les expansions ;
- DX-002 expose l’expansion dans le REPL ;
- DX-003 gouverne les versions déjà installées après réexpansion ;
- DX-004 définit diagnostics et explications structurées ;
- CPL-001 revérifie chaque transition d’IR ;
- RUN-005 retire le moteur de métaprogrammation d’une release scellée ;
- TOOL-001 fournit formatage, navigation et refactorings ;
- AI-001 consomme provenance et identités structurelles ;
- PKG-002 sandboxe tâches de build, plugins et dépendances ;
- STD-001 interdit les extensions globales du reader et des effets.

## Compatibilité et migration

Une modification du corps d’une macro est compatible uniquement si toutes ses
expansions observables conservent comportement, effets, diagnostics
contractuels et déclarations publiques. Changer une expansion peut être
source-breaking ou semantic-breaking même si la signature de la macro reste
identique.

Ajouter ou retirer `Syntax.Capture`, changer de phase, modifier une catégorie
syntaxique ou exiger une nouvelle capacité est une modification d’interface.

Une modification de dérivation qui change une déclaration publique suit
ARCH-001. Le toolchain DOIT pouvoir comparer les artefacts avant et après
réexpansion.

Une extension qui utilisait concaténation de texte, reader global, état
d’expansion ambiant ou I/O de compilation doit migrer respectivement vers :

- builders `Syntax` ;
- grammaire locale délimitée ;
- entrée constante explicite ;
- tâche de build sandboxée.

Les représentations `Syntax`, fragments `Code` et interfaces de dérivation sont
versionnées. Une incompatibilité NE DOIT PAS être contournée par
désérialisation permissive.

## Tests de conformité

La suite de conformité DOIT couvrir :

- introduction de bindings frais sans capture accidentelle ;
- conservation des bindings du site d’usage dans les fragments injectés ;
- deux noms textuellement identiques appartenant à des scopes distincts ;
- capture intentionnelle autorisée, auditée puis refusée par politique ;
- macro d’expression tentant de produire une déclaration ;
- rejet d’une concaténation textuelle comme sortie de macro ;
- expansion imbriquée déterministe ;
- détection d’une boucle d’expansion ;
- dépassements indépendants de temps, mémoire, profondeur et taille de sortie ;
- annulation d’une expansion incrémentale sans publication partielle ;
- rejet des accès filesystem, réseau, horloge, environnement et processus ;
- passage du même besoin externe par une tâche de build autorisée et hachée ;
- séparation des imports de compilation et d’exécution ;
- détection d’un cycle interphase ;
- persistance interphase d’une constante autorisée ;
- rejet d’un handle, secret, borrow ou capacité traversant une phase ;
- composition typée de fragments ;
- rejet ou let-insertion lorsqu’un splice dupliquerait un effet ;
- dérivation produisant une implémentation valide ;
- rejet d’une dérivation tentant de retirer un effet ou de forger une preuve ;
- revérification complète d’une HIR candidate mal typée ;
- grammaire locale confinée à sa région et son module importeur ;
- conflit de grammaire diagnostiqué sans réinterprétation silencieuse ;
- source incomplet sous une extension de grammaire ;
- formatage canonique et idempotent d’une invocation ;
- rename correct à travers expansion sans modifier les noms frais ;
- diagnostic ramené au fragment utilisateur et à la règle génératrice ;
- inspection et diff d’expansion dans CLI, éditeur et REPL ;
- invalidation après modification d’une macro consultée ;
- absence d’invalidation après modification d’une dépendance non consultée ;
- résultat identique avec cache froid, chaud, absent et reconstruit ;
- exclusion du moteur d’expansion et du code de macro d’un artefact scellé ;
- rapport des allocations, copies et effets introduits par une expansion ;
- compatibilité des expansions sémantiques entre les prototypes syntaxiques de
  LANG-002.

## Alternatives rejetées

La substitution textuelle est rejetée : elle ignore catégories syntaxiques,
bindings, types, provenance et outils.

La représentation du code par listes ou tokens nus est rejetée comme interface
canonique : elle ne porte pas suffisamment les scopes, phases, identités et
contrats nécessaires, même si une projection en données reste utile.

Les macros procédurales non hygiéniques par défaut sont rejetées : les
conventions de noms improbables ne constituent pas un système de portée.

Un unique mécanisme Turing-complet pour syntaxe, dérivation, I/O de build et
plugins est rejeté : il confond abstraction, autorité et phase.

L’accès direct aux internes du compilateur depuis un package ordinaire est
rejeté : il rend le langage, les caches et l’outillage dépendants d’une API
privée mouvante.

L’interdiction totale des macros est rejetée : certaines abstractions de
binding, contrôle et DSL ne peuvent pas être exprimées fidèlement par des
fonctions ordinaires.

## Questions ouvertes

- Syntaxe de surface des invocations, quotations, splices et grammaires locales.
- Sous-langage déclaratif minimal de patterns et templates.
- Présence de staging runtime dans le profil standard Robine 1.0.
- Format binaire stable de `Syntax` pour les caches distants.
- Ensemble minimal d’informations typées exposées aux elaborators sans coupler
  leur API à la HIR interne.
