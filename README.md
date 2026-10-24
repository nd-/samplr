# samplr

Gestionnaire de samples audio en Rust, pour macOS et Linux.

En ligne de commande pour l'instant : scanner un dossier de samples, les garder dans une base SQLite, en écouter un.

## Installation

### Sur le Mac (M4)

```sh
# 1. Outils de compilation d'Apple (clang, git…)
xcode-select --install

# 2. Rust (installe rustup, cargo et rustc dans ~/.cargo)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Ferme puis rouvre le terminal. Rien d'autre à installer : le son passe par CoreAudio, intégré à macOS.

### Sur Linux

```sh
# Debian / Ubuntu : en-têtes ALSA et pkg-config pour la sortie audio
sudo apt install build-essential pkg-config libasound2-dev
# Fedora : sudo dnf install gcc pkg-config alsa-lib-devel

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Vérifie l'installation avec `cargo --version`.

## Lancer le programme

Depuis le dossier `samplr` :

```sh
cargo run -- scan ~/Samples              # ajoute les samples du dossier à la base
cargo run -- liste                       # affiche les samples de la base
cargo run -- play ~/Samples/kick.wav     # joue un fichier
```

Le `--` sépare les options de cargo de celles du programme. La première compilation prend une minute, les suivantes quelques secondes.

Pour une version optimisée : `cargo build --release`, puis `./target/release/samplr scan ~/Samples`.

La base est créée au premier lancement, dans `~/.local/share/samplr/samplr.db` sous Linux et `~/Library/Application Support/samplr/samplr.db` sur Mac. Pour faire des essais sur une base à part : `SAMPLR_BASE=/tmp/essai.db cargo run -- scan ~/Samples`.

## Commandes cargo utiles

| Commande        | Effet                                              |
|-----------------|----------------------------------------------------|
| `cargo build`   | compile                                            |
| `cargo run`     | compile et lance                                   |
| `cargo check`   | vérifie le code sans produire de binaire (rapide)  |
| `cargo clippy`  | conseils pour améliorer le code                    |
| `cargo fmt`     | formate le code automatiquement                    |

## Éditeur conseillé

VS Code avec l'extension **rust-analyzer** : complétion, types affichés au survol et erreurs en direct. Il existe aussi Zed ou RustRover.

## Formats reconnus

WAV, AIFF, FLAC, MP3, OGG, M4A.
