# LANG-002 — Formes source et syntaxe canonique

- Statut : **Exploration**
- Version : **0.3.0**
- Domaine : `language`

## Objet

Sélectionner la syntaxe source canonique sans confondre la qualité du REPL,
l’orientation expressions et l’homoiconicité.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Exigences indépendantes de la syntaxe

La syntaxe retenue DOIT :

- délimiter une unité évaluable sans analyser le projet entier ;
- produire un arbre à identités stables pour les patches structurels ;
- avoir un formatage canonique ;
- permettre une récupération d’erreur incrémentale ;
- représenter types, effets et contrats sans sous-langage opaque ;
- conserver des positions source après expansion ou dérivation ;
- être lisible pour les domaines mathématiques, UI et services ;
- éviter que des extensions modifient silencieusement la grammaire.

### Candidats

#### S-expressions

Atouts :

- lecture en structures régulières ;
- édition structurelle ;
- macros et REPL naturellement alignés ;
- grammaire minimale.

Risques :

- DSL privés et fragmentation sémantique ;
- faible distinction visuelle entre appel et forme spéciale ;
- coût d’adoption ;
- formules numériques et types riches moins immédiatement lisibles.

#### Syntaxe conventionnelle orientée expressions

Atouts :

- familiarité ;
- formules, signatures et appels de plateformes naturels ;
- davantage de repères visuels.

Risques :

- parseur et récupération d’erreur plus complexes ;
- macros moins uniformes ;
- représentation source distincte des données.

### Décision provisoire

Aucun candidat n’est retenu. Les prototypes DOIVENT partager la même IR
sémantique et être comparés sur :

1. temps de parse incrémental ;
2. qualité de récupération après source incomplet ;
3. précision des patches IA ;
4. lisibilité de DSP, acteurs, UI et types ensemblistes ;
5. capacité de refactoring ;
6. compréhension par des développeurs non Lisp ;
7. propension à créer des sous-langages incompatibles.

### Métaprogrammation

Quelle que soit la syntaxe :

- les reader macros globales sont interdites ;
- une transformation DOIT produire une structure revérifiable avec source map ;
- l’hygiène est obligatoire par défaut selon LANG-004 ;
- une macro portable est pure et sans autorité ambiante ;
- les I/O et outils de génération appartiennent à une tâche de build déclarée
  selon PKG-002, pas à une macro.

### Représentation structurelle

Le compilateur DOIT exposer la famille abstraite publique et versionnée :

```text
Syntax<Kind, Phase>
```

`Kind` décrit la catégorie structurelle, par exemple expression, pattern, type
ou déclaration. `Phase` indique au minimum si la vue est lue, expansée,
résolue ou typée.

Une vue NE DOIT PAS prétendre contenir noms résolus, types, effets ou ownership
avant la phase qui les établit. Les interfaces machine DOIVENT mentionner la
phase ; `Syntax<T>` ne peut être utilisé que comme abréviation narrative lorsque
la phase est déjà fixée sans ambiguïté.

Cette API ne présuppose pas que le source soit lui-même une liste.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Aucune exigence supplémentaire spécifique à cette fonctionnalité n’est définie.

## Interactions

- LANG-004
- PKG-002

## Compatibilité et migration

La version 0.3.0 remplace le modèle ambigu `Syntax<T>` par
`Syntax<Kind, Phase>`. Les outils et caches doivent ajouter la phase à leurs
contrats et clés ; ce changement est ABI-breaking pour l’API structurelle.

La version 0.2.0 sépare les macros pures des tâches de build avec effets. Une
extension qui effectuait une I/O pendant l’expansion doit produire un artefact
par une tâche de build PKG-002, puis le fournir comme entrée explicite à la
macro. Ce changement est source-breaking pour ces extensions.

## Tests de conformité

La suite de conformité DOIT couvrir :

- mêmes catégories `Kind` sous chaque prototype syntaxique ;
- absence d’informations résolues dans une phase antérieure ;
- présence de types, effets et ownership dans la vue typée ;
- rejet d’une interface machine omettant `Phase` ;
- migration et invalidation d’un cache `Syntax<T>` antérieur.

## Questions ouvertes

- Une projection S-expression non canonique est-elle utile aux outils ?
- Les macros utilisateur font-elles partie de Robine 1.0 ?
- Une syntaxe unique peut-elle servir correctement DSP et UI ?
- Noms et granularité exacts des phases publiques au-delà du minimum normatif.
