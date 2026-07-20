# FFI-002 — Python, écosystème IA et formats de modèles

- Statut : **Draft**
- Version : **0.1.0**
- Domaine : `interop`

## Objet

Accéder à l’écosystème Python et aux modèles existants sans faire de Python le
modèle d’exécution obligatoire d’une release Robine.

## Modes

### Worker Python

Un environnement Python verrouillé s’exécute dans un processus isolé. Son
contrat déclare packages, version, capacités, protocole et limites.

Les valeurs traversent une frontière sérialisée ou des buffers partagés
validés. Les objets Python arbitraires ne deviennent pas des valeurs Robine.

### Bibliothèque native

Un package Python qui expose une ABI native peut être appelé selon FFI-001.
Cette voie ne rend pas automatiquement son API mémoire-sûre.

### Import de modèle

Robine importe un graphe et ses poids depuis un format supporté vers l’IR
COMP-002. L’import produit :

- opérations traduites ;
- opérations externes ;
- formes et précisions ;
- pré/post-traitements ;
- incompatibilités et fallbacks ;
- empreinte des poids.

## Développement et production

Le REPL peut utiliser un worker Python pour exploration. Le gate de release
indique explicitement si Python reste une dépendance d’exécution.

Un chemin de production peut :

- conserver le worker isolé ;
- exporter/importer le modèle ;
- remplacer progressivement les opérations ;
- lier une bibliothèque native.

Robine NE promet PAS de compiler tout Python dynamique.

## `Dynamic`

Une valeur provenant de Python est `Dynamic` jusqu’à décodage :

```text
decode<User>(dynamic)
tensor_from_buffer(dynamic, shape, dtype)
```

Le contrôle se produit à la frontière et peut être amorti par un handle de
buffer validé. `Dynamic` ne se propage pas implicitement dans du code typé.

## Modèles

Les formats portables DEVRAIENT inclure ONNX et une IR tensorielle stable
compatible avec l’écosystème. `safetensors` ou équivalent peut transporter les
poids sans exécution de code.

Une opération custom exige plugin signé, capacité et fallback ou cible requise.

## Reproductibilité

Environnement Python, code de conversion, dataset de calibration, poids et
versions de backend sont verrouillés. Une conversion non déterministe indique
seed et sources de variation.

## Performance

Le profiler sépare coût de sérialisation, copie, IPC, Python, bibliothèque
native et accélérateur. La frontière ne peut être qualifiée « zéro coût » sans
mesure.
