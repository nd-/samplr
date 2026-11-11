# Contributing to samplr

Install what the [README](README.md#installation) lists for your system, then work from the
project folder. Before handing a change back: `cargo build`, `cargo clippy` and `cargo fmt`
without warnings, and every test passing (see [Tests](#tests)).

## Proposing a change

- **A bug or an idea: open an issue first.** For a bug, say what you did, what you expected and
  what happened, on which system (Linux distribution or macOS version), with what samplr printed
  if anything.
- **Then a pull request**, from a branch of your own, about one thing only. A small change that
  builds and runs is easier to review than a big one; split a big one into steps that each build
  and run.
- **Keep samplr's two promises**: audio files are never moved, renamed or changed without an
  explicit action of the user (DAW projects point to their paths), and tags stay in samplr's
  database, never written into the files.

## Code

- **Everything in English**: identifiers, comments, messages shown to the user, documentation
  and commit messages.
- **Simple and readable** rather than clever: no abstraction before it is needed. A comment says
  why, when the code cannot; the code says what.
- **Linux and macOS for every choice**, and Windows in mind: paths through
  `dunce::canonicalize`, never a `/` written in a path, code for one system under
  `#[cfg(target_os = "…")]`.
- **The interface** (`ui/`) is plain HTML, CSS and JavaScript, with no build tool: its scripts
  are classic ones, loaded in order by `index.html`.

## Useful cargo commands

| Command        | Effect                                           |
|----------------|--------------------------------------------------|
| `cargo build`  | compiles                                         |
| `cargo run`    | compiles and runs                                |
| `cargo check`  | checks the code without building a binary (fast) |
| `cargo clippy` | suggestions to improve the code                  |
| `cargo fmt`    | formats the code automatically                   |

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): a prefix, an optional
scope, then a short title; a body only for what the change does not show and would still matter
in six months.

```text
feat: drop samples on a collection to add them
fix: no tempo detected from a sound without attacks
docs(readme): how to run the app
```

Prefixes: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `style`, `build`, `ci`, `chore`,
`revert`.

## Tests

Every test, from the project folder:

```sh
./tests/run.sh
```

It runs the Rust tests first, then the interface's, then the real app's, and stops at the first
suite that fails. The interface's need Node (any install: nvm, Homebrew, the system's packages);
on the first run the script installs their tools and the browser they drive
(`tests/ui/node_modules`, and Playwright's Chromium in its own cache).

The real app's run on Linux only, with a display, `tauri-driver` (`cargo install tauri-driver`)
and WebKitGTK's `WebKitWebDriver`; elsewhere the script says they were skipped. Each opens the
app's window for a few seconds, on a library of its own: your samples, database and settings
are left alone, and nothing is heard (the volume is set to zero).

The tests never touch your library: they work on WAV files they generate in a temporary folder
and on a throwaway database. Those that convert samples run ffmpeg if it is installed, and say
they were skipped otherwise.

| What                       | Where                | Run alone                              |
|----------------------------|----------------------|----------------------------------------|
| The library, unit tests    | `src/*.rs`           | `cargo test -p samplr --lib`           |
| The interface's commands   | `src-tauri/src/*.rs` | `cargo test -p samplr-app`             |
| The command line           | `tests/cli.rs`       | `cargo test -p samplr --test cli`      |
| The interface, in Chromium | `tests/ui/*.spec.js` | `cd tests/ui && npx playwright test`   |
| The README's Credits       | `tests/credits.rs`   | `cargo test -p samplr --test credits`  |
| The real app, in WebKitGTK | `tests/app/*.mjs`    | `node --test tests/app/app.test.mjs`   |

The interface is tested in a plain browser: `tests/ui/fake-tauri.js` stands in for the Rust
side, with a library of 50 samples. To run a single test: `cargo test -p samplr --lib watch::`,
or `npx playwright test -g "the arrow keys"` in `tests/ui`.

A dependency added (a crate, or a package of `tests/ui/package.json`) goes into the README's
Credits, and into the `CREDITED` table of `tests/credits.rs`, or its `UNCREDITED` list for a
small helper: the test fails until it does.

## Continuous integration

`.github/workflows/tests.yml` runs `./tests/run.sh` at each push to `main` and each pull request:
on Linux every suite, the real app's included (in a virtual display, with a silent audio
output); on macOS the Rust and interface tests. A pull request is merged once both pass.

## License of contributions

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
samplr by you, as defined in the Apache-2.0 license, shall be dual licensed as above (MIT or
Apache-2.0), without any additional terms or conditions.
