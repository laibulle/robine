# Robine

Serveur domotique local-first écrit en Rust. Le premier vertical slice fournit
le registre Hue, les états persistés dans SQLite, les commandes idempotentes,
l'API Actix et les événements WebSocket.

## Lancer le serveur

```sh
cargo run -p robine-runtime
```

Par défaut, Robine écoute uniquement `127.0.0.1:3030` et conserve ses données
dans `./data`. Ces deux valeurs peuvent être changées avec `ROBINE_BIND` et
`ROBINE_DATA_DIR`.

À la première exécution, créer l'administrateur depuis loopback :

```sh
curl -X POST http://127.0.0.1:3030/api/v1/setup/administrator \
  -H 'content-type: application/json' \
  -d '{"password":"une phrase de passe longue"}'
```

La réponse contient le jeton Bearer local, affiché une seule fois. Les routes
produit utilisent ensuite `Authorization: Bearer <jeton>`.

Si ce jeton est perdu, la machine qui héberge Robine peut en récupérer un avec
le mot de passe administrateur, toujours via loopback :

```sh
curl -X POST http://127.0.0.1:3030/api/v1/auth/tokens \
  -H 'content-type: application/json' \
  -d '{"password":"une phrase de passe longue"}'
```

Depuis le réseau local, cette même opération requiert en plus un bearer déjà
valide : le mot de passe seul n’est jamais accepté comme connexion distante.

## Restauration de maintenance

La restauration ne s'effectue jamais via le serveur actif. Arrêter d'abord
Robine, puis lancer la commande suivante avec le manifeste situé directement
dans `backups/` :

```sh
ROBINE_DATA_DIR=./data cargo run -p robine-runtime -- \
  restore --manifest ./data/backups/robine-….manifest.json --confirm
```

La commande vérifie le manifeste et l'intégrité SQLite, refuse si le runtime
détient encore son verrou, puis conserve la base précédente sous
`robine-pre-restore-…sqlite3`.

## Vérification

```sh
cargo test --workspace
```

Le test Hue n'accède pas au réseau : il utilise un bridge déterministe afin de
vérifier la découverte, la stabilité des identifiants, la commande et les
événements persistés.
