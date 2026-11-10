# samplr

Audio sample manager written in Rust, for macOS, Linux and Windows (Windows not supported yet).

Scan sample folders, keep them in a SQLite database, tag them, find them again and listen to them. A command line tool, and a graphical interface in progress (Tauri).

## Installation

### On Mac (Apple Silicon)

```sh
# 1. Apple's build tools (clang, git…)
xcode-select --install

# 2. Rust (installs rustup, cargo and rustc in ~/.cargo)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Close and reopen the terminal. Nothing else to install: sound goes through CoreAudio, built into macOS.

### On Linux

```sh
# Debian / Ubuntu: ALSA headers and pkg-config for audio output
sudo apt install build-essential pkg-config libasound2-dev
# Fedora: sudo dnf install gcc pkg-config alsa-lib-devel

# For the graphical interface (Tauri):
# Debian / Ubuntu: sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev
# Fedora: sudo dnf install webkit2gtk4.1-devel libsoup3-devel gtk3-devel javascriptcoregtk4.1-devel libappindicator-gtk3-devel librsvg2-devel openssl-devel

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Check the installation with `cargo --version`.

## Running

From the project folder:

```sh
cargo run -- scan ~/Samples                  # adds the folder's samples to the database
cargo run -- list                            # shows the samples in the database
cargo run -- tag ~/Samples/kick.wav kick 808 # adds tags to a sample
cargo run -- untag ~/Samples/kick.wav 808    # removes some
cargo run -- tags                            # lists the tags
cargo run -- search '#kick' hard bpm:120     # tagged kick, "hard" in the path or a tag, at 120 BPM
cargo run -- watch                           # updates the database live when scanned folders change
cargo run -- play ~/Samples/kick.wav         # plays a file
cargo run -- wave ~/Samples/kick.wav         # draws its waveform in the terminal
cargo run -- waveforms                       # computes the missing waveforms
cargo run -- help                            # every command
cargo run -p samplr-app                      # opens the graphical interface
```

The `--` separates cargo's options from the program's. The first build takes a minute, the next ones a few seconds.

For an optimised build: `cargo build --release`, then `./target/release/samplr scan ~/Samples`.

The database is created on first launch, in `~/.local/share/samplr/samplr.db` on Linux and `~/Library/Application Support/samplr/samplr.db` on Mac. To experiment on a separate database: `SAMPLR_DB=/tmp/test.db cargo run -- scan ~/Samples`.

## Useful cargo commands

| Command         | Effect                                            |
|-----------------|---------------------------------------------------|
| `cargo build`   | compiles                                          |
| `cargo run`     | compiles and runs                                 |
| `cargo check`   | checks the code without building a binary (fast)  |
| `cargo clippy`  | suggestions to improve the code                   |
| `cargo fmt`     | formats the code automatically                    |

## Recommended editor

VS Code with the **rust-analyzer** extension: completion, types on hover and live errors. Zed and RustRover work too.

## Supported formats

WAV, AIFF, FLAC, MP3, OGG, M4A.
