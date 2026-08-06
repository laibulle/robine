# 0001 — Interface cocooning autour de Robine

## Intention

Robine doit faire ressentir que la maison est comprise et paisible. L'interface est chaleureuse, calme et immédiatement utile, sans ressembler à un tableau de bord industriel ni à un jouet.

Robine, la husky, est la présence affective du produit : un repère visuel discret qui accompagne l'utilisateur et donne un ton humain à la maison. Elle ne remplace jamais une information importante, un état de sécurité ou une confirmation technique.

## Principes directeurs

- **La maison avant le système** : présenter une pièce, une ambiance, un moment ou une intention avant une liste d'entités et de protocoles.
- **La sérénité avant la densité** : peu de surfaces, des espaces généreux, un ordre visuel net et aucun mur de cartes.
- **La puissance à la demande** : contrôle quotidien simple ; historique, trace Flow, capacités, diagnostics et détails protocolaires accessibles sans encombrer l'accueil.
- **Une parole honnête** : distinguer clairement une commande demandée, envoyée, confirmée, expirée ou échouée.
- **La maison raconte ce qui se passe** : formuler les événements en phrases courtes et causales, par exemple « Les lumières se sont adoucies pour la soirée », avec un accès au détail exact.
- **Le calme reste sous contrôle** : aucune couleur douce, illustration ou animation ne peut masquer une indisponibilité, un refus de commande ou une action à risque.

## Vocabulaire produit

| Terme interne | Libellé principal | Détail expert |
|---|---|---|
| `Device` | appareil | marque, modèle, intégration |
| `Entity` | élément de la maison | identifiant et capacité |
| `Automation` / Flow | habitude | Robine Flow, déclencheurs et trace |
| `Adapter` | connexion | bridge, protocole, diagnostic |
| `unavailable` | ne répond pas pour le moment | état de disponibilité et dernière vue |

Le vocabulaire expert reste disponible dans les vues de détail macOS et la console de secours. Il n'est jamais supprimé ou reformulé au point de rendre un diagnostic ambigu.

## Hiérarchie de l'accueil

L'accueil quotidien iOS et macOS suit cet ordre :

1. **Moment présent** : salutation, heure et ambiance courte (« La maison est douce »).
2. **Intention active** : une habitude ou un état significatif, avec un seul accès à son explication.
3. **Pièces proches** : vues par pièce, avec le résumé utile et une commande immédiate.
4. **Ce qui mérite l'attention** : indisponibilité, action expirée ou automatisation en attente, visible sans dramatisation.

Les favoris sont choisis par l'utilisateur et non par l'intégration qui a produit le plus d'événements. Une vue « toute la maison » et les recherches restent accessibles, mais ne constituent pas l'écran par défaut.

## Robine, la husky

La husky apparaît comme une petite présence graphique — portrait, silhouette ou empreinte — dans l'en-tête, les états vides bienveillants et les explications d'habitude. Sa taille et ses animations sont secondaires au contenu.

Ses états sont limités et sémantiques :

| État | Usage |
|---|---|
| `paisible` | tout va bien, aucune attention nécessaire |
| `attentive` | une information nouvelle est disponible |
| `préoccupée` | une anomalie non critique demande une action |
| `endormie` | aucune activité récente, jamais une panne |

Les états critiques utilisent l'iconographie et les messages d'alerte standard de la plateforme, pas le visage de Robine. La husky ne prétend pas ressentir des émotions ou surveiller physiquement le foyer ; elle représente l'interface, pas un système de sécurité.

## Couleur, matière et typographie

La palette est douce et naturelle : fond lin ou vert nuit, surfaces crème ou mousse, accent miel/argile pour la chaleur, vert sauge pour les états sains. L'accent chaud n'est pas réservé aux actions destructrices ; celles-ci utilisent le traitement d'alerte natif et un libellé explicite.

Les noms de jetons de design sont sémantiques : `surface-home`, `surface-quiet`, `accent-warm`, `state-healthy`, `state-attention`, `state-critical`, `text-primary` et `text-secondary`. Chaque jeton a une variante claire et sombre testée en contraste ; aucun écran ne repose sur des couleurs codées en dur.

Sur Apple, l'interface utilise la typographie système et les contrôles SwiftUI. Les tailles dynamiques, le contraste renforcé, la réduction de transparence et la réduction des animations sont pris en charge. La console Leptos reprend cette hiérarchie, sans imiter artificiellement macOS.

## Surfaces et composants

- Les cartes représentent une **pièce**, une **intention**, une **action en cours** ou une **information nécessitant l'attention** — jamais une métrique isolée sans contexte.
- Une carte de pièce affiche au plus un résumé d'ambiance, le nombre d'éléments actifs et les commandes les plus naturelles. Les détails s'ouvrent en navigation, pas dans une grille dense.
- Les contrôles immédiats utilisent des gestes et contrôles natifs : interrupteur, curseur de luminosité, sélecteur de température. Leur état rapporté est toujours distinct de l'état local en attente.
- Les actions longues affichent une progression compréhensible et peuvent être annulées lorsqu'elles sont annulables.
- Les états vides proposent une action réelle (« Ajouter un bridge Hue », « Créer une habitude »), accompagnée de Robine avec retenue.

## Habitudes et explications

Les automatisations sont nommées **habitudes** dans les parcours courants. Une habitude se lit comme une phrase : « Quand il fait sombre dans l'entrée, éclairer doucement ». Son écran montre le déclencheur, la décision et le résultat avant les blocs Flow.

L'éditeur visuel Flow est un atelier : il utilise les mêmes couleurs et la même lisibilité, mais ne sacrifie jamais le typage, les unités ou les diagnostics pour être décoratif. Sur macOS, l'éditeur texte est disponible comme vue experte du même AST ; il ne crée pas une seconde source de vérité.

## Mouvements et retours

Les animations sont lentes, courtes et fonctionnelles : arrivée discrète d'une mise à jour, transition d'intensité d'une lumière, apparition d'une explication. Elles ne doivent ni s'exécuter en boucle, ni retarder une commande, ni simuler l'état physique d'un appareil avant sa confirmation.

Le retour haptique iOS et les signaux sonores éventuels sont réservés aux commandes effectivement confirmées et aux alertes configurées. Tous peuvent être désactivés par les préférences système ou Robine.

## Variantes par surface

| Surface | Rôle UX |
|---|---|
| iPhone | compagnon immédiat : pièces, favoris, contrôle et explication courte |
| iPad | vue maison plus ample, navigation et détail côte à côte |
| macOS | atelier de la maison : habitudes, historique, diagnostics et plusieurs fenêtres |
| Web Leptos | secours : amorçage, appairage, diagnostic et récupération ; aucune ambition de dashboard complet |

## Critères d'acceptation

- Une personne comprend l'état d'une pièce et peut agir sans connaître une entité, un protocole ou un identifiant technique.
- Une commande en attente et une commande confirmée sont visuellement et textuellement distinguables, y compris sans couleur.
- Une indisponibilité est visible depuis l'accueil mais n'empêche pas le reste de la maison de rester calme et utilisable.
- Les états de Robine la husky ne sont jamais le seul vecteur d'une information fonctionnelle ou critique.
- Les parcours courants sont utilisables avec Dynamic Type, VoiceOver, clavier macOS et réduction des animations.
- Les écrans d'habitude expliquent un résultat sans obliger l'utilisateur à lire du Flow ; l'utilisateur expert peut néanmoins atteindre la trace et l'AST exacts.
