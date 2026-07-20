# RT-002 — Communication bornée et échange de graphe audio

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `realtime`

## Objet

Permettre contrôle interactif, observation et hot reload d’un moteur audio sans
violer RT-001.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Canaux

Le standard fournit des primitives bornées adaptées à un producteur et un
consommateur, ainsi que des snapshots atomiques pour petites valeurs.

Une opération appelée depuis le thread audio :

- termine en nombre borné d’étapes ;
- n’alloue pas ;
- ne bloque pas ;
- indique immédiatement succès, saturation ou absence de donnée.

### Politiques de commandes

Chaque commande choisit une politique :

- `latest` pour paramètres continus ;
- `ordered(capacity)` pour événements musicaux ;
- `coalesce(key)` pour mises à jour remplaçables ;
- `reject` pour commandes qui ne peuvent être perdues.

Un producteur non temps réel peut attendre de la capacité ; le consommateur
temps réel ne le peut jamais.

### Télémétrie

Le thread audio écrit dans un ring buffer borné. La saturation incrémente un
compteur et perd l’événement selon la politique. L’observabilité NE DOIT PAS
modifier la deadline du DSP.

### Préparation d’un graphe

Un nouveau graphe est :

1. compilé hors du thread audio ;
2. typé et validé `realtime` ;
3. entièrement alloué ;
4. initialisé et éventuellement préchauffé ;
5. soumis comme ressource unique.

### Commit

L’activation se produit à une frontière de bloc par échange atomique. Le
callback en cours termine avec son graphe d’origine.

L’ancien graphe est placé dans une file de retrait et libéré par un thread non
temps réel après une époque de quiescence.

### Continuité sonore

Une mise à jour déclare :

- remplacement instantané ;
- crossfade borné ;
- migration d’état compatible ;
- réinitialisation avec silence ou bypass.

Un crossfade réserve à l’avance les ressources nécessaires aux deux graphes.
Le runtime refuse l’upgrade si le budget CPU ou mémoire ne permet pas leur
coexistence.

### Échec

Si compilation, préparation, migration ou admission échoue, le graphe actif
reste inchangé. Le commit est transactionnel.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- RT-001

## Compatibilité et migration

L’état compatible peut être copié ou transformé hors callback. Une migration
qui dépend de l’état changeant à chaque bloc utilise un snapshot versionné et
accepte explicitement sa tolérance.

## Tests de conformité

La suite de conformité DOIT couvrir au moins un cas valide et un cas de violation pour chaque exigence observable.

## Questions ouvertes

Aucune à ce stade.
