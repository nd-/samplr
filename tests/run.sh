#!/usr/bin/env bash
# Runs every test of samplr: the Rust ones (library, app, command line), the
# interface's in a browser (tests/ui), then the real app's (tests/app) where
# they can run. Stops at the first suite failing.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

echo "== Rust: library, app, command line"
cargo test --workspace

echo "== Interface (Playwright, Chromium)"
if ! command -v npx >/dev/null; then
  echo "Node and npm are needed for the interface tests: install Node, then run this again." >&2
  exit 1
fi
cd tests/ui
# On a fresh clone: the test tools, and the browser they drive.
if [ ! -d node_modules ]; then
  npm ci
  npx playwright install chromium
fi
npx playwright test
cd "$root"

echo "== The real app (tauri-driver, WebKitGTK)"
# tauri-driver drives WebKitGTK on Linux only (macOS has no WebDriver for
# its web view), and the app needs a display to open its window.
if [ "$(uname)" != "Linux" ]; then
  echo "Skipped: the real app is driven on Linux only."
elif ! command -v tauri-driver >/dev/null || ! command -v WebKitWebDriver >/dev/null; then
  echo "Skipped: needs tauri-driver (cargo install tauri-driver) and WebKitWebDriver (WebKitGTK's)."
elif [ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
  echo "Skipped: no display to open the app's window."
else
  cargo build -p samplr -p samplr-app
  node --test --test-concurrency=1 tests/app/app.test.mjs
fi
