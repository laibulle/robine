# Console Web Robine

La console est un secours local, compilé en WebAssembly avec Leptos CSR.

```sh
cargo install trunk
trunk build --release
```

Le bundle est écrit dans `dist/`. Au lancement, `robine-runtime` le sert à la
racine ; `ROBINE_WEB_DIR` permet de fournir un autre répertoire de bundle.
