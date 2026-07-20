# TOOL-002 — Adaptateurs éditeur et LSP

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `tooling`

## Objet

Permettre à un éditeur compatible LSP, dont Zed est le premier client, de
consommer le service Robine sans devenir une seconde implémentation du langage.

## Non-objectifs

- faire de LSP le protocole structurel canonique de Robine ;
- sélectionner la syntaxe source canonique ;
- définir un protocole de debugger ;
- publier une extension dans un registre d’éditeur.

## Spécification normative

### Adaptateur LSP

L’adaptateur LSP DOIT traduire les opérations standard vers TOOL-001 et NE DOIT
PAS recalculer résolution, types, effets, capacités ou domaines.

Le profil initial fournit au minimum :

- synchronisation complète et incrémentale des documents ;
- diagnostics ;
- hover ;
- navigation vers une définition ;
- symboles du document ;
- complétion ;
- formatage.

Une fonctionnalité absente de LSP PEUT utiliser une extension versionnée, mais
la réponse structurée de TOOL-001 reste la source de vérité.

### Snapshots et versions

Chaque document ouvert possède une version monotone. Un résultat asynchrone
DOIT être associé à la version analysée. Un diagnostic ou une édition calculé
sur une version périmée NE DOIT PAS remplacer un résultat plus récent.

La conversion entre positions LSP et positions source DOIT traiter
explicitement UTF-16, UTF-8 et fins de ligne ; elle NE DOIT PAS supposer qu’un
offset en octets est une colonne LSP.

### Diagnostics

L’adaptateur conserve au minimum :

- code stable ;
- sévérité ;
- emplacement primaire ;
- message orienté domaine ;
- données structurées nécessaires à un correctif sûr.

La CLI et l’éditeur DOIVENT produire le même code et la même cause primaire
pour un même snapshot, une même cible et un même profil.

### Grammaire de présentation

Un éditeur PEUT utiliser Tree-sitter ou une autre grammaire locale pour
coloration, indentation et navigation purement syntaxique. Cette grammaire NE
DOIT PAS décider de la validité sémantique d’un programme.

Lorsque LANG-002 reste en exploration, l’extension et la grammaire DOIVENT
déclarer le profil syntaxique provisoire qu’elles prennent en charge.

### Client Zed

Le premier client Zed est une extension mince qui :

- associe les fichiers Robine au profil syntaxique déclaré ;
- charge la grammaire de présentation ;
- lance le serveur Robine avec une commande et des arguments explicites ;
- délègue diagnostics et fonctions sémantiques à l’adaptateur LSP.

L’extension NE DOIT PAS embarquer une copie du solveur ou du compilateur. Si
elle télécharge un serveur, l’artefact et sa provenance suivent PKG-002.

## Diagnostics et erreurs

Un échec de lancement indique la commande recherchée et la manière de fournir
le serveur. Une réponse périmée est ignorée sans effacer un résultat récent.
Une position impossible ou une version inconnue produit une erreur de
protocole attribuée au client ou à l’adaptateur responsable.

## Sécurité, confidentialité et ressources

Le profil local ne transmet aucun source ni diagnostic sur le réseau.
L’extension n’exécute que le serveur déclaré et ne reçoit aucune capacité du
programme analysé. Les caches éditeur et compilateur restent séparés des
secrets du build.

## Interactions

- TOOL-001 possède le service sémantique et le protocole structurel ;
- DX-001 fournit snapshots, invalidation et cache ;
- DX-004 définit diagnostics et explications ;
- LANG-002 possède les profils syntaxiques ;
- PKG-002 contraint téléchargement et exécution des outils.

## Compatibilité et migration

L’adaptateur annonce ses versions LSP et Robine. L’ajout d’une capacité LSP
standard est compatible ; modifier le sens d’un code diagnostic, d’une
position ou d’une édition structurée suit la classification de META-001.

## Tests de conformité

La suite de conformité DOIT couvrir :

- diagnostic identique entre CLI et LSP ;
- rejet ou abandon d’un résultat périmé ;
- positions correctes avec caractères hors BMP et fins de ligne mixtes ;
- hover, définition, symboles, complétion et formatage sur source valide ;
- récupération sur source incomplet ;
- grammaire de présentation incapable d’accepter seule un programme invalide ;
- lancement du serveur par l’extension Zed sans solveur embarqué.

## Questions ouvertes

- Sous-ensemble des extensions LSP nécessaires aux patches structurels
  AI-001.
- Distribution vérifiable du serveur pour les extensions publiées.
