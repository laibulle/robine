# RUN-005 — Runtime synthétisé et préemption sélective

- Statut : **Draft**
- Version : **0.2.0**
- Domaine : `runtime`

## Objet

Définir comment Robine fournit isolation d’acteurs, équité, supervision et
reprise sans imposer une machine virtuelle, un collecteur global ou des points
de préemption au code qui ne demande pas ces garanties. La présente spec
définit la fermeture de services calculée pour chaque artefact, l’abaissement
sélectif du code préemptible, les limites de l’isolation native et le contrat de
coût observable.

## Non-objectifs

- définir la syntaxe source des annotations de domaine ;
- promettre une absence absolue de travail à l’exécution ;
- fournir une garantie temps réel dure au domaine `responsive` ;
- isoler dans un même processus les fautes du compilateur, du runtime, du
  matériel ou du code étranger `unsafe` ;
- imposer un algorithme particulier de file de prêts ou de work stealing ;
- remplacer les règles fonctionnelles des acteurs, tâches, kernels ou mises à
  jour vivantes définies par leurs specs propriétaires.

## Spécification normative

### Terminologie

Un **service d’exécution** est du code ou un état nécessaire pendant
l’exécution, par exemple une file de prêts, un timer, une mailbox, un
allocateur de région ou une table de versions.

La **fermeture de runtime** d’un artefact est l’ensemble transitif minimal de
services d’exécution requis par :

- ses domaines et effets conservés ;
- ses points d’entrée ;
- ses frontières dynamiques explicitement ouvertes ;
- ses capacités de plateforme ;
- son profil de développement ou de release.

Un **runtime synthétisé** est la matérialisation compilée et spécialisée de
cette fermeture. Ce terme NE SIGNIFIE PAS qu’aucun code ne s’exécute pour
ordonner les acteurs ou transporter les messages.

Une **région atomique responsive** est une portion de calcul qui ne peut pas
rendre la main au scheduler responsive. Son coût maximal admissible appartient
au profil d’admission de RUN-004.

### Propriété d’exécution préemptible

La préemptibilité est une propriété d’exécution locale. Elle est demandée
explicitement par un domaine `responsive`, un point d’entrée acteur exigeant
l’équité, ou une signature qui la conserve. Elle PEUT être inférée, mais elle
DOIT alors apparaître dans la HIR, l’artefact d’interface et les diagnostics de
la même manière qu’une propriété explicitement écrite.

La propriété préemptible se propage à travers :

- les appels directs ;
- les paramètres fonctionnels ;
- les implémentations de protocoles ;
- les callbacks ;
- les appels indirects et le dispatch dynamique.

Une abstraction NE DOIT PAS effacer cette propriété d’un appel possible.

Lorsque cette propagation produit une variante d’une fonction dont le domaine
contractuel reste `normal`, la variante et sa relation à la définition
d’origine DOIVENT être enregistrées selon RUN-004. Le contexte d’appel NE DOIT
PAS réécrire silencieusement le domaine de l’interface publique.

Une fonction `normal` qui n’est atteignable depuis aucun contexte préemptible
NE DOIT PAS recevoir de poll de scheduler, de budget caché, de continuation ou
de convention d’appel préemptible uniquement parce qu’un autre composant du
programme utilise des acteurs.

### Abaissement en calcul reprenable

Le compilateur abaisse le contrôle d’une fonction préemptible en calcul
reprenable. Le modèle dynamique produit l’un des états logiques suivants :

```text
terminé(valeur)
en_attente(ressource, continuation)
budget_épuisé(continuation)
faute(erreur structurée)
```

Cette représentation est une description d’IR non normative pour la syntaxe
source et NE DOIT PAS imposer un objet universel ou un appel virtuel lorsque le
backend peut spécialiser les états.

Les valeurs vivantes nécessaires après un rendement DOIVENT être conservées
dans une continuation dont l’ownership est vérifié. Une référence empruntée
vers une pile étrangère, une région déplaçable ou une ressource non épinglée NE
DOIT PAS traverser un point de rendement.

Un point de rendement PEUT être placé :

- sur le retour d’une boucle non bornée ;
- avant ou après un appel dont le coût est variable ;
- lors d’une allocation ou d’une transition de région ;
- à une frontière de message ;
- à une opération cancellable ou suspendable.

Un point de rendement réel NE DOIT être pris qu’à un état où les invariants de
mémoire, de destructeurs, de verrous et de ressources sont restaurés. Le
compilateur PEUT différer un poll jusqu’au prochain état sûr, à condition que
la région atomique ainsi créée respecte la borne admise.

### Budgets et élision des polls

Un backend préemptible DOIT publier son unité de budget et la relation entre
cette unité, le poids d’un acteur et les régions atomiques. Le budget PEUT être
exprimé en travail logique, en échantillons de temps CPU ou par un modèle
hybride ; il NE DOIT PAS être présenté comme une durée murale garantie.

Une séquence dont le coût maximal est prouvé PEUT être facturée en une fois.
Ses polls internes PEUVENT alors être supprimés si son coût maximal respecte la
borne atomique admise.

Une boucle vectorisée, un appel monomorphisé ou une fonction leaf bornée NE
DOIT PAS perdre ses optimisations uniquement pour conserver des polls devenus
inutiles après preuve.

La suppression d’un poll NE DOIT PAS modifier :

- le résultat fonctionnel ;
- l’ordre observable des effets ;
- la propagation d’une annulation déjà observable ;
- les garanties d’équité déclarées pour le profil admis.

### Appels depuis un contexte préemptible

Un contexte préemptible PEUT appeler directement une fonction non préemptible
seulement si le compilateur démontre que son coût maximal tient dans la région
atomique admise.

À défaut de cette preuve, l’appel DOIT être :

- abaissé lui-même en calcul reprenable ;
- soumis comme kernel ou travail asynchrone sur un exécuteur séparé ;
- déplacé vers un worker `isolated` ;
- ou rejeté.

Une annotation qui renonce à l’équité PEUT autoriser un appel atomique non
borné, mais elle DOIT :

- être visible dans le contrat public ;
- invalider la garantie d’équité pour la chaîne d’appel concernée ;
- produire une surface de coût dans le rapport de build ;
- être interdite par un profil qui exige une équité transitive.

Une FFI `blocking` ou `unsafe` ne devient jamais préemptible par annotation.
Elle suit FFI-001. Une bibliothèque étrangère NE DOIT PAS être interrompue par
un signal système arbitraire pour simuler un rendement sûr.

### Kernels et opérations non préemptibles

Un kernel CPU, vectoriel ou NPU est un travail distinct du scheduler
responsive. Sa soumission DOIT rendre la main sans attendre sa terminaison.
L’acteur demandeur devient `en_attente` et n’occupe pas un worker responsive.

Lorsqu’un matériel ne permet pas d’interrompre un kernel déjà soumis,
l’annulation signifie au minimum :

- ne plus attendre son résultat ;
- libérer les ressources annulables ;
- conserver les buffers jusqu’à ce que le matériel ne puisse plus les
  utiliser ;
- comptabiliser le travail résiduel dans la télémétrie.

Avant que la tâche demandeuse publie `Cancelled`, l’exécuteur de kernels DOIT
recevoir l’ownership des buffers encore utilisés, du signal de terminaison et
de la comptabilité résiduelle. Le kernel NE DOIT plus pouvoir compléter la
tâche ni rappeler son scope. La fin logique de RUN-002 est alors distincte de
la fin physique du travail matériel.

Un exécuteur de kernels DOIT posséder une admission et une saturation
indépendantes afin qu’une file de calcul ne bloque pas la file des acteurs.

### Mémoire locale d’acteur

Un acteur natif possède au minimum :

- son état privé ;
- sa continuation éventuelle ;
- sa mailbox ;
- son budget et ses compteurs ;
- ses liens de supervision nécessaires.

Le compilateur et RUN-001 DOIVENT attribuer chaque allocation d’un handler à
l’état persistant, à une continuation, à un message transféré ou à une région
temporaire récupérable. Une arène temporaire PEUT être réinitialisée à la fin
du message lorsque aucune valeur vivante ne la référence.

Le domaine `responsive` NE DOIT PAS exiger un collecteur global. Un graphe
cyclique explicitement traçable PEUT lier un collecteur local à son acteur ou à
sa région ; son coût et ses points de travail DOIVENT participer au budget.

Un buffer exclusivement possédé envoyé entre deux acteurs natifs du même
processus DOIT pouvoir être déplacé sans copie de son contenu lorsque la
représentation et l’alignement de destination sont compatibles. Une copie
requise par une frontière de processus, de cible ou de représentation DOIT
être visible dans le plan d’exécution et le profiler.

Une valeur immuable partagée PEUT utiliser une représentation partagée. Le coût
de rétention, de comptage de références ou de récupération NE DOIT PAS devenir
une opération cachée non bornée dans `realtime`.

### Mailboxes spécialisées

Les règles fonctionnelles des mailboxes sont celles de RUN-003. Leur
représentation PEUT être spécialisée selon :

- le protocole de messages ;
- la capacité maximale ;
- le nombre connu de producteurs ;
- la localité des acteurs ;
- les politiques de saturation ;
- les tailles et multiplicités des valeurs.

Une mailbox à producteur unique PEUT être abaissée vers une file SPSC ; une
mailbox à producteurs multiples PEUT utiliser une file MPSC. Cette
spécialisation NE DOIT PAS changer l’ordre ou la politique de saturation
observables.

Un protocole statiquement fermé NE DEVRAIT PAS employer une représentation
universelle étiquetée ou une allocation par message lorsque ses variantes
peuvent être représentées directement. Toute représentation universelle
conservée pour réflexion, plugin ou distribution DOIT être limitée à cette
frontière.

### Faute, isolation et supervision

L’entrée d’un handler d’acteur DOIT établir une frontière de faute. Une faute
Robine récupérable ou une violation de contrat configurée comme faute d’acteur
DOIT :

1. arrêter le handler concerné ;
2. rendre l’état partiellement modifié inobservable ;
3. libérer ou transférer ses ressources selon leur politique ;
4. notifier liens, moniteurs et superviseur ;
5. appliquer la stratégie de RUN-003.

Le compilateur PEUT réaliser cette frontière par valeurs de retour, tables de
landing pads ou trampoline spécialisé. Il NE DOIT PAS exiger une exception
dynamique universelle.

Une limite mémoire par acteur n’isole une pénurie que si l’allocateur de la
cible peut refuser localement l’allocation. Une pénurie du système, une faute
matérielle, une corruption issue de `unsafe`, une faute du runtime ou un abort
étranger restent hors de la garantie d’isolation en processus.

Un profil exigeant l’isolation contre ces fautes DOIT employer un processus OS,
une sandbox mémoire ou un worker `isolated`. Le niveau d’isolation choisi DOIT
apparaître dans l’artefact de déploiement.

### Synthèse de la fermeture de runtime

Avant la génération finale, le compilateur DOIT calculer la fermeture de
runtime depuis le graphe vérifié du programme. Au minimum, les familles
suivantes sont distinguées :

```text
tasks
actors
responsive-scheduler
actor-memory
timers
hot-reload
realtime-bridge
compute
distribution
foreign-workers
observability
```

Une entrée dynamique, un plugin ou une bibliothèque chargée tardivement DOIT
déclarer la fermeture maximale qu’il peut requérir. Un build scellé NE DOIT PAS
accepter une extension susceptible d’introduire après coup un service absent.

Chaque service retenu DOIT posséder une raison traçable vers un point d’entrée,
un effet, un domaine ou une frontière dynamique. Le rapport de build DOIT
expliquer ces chaînes de rétention.

Un programme sans acteur, tâche responsive ni point d’entrée responsive NE
DOIT PAS lier le scheduler d’acteurs, les mailboxes ou la supervision. Le fait
qu’une dépendance inutilisée contienne de tels éléments ne suffit pas à les
retenir dans un build scellé.

La fermeture est spécialisée après résolution des features, monomorphisation
et composition connues. Les services identiques PEUVENT être fusionnés ; les
branches, métadonnées et variantes inatteignables DEVRAIENT être éliminées.

### Profil natif fermé

Un artefact conforme au profil `native-closed` :

- DOIT contenir le code machine applicatif et sa fermeture de runtime ;
- NE DOIT PAS exiger l’installation d’une VM Robine ;
- NE DOIT PAS dépendre d’un interpréteur général de bytecode Robine ;
- PEUT dépendre du système d’exploitation, des frameworks de plateforme et des
  bibliothèques dynamiques déclarées ;
- DOIT publier ces dépendances et leurs versions minimales.

Le profil `native-closed` n’interdit pas un compilateur incrémental, un JIT de
développement ou un REPL dans un autre profil. Il interdit de les faire payer à
une release scellée qui ne les conserve pas.

### Développement, reload et scellement

Une image de développement PEUT conserver tables de versions, identités
structurelles, instrumentation et points de remplacement selon DX-003.

Une release scellée NE DOIT PAS conserver une indirection de reload uniquement
parce que la compilation de développement l’utilisait. Une frontière déclarée
rechargeable en production conserve une indirection versionnée à cette
frontière ; les appels internes qui ne la traversent pas DEVRAIENT rester
directs et spécialisables.

Les versions immédiate, chaude et scellée DOIVENT être observationnellement
équivalentes selon leurs contrats publics. Les effets et garanties déclarées
restent identiques. Les résultats sont égaux lorsque le contrat exige une
égalité exacte ; un contrat numérique de COMP-004 utilise sa relation de
conformité `Accepts_C`. Les versions PEUVENT différer par leurs coûts,
métadonnées et capacités de remplacement.

### Contrat de coût

Le compilateur DOIT produire, pour chaque artefact, un rapport de surface de
coût contenant au minimum :

- services retenus dans la fermeture de runtime ;
- fonctions abaissées en calcul reprenable ;
- polls conservés et polls éliminés ;
- continuations susceptibles d’être allouées ;
- copies et transferts aux frontières de messages ;
- appels indirects conservés ;
- travaux déplacés vers kernel ou worker ;
- limites dont dépend une garantie d’équité.

Le code `normal` NE DOIT PAS payer un poll, une continuation, un enregistrement
au scheduler ou un en-tête acteur du seul fait qu’il partage le binaire avec du
code `responsive`.

Les frontières vers `responsive`, `isolated`, `kernel`, `ui` ou un processus
distant ont un coût réel. Une implémentation NE DOIT PAS qualifier ce coût de
« nul » lorsqu’une queue, une copie, une allocation, une synchronisation ou un
changement d’exécuteur subsiste.

### Portée des garanties d’équité

L’équité responsive est garantie uniquement pendant les intervalles où :

- l’hôte accorde du temps CPU au processus ;
- les ressources ont été admises selon RUN-004 ;
- le code atteint les points sûrs dans les bornes déclarées ;
- aucun appel hors garantie ne bloque le worker ;
- le scheduler et l’allocateur restent fonctionnels.

Une suspension imposée par iOS, Android, un hôte serverless ou le système
d’exploitation ne constitue pas une violation de l’équité entre acteurs
pendant l’exécution. L’artefact DOIT toutefois propager la suspension, les
deadlines expirées et la reprise selon son profil de cycle de vie.

Cette garantie est soft et relative aux acteurs exécutables. Elle NE DOIT PAS
être présentée comme une deadline murale, une garantie hard real-time ou une
promesse de disponibilité distribuée.

## Diagnostics et erreurs

Un rejet causé par la préemption DOIT montrer :

- le point d’entrée responsive ;
- la chaîne d’appels concernée ;
- la première région non bornée, non reprenable ou bloquante ;
- les solutions admissibles : preuve de borne, abaissement préemptible,
  offload, isolation ou renoncement explicite.

Un borrow qui traverse un rendement interdit DOIT identifier son origine, le
point de rendement et la ressource dont la durée de vie ne peut être garantie.

Une cible incapable d’implémenter une garantie DOIT rejeter le profil ou
demander une dégradation explicite. Elle NE DOIT PAS compiler silencieusement
`responsive` comme du code run-to-completion non borné.

Le rapport de fermeture DOIT permettre d’expliquer pourquoi un scheduler, un
collecteur local, une table de versions ou un worker étranger est présent dans
l’artefact.

## Sécurité, confidentialité et ressources

Les files de prêts, mailboxes, continuations, régions d’acteur, timers et
travaux externes DOIVENT avoir une limite configurée ou dérivée du profil
d’admission. Une saturation applique une politique explicite ; elle NE DOIT PAS
provoquer une croissance mémoire silencieusement non bornée.

Les continuations et messages conservent les capacités de leurs valeurs. Un
rendement, un déplacement d’acteur ou une reprise NE DOIT PAS élargir
l’autorité d’une capacité.

Le scheduler DEVRAIT isoler les budgets par tenant lorsque l’identité de tenant
fait partie du profil. La télémétrie DOIT permettre d’attribuer CPU, mémoire,
attente, rejets, copies et travail résiduel de kernels au propriétaire
responsable.

La spécialisation et l’élision de contrôles NE DOIVENT PAS supprimer une
validation de frontière nécessaire à la sécurité, sauf si son obligation a été
prouvée et enregistrée dans l’artefact de preuve.

## Interactions

- RUN-001 définit régions, arènes, collecte locale et destruction ;
- RUN-002 définit tâches, annulation, scopes et deadlines ;
- RUN-003 définit acteurs, mailboxes, équité et supervision ;
- RUN-004 définit domaines, points sûrs et admission ;
- TYPE-004 définit la propagation des effets et capacités ;
- TYPE-005 définit ownership, déplacements et borrows ;
- CPL-001 définit l’IR de domaines, le scellement et les backends ;
- DX-003 définit les frontières de reload et la migration ;
- RT-001 interdit la préemption et les coûts non bornés en `realtime` ;
- COMP-001 définit la séparation entre contrôle CPU et kernels hétérogènes ;
- FFI-001 définit les frontières bloquantes et `unsafe`.

## Compatibilité et migration

La version 0.2.0 aligne les variantes préemptibles sur les domaines explicites
de RUN-004, définit le transfert de travail résiduel de RUN-002 et paramètre
l’équivalence des résultats par COMP-004. Les artefacts de continuation et
protocoles de tâches antérieurs doivent ajouter issue terminale et ownership du
travail résiduel ; ce changement est ABI-breaking.

La version 0.1.0 avait introduit le profil `native-closed`, la fermeture de
runtime et la propriété d’exécution préemptible. Modifier la représentation
interne d’une
continuation ou d’une mailbox dans un artefact scellé est compatible lorsque
ces représentations ne traversent aucune frontière publique.

Une modification de l’unité de budget, de la borne atomique ou du comportement
de rendement est semantic-breaking pour un profil qui les expose et DOIT
incrémenter sa version. Une continuation conservée par un reload ou un plugin
utilise une ABI versionnée et une migration selon DX-003.

Les artefacts d’interface antérieurs qui n’exposent pas la préemptibilité NE
DOIVENT PAS être supposés préemptibles. Leur import exige une analyse
compatible, un adaptateur borné ou un diagnostic.

## Tests de conformité

La suite de conformité DOIT inclure :

- un programme purement `normal` dont l’IR et le binaire ne contiennent ni
  poll, ni continuation, ni scheduler d’acteurs ;
- un binaire mixte où une boucle `normal` conserve le même abaissement qu’en
  l’absence du composant responsive ;
- deux acteurs préemptibles dont l’un boucle sans affamer l’autre ;
- une boucle bornée dont les polls internes sont éliminés après facturation
  agrégée ;
- le rejet d’un appel non borné `normal` depuis un handler préemptible ;
- l’acceptation du même travail après abaissement reprenable ou offload ;
- le rejet d’une FFI bloquante appelée directement depuis `responsive` ;
- le rejet d’un borrow non admissible traversant un rendement ;
- la conservation d’un état d’acteur et d’une continuation après rendement ;
- une faute d’acteur qui déclenche exactement la stratégie de supervision
  déclarée sans exposer un état partiellement modifié ;
- un déplacement de buffer possédé entre acteurs natifs sans copie de son
  contenu ;
- chaque politique de saturation de RUN-003 avec représentation spécialisée ;
- une annulation de kernel non interruptible qui conserve ses buffers jusqu’à
  la fin matérielle, transfère leur ownership hors du scope et ignore son
  résultat ;
- l’absence de callback tardif vers un scope dont la tâche est `Cancelled` ;
- l’absence de services hot reload dans une release scellée non rechargeable ;
- la conservation d’une seule frontière versionnée dans une release
  partiellement rechargeable ;
- le rejet d’un plugin dont la fermeture maximale n’était pas déclarée ;
- l’explication, par le rapport de build, de chaque service retenu ;
- des tests différentiels entre compilation immédiate, chaude et scellée ;
- des tests de famine, OOM local, surcharge de mailbox, suspension d’hôte et
  reprise ;
- un test démontrant qu’une FFI `unsafe` n’est pas présentée comme isolée par
  la seule frontière acteur.

## Alternatives rejetées

Une VM Robine obligatoire est rejetée pour le profil natif fermé : elle
imposerait représentation universelle, distribution séparée et services non
utilisés à des programmes qui n’en ont pas besoin.

Des polls dans toutes les fonctions sont rejetés : ils feraient payer
l’équité au DSP, aux kernels et au code natif ordinaire.

La préemption arbitraire par signaux OS est rejetée : elle peut interrompre le
programme lorsque ownership, verrous, ABI étrangère et invariants ne sont pas
dans un état reprenable.

Un runtime déclaré « inexistant » est rejeté comme description : une mailbox,
un scheduler actif et une supervision consomment nécessairement code, mémoire
et temps CPU.

## Questions ouvertes

- Unité portable minimale du budget logique entre backends.
- ABI des continuations conservées à travers une mise à jour de production.
- Format machine-readable commun au rapport de fermeture et au rapport de
  surface de coût.
- Seuil à partir duquel une preuve de coût borné doit être complétée par une
  mesure sur profil matériel.
