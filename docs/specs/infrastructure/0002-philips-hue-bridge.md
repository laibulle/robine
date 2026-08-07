# 0002 — Intégration du bridge Philips Hue

## Objectif

Faire du bridge Philips Hue local la première intégration de production de Robine. Elle valide de bout en bout le registre d'appareils, l'état, les commandes, les événements poussés, les automatisations Flow et la console Leptos avec du matériel réel.

Robine dialogue avec le bridge sur le réseau local. Il ne communique pas directement avec le réseau Zigbee et n'utilise pas le cloud Hue pour le contrôle normal.

## Frontière d'architecture

Le crate `robine-integration-hue` est un adaptateur d'infrastructure. Il encapsule l'API Hue, HTTPS, découverte, appairage, événementiel, limitation de débit et représentation des ressources Hue. Aucun type Hue, identifiant de ressource Hue ou erreur HTTP Hue ne traverse `robine-application`.

Le bridge est un `AdapterId`. Chaque ressource Hue est identifiée en interne par le couple stable `(bridge_id, hue_resource_id)`. Les `DeviceId` et `EntityId` Robine restent opaques et sont conservés lors d'une redécouverte du bridge.

## Portée fonctionnelle V1

| Ressource Hue | Représentation Robine | Support V1 |
|---|---|---|
| bridge | adaptateur et état de santé | oui |
| lumière | appareil et entité `light` | oui |
| état marche/arrêt | capacité `switch` | oui |
| luminosité | capacité `light.brightness` en pourcentage | oui |
| température de couleur | capacité `light.color_temperature` (mirek sérialisé) | oui si supportée |
| couleur | capacité `light.color` normalisée | oui si supportée |
| pièce/zone Hue | suggestion de regroupement | import explicite uniquement |
| capteur/accessoire | appareil et entités de capteur | découverte et lecture si exposés par le bridge |
| scène Hue | hors périmètre V1 | non |
| entertainment/streaming rapide | hors périmètre V1 | non |

Une pièce ou zone Hue ne remplace jamais silencieusement une `Area` Robine. L'interface propose son import ou son association ; l'organisation de Robine appartient à l'utilisateur.

## Découverte et appairage

1. Robine cherche les bridges sur le réseau local via mDNS.
2. L'utilisateur peut saisir une adresse IP manuellement si la découverte échoue.
3. Robine établit une connexion HTTPS avec le bridge et affiche son identité.
4. L'utilisateur appuie physiquement sur le bouton du bridge.
5. Robine demande une clé d'application locale Hue, l'enregistre dans le magasin de secrets et crée la configuration non secrète de l'adaptateur.
6. L'adaptateur récupère l'inventaire, enregistre les appareils/entités, puis ouvre le flux d'événements.

La découverte Internet Hue est désactivée par défaut afin de préserver le fonctionnement local-first. Une éventuelle activation future exige un consentement explicite.

La clé d'application n'est jamais envoyée au navigateur, incluse dans une sauvegarde par défaut, écrite dans SQLite, ni imprimée dans les logs. Le certificat du bridge est vérifié et son empreinte de confiance est conservée ; un changement d'identité ou de certificat exige une confirmation d'administration avant toute nouvelle commande.

## Synchronisation d'état

Au démarrage et après une reconnexion, l'adaptateur récupère un inventaire complet des ressources supportées, le transforme en commandes de découverte/état canoniques puis s'abonne au flux de changements local fourni par l'API Hue actuelle.

Chaque événement reçu est validé, normalisé et soumis au cas d'utilisation `ApplyReportedState`. Les champs partiels ne remplacent que les propriétés correspondantes. Un événement Hue dupliqué ou en retard ne peut pas faire régresser la `StateVersion` Robine.

Si le flux d'événements est interrompu, l'adaptateur devient `degraded`, applique un backoff avec gigue, puis effectue une resynchronisation complète avant de redevenir `available`. Le polling n'est qu'une solution de dégradation bornée ; il ne constitue pas le mode normal.

## Commandes et confirmation

Les commandes Robine sont traduites uniquement si la capacité de l'entité le permet. Les conversions d'unités, de luminosité et de colorimétrie sont confinées à l'adaptateur.

Une commande possède l'identifiant de corrélation Robine. Une réponse HTTP Hue réussie confirme le transport ; une confirmation `:reported` est obtenue seulement lorsque le flux d'état reflète la valeur demandée. Le délai et l'échec sont alors visibles dans `commands` et dans la trace Flow.

L'adaptateur coalesce les mises à jour concurrentes portant sur la même propriété d'une lumière et limite les commandes à un rythme conservateur configuré par bridge. Les valeurs de départ sont au maximum de 10 commandes par seconde sur les lumières et 1 commande par seconde sur les groupes ; une limite atteinte met la demande en file avec échéance plutôt que de la perdre silencieusement.

Les animations rapides, variations continues de couleur ou entertainment ne passent pas par l'API REST classique. Elles sont refusées explicitement en V1, au lieu de dégrader le bridge et le flux d'événements.

## Modèle de disponibilité

| État | Sens |
|---|---|
| `starting` | appairage ou synchronisation initiale en cours |
| `available` | inventaire synchronisé et flux d'événements actif |
| `degraded` | dernier état connu servi, mais flux ou bridge indisponible |
| `unauthorized` | clé supprimée, refusée ou appairage requis |
| `disabled` | adaptateur désactivé par l'utilisateur |

Lorsque l'adaptateur est `degraded`, les entités Hue deviennent `unavailable` pour les nouvelles commandes. Leur dernier état rapporté reste consultable et clairement marqué comme potentiellement ancien.

## Interface utilisateur

La console Leptos propose : découverte/ajout de bridge, écran demandant l'appui sur le bouton, suivi d'appairage, aperçu d'inventaire, association optionnelle aux pièces Robine, état de santé et action de resynchronisation.

Les libellés affichent « Philips Hue » en texte simple lorsque nécessaire, sans reprendre logo, icône ni charte de la marque. Le premier écran d'appairage indique que Robine est une application indépendante.

## Tests

`robine-integration-hue` fournit un client de bridge abstrait et un faux déterministe. Les tests unitaires couvrent le mapping Hue -> Robine, les conversions d'unités, les erreurs de payload, le coalescing et les limites de débit. Les tests d'intégration rejouent des fixtures d'inventaire et de flux ; aucun test CI ne dépend d'un bridge réel.

Un test manuel de recette avec le bridge réel vérifie :

- appairage après appui physique ;
- découverte stable après redémarrage ;
- commande marche/arrêt et luminosité avec confirmation d'état ;
- changement fait dans l'application Hue visible dans Robine sans polling normal ;
- déconnexion/reconnexion du bridge sans doublon d'appareil ni perte de réconciliation ;
- révocation de la clé Hue détectée sans fuite de secret.

## Critères d'acceptation

- Robine démarre et sert son API même si le bridge est indisponible.
- Deux redécouvertes successives du même bridge conservent les mêmes identifiants publics Robine.
- Aucun protocole Zigbee, SDK Hue ou secret d'appairage ne dépend de `robine-domain` ou `robine-application`.
- Une commande commandée en `:reported` échoue visiblement si l'état correspondant n'arrive pas avant son timeout.
- Le bridge ne reçoit pas une rafale au-delà de la limite configurée, même quand plusieurs automatisations ciblent la même lumière.
- Le réseau local peut être entièrement isolé d'Internet après l'appairage sans empêcher le contrôle Hue par Robine.
