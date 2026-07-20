# PKG-003 — Applications et frontière hôte

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `packages`

## Objet

Définir comment une cible exécutable choisit sa racine de composition et reçoit
les valeurs et capacités fournies par son hôte sans autorité ambiante.

## Non-objectifs

- sélectionner la syntaxe source canonique ;
- définir les applications asynchrones, les services ou le cycle de vie UI ;
- normaliser toutes les capacités de plateforme.

## Spécification normative

### Profil d’application synchrone

Le profil portable initial `app.sync-v0` DOIT déclarer dans le manifeste :

- le module source principal ;
- l’identité qualifiée de la fonction racine ;
- les capacités maximales accordées par l’hôte ;
- le domaine d’exécution de la racine.

La fonction racine DOIT être une définition de module, recevoir explicitement
ses valeurs et capacités hôtes, puis retourner `Unit`. Son chargement de module
NE DOIT PAS exécuter d’effet conformément à LANG-003.

Le lancement DOIT échouer avant l’exécution si la racine est absente, si sa
signature ne correspond pas au profil ou si une capacité demandée dépasse le
manifeste.

### Injection de l’hôte

Une capacité hôte est fournie comme paramètre non forgeable à la racine. Elle
NE DOIT PAS être obtenue depuis une variable globale, l’environnement du
processus, un singleton caché ou une initialisation de module.

Une bibliothèque reçoit une capacité uniquement par passage explicite depuis
la racine ou par une délégation plus restrictive autorisée par TYPE-004.

### Console portable

Le profil `app.sync-v0` définit une capacité minimale `Console`. Son opération
`write_line` :

- reçoit un `Text` ;
- produit `Unit` ;
- porte l’effet observable `Console.Write` et tout effet de contrôle ou de
  ressource que l’implémentation ne sait pas éliminer ;
- écrit une ligne complète dans l’ordre d’évaluation du programme.

L’accès à `Console` DOIT être accordé par la capacité manifeste
`console.write`. Un domaine qui interdit un effet conservé par l’opération
rejette l’appel avant exécution.

### Terminaison

Le retour normal de `Unit` correspond à une terminaison réussie. Une erreur de
chargement, de contrat hôte ou d’exécution produit une terminaison en échec et
un diagnostic ; son encodage numérique précis est défini par la cible.

## Diagnostics et erreurs

Un diagnostic de lancement DOIT distinguer au minimum racine absente, signature
incompatible, capacité non accordée et domaine incompatible. Une erreur de
console DOIT être attribuée à l’appel responsable et conserver sa cause hôte.

## Sécurité, confidentialité et ressources

L’hôte NE DOIT PAS accorder une capacité qui dépasse le manifeste effectif. La
console peut divulguer les données écrites et peut bloquer ou allouer selon la
cible ; ces coûts DOIVENT rester visibles dans son contrat effectif.

## Interactions

- LANG-003 interdit les effets au chargement d’un module ;
- TYPE-004 définit effets, capacités et délégation ;
- RUN-004 définit les domaines d’exécution ;
- PKG-001 définit manifeste et commande `run` ;
- PKG-002 définit l’autorité nulle par défaut ;
- STD-001 gouverne l’évolution de la console standard.

## Compatibilité et migration

Cette première version ajoute un profil portable sans modifier un format
accepté antérieur. Le nom du profil est versionné ; une évolution incompatible
utilise un autre profil ou une nouvelle version selon META-001.

## Tests de conformité

La suite de conformité DOIT couvrir :

- exécution réussie d’une racine synchrone retournant `Unit` ;
- rejet d’une racine absente ou de signature incompatible ;
- rejet d’un appel console sans `console.write` ;
- passage explicite de `Console` de la racine à une bibliothèque ;
- ordre de plusieurs écritures console ;
- absence d’effet au chargement ;
- refus d’une capacité ou d’un effet incompatible avec le domaine.

## Questions ouvertes

- Forme commune des profils asynchrones, services et applications UI.
- Représentation portable des arguments, de l’environnement explicite et du
  statut de sortie détaillé.
