# DATA-001 — Sérialisation et évolution de schémas

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `data`

## Objet

Transporter des valeurs orientées données entre stockage, réseau, UI et
langages externes sans confondre champs statiques, clés textuelles et types
vivants.

## Non-objectifs

Aucun non-objectif supplémentaire n’est déclaré à ce stade.

## Spécification normative

### Schéma

Un schéma public contient :

- identités stables de type, champs et variantes ;
- noms externes ;
- types, optionalité et valeurs par défaut ;
- version et compatibilité ;
- contraintes sérialisables ;
- politique pour champs inconnus.

Les identités binaires NE DOIVENT PAS être dérivées d’un ordre de déclaration
fragile.

### Encodages

La bibliothèque standard fournit :

- une notation texte canonique orientée données ;
- un encodage binaire déterministe ;
- adaptateurs JSON ;
- intégration des formats de modèles selon FFI-002.

Le même schéma peut avoir plusieurs encodages sans changer son type métier.

### Texte et champs

Une clé JSON est `Text`. Elle ne devient un identifiant de champ qu’après
décodage par schéma. Les clés inconnues suivent la politique : rejeter, ignorer,
conserver dans une extension typée.

### Déterminisme

L’encodage canonique fixe ordre, normalisation et représentation numérique. Il
peut servir au hachage et aux signatures.

### Streaming

Les gros ensembles utilisent décodeur incrémental et `Stream<T>` avec
contre-pression. Un parseur streaming ne construit pas implicitement tout le
document en mémoire.

## Diagnostics et erreurs

Toute violation observable d’une exigence normative DOIT être rattachée à la source, à l’artefact ou à la frontière responsable.

## Sécurité, confidentialité et ressources

Les décodeurs sont bornés en profondeur, taille, allocations et nombre de
champs. Ils ne chargent aucun code à partir des données.

Les types secrets peuvent interdire sérialisation, log ou inspection.

## Interactions

- FFI-002
- DX-003

## Compatibilité et migration

Le vérificateur classe :

- ajout facultatif ou avec défaut ;
- retrait ;
- changement de représentation ;
- élargissement/rétrécissement ensembliste ;
- changement de tag ;
- modification de contrainte ;
- perte possible de données.

Une migration explicite transforme une valeur versionnée et peut être utilisée
par DX-003.

## Tests de conformité

Tests round-trip, corpus malveillant, compatibilité inter-version et
interopérabilité cross-language sont obligatoires.

## Questions ouvertes

Aucune à ce stade.
