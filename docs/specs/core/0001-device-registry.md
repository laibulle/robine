# 0001 — Registre d'appareils

## Objectif

Maintenir l'identité stable des appareils et de leurs capacités, indépendamment du protocole qui les expose.

## Langage ubiquitaire

- **Appareil** : matériel ou service externe identifié dans Robine.
- **Entité** : unité contrôlable ou observable appartenant à un appareil (ampoule, capteur, prise, thermostat).
- **Capacité** : type de donnée ou de commande supporté par une entité, tel que `switch`, `light`, `temperature` ou `battery`.
- **Adresse protocolaire** : identifiant technique propre à un adaptateur ; elle n'est jamais l'identité métier publique.

## Modèle métier

Un `Device` possède un `DeviceId` opaque et stable, un nom affichable, une provenance, des métadonnées et des entités. Une `Entity` possède un `EntityId` opaque, une catégorie et une collection de capacités versionnées.

L'unicité est assurée par le couple `(adapter_id, protocol_address)`. Lorsqu'un appareil est redécouvert avec ce même couple, Robine met à jour ses métadonnées et conserve les identifiants publics existants.

Les appareils passent par les statuts `discovered`, `available`, `unavailable`, `removed`. La suppression est logique : l'historique reste consultable et une nouvelle découverte peut réactiver l'appareil si son identité protocolaire est reconnue.

## Cas d'utilisation

### Enregistrer une découverte

Entrée : adaptateur, adresse protocolaire, descripteur d'appareil et capacités annoncées.

Effet : crée ou met à jour l'appareil et ses entités ; émet `DeviceRegistered` ou `DeviceUpdated`.

### Renommer un appareil ou une entité

Entrée : identifiant public et libellé validé.

Effet : modifie uniquement le libellé utilisateur ; ne modifie jamais l'adresse protocolaire.

### Affecter une entité à une pièce

Entrée : identifiant public d'entité et identifiant de pièce, ou `null` pour retirer l'affectation.

Effet : modifie l'entité et la projection de son appareil dans la même transaction, émet
`entity.area_assigned`, et conserve l'affectation lors d'une redécouverte du matériel.

### Lister et rechercher le registre

Entrée : filtres de statut, adaptateur, catégorie, pièce et texte.

Effet : retourne une vue paginée, triée de manière stable, sans interroger un adaptateur distant.

### Retirer un appareil

Entrée : identifiant d'appareil et demande explicite.

Effet : désactive l'appareil, retire ses entités des sélecteurs actifs et émet `DeviceRemoved`.

## Ports applicatifs

- `DeviceRepository` : lecture et écriture atomique du registre.
- `DomainEventPublisher` : publication après persistance.
- `Clock` : horodatage injectable.

## Critères d'acceptation

- Une double découverte de la même adresse ne crée pas de doublon.
- Le renommage survit à un redémarrage et à une redécouverte.
- Une entité ne peut pas déclarer deux capacités de même clé et version.
- Une entité retirée ne reçoit aucune commande tant qu'elle n'est pas réactivée.
- Une redécouverte d'une lumière Hue ne modifie pas la pièce choisie par la personne.
