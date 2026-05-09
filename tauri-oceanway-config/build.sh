#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

APP_NAME="${APP_NAME:-codex-config}"

if ! command -v cargo >/dev/null 2>&1 && [[ -f "$HOME/.cargo/env" ]]; then
  # Load Rust when the current shell has not picked up rustup's PATH change yet.
  # shellcheck disable=SC1090
  . "$HOME/.cargo/env"
fi

command -v npm >/dev/null 2>&1 || {
  echo "npm was not found. Install Node.js, then rerun this script." >&2
  exit 1
}

command -v cargo >/dev/null 2>&1 || {
  echo "cargo was not found. Install Rust from https://rustup.rs, then rerun this script." >&2
  exit 1
}

npm install

if [[ "$(uname -s)" == "Darwin" ]]; then
  npm exec tauri -- build --bundles app

  APP_PATH="src-tauri/target/release/bundle/macos/$APP_NAME.app"
  ZIP_NAME="${ZIP_NAME:-codex-config-macOS.zip}"
  mkdir -p dist
  rm -f "dist/$ZIP_NAME"
  ditto -c -k --keepParent "$APP_PATH" "dist/$ZIP_NAME"

  echo
  echo "Built app: $APP_PATH"
  echo "Built zip: dist/$ZIP_NAME"
else
  npm run build

  echo
  echo "Built Tauri bundles under: $SCRIPT_DIR/src-tauri/target/release/bundle"
fi
