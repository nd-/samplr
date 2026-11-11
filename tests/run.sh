#!/usr/bin/env bash
# Runs every test of samplr: the Rust ones (library, app, command line), then
# the interface's in a browser (tests/ui). Stops at the first suite failing.
set -euo pipefail

cd "$(dirname "$0")/.."

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
