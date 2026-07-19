#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

APP_NAME="${APP_NAME:-codex-config}"
VERSION="$(node -p "require('./package.json').version")"
UPDATER_KEY_PATH="${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.tauri/oceanway-codex-config.key}"

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
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  mkdir -p dist

  CONFIG_ARGS=(--config src-tauri/tauri.adhoc.conf.json)
  if [[ -f "$UPDATER_KEY_PATH" ]]; then
    export TAURI_SIGNING_PRIVATE_KEY
    TAURI_SIGNING_PRIVATE_KEY="$(<"$UPDATER_KEY_PATH")"
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
    CONFIG_ARGS+=(--config src-tauri/tauri.updater.conf.json)
  else
    echo "Updater key not found at $UPDATER_KEY_PATH; signed updater archives will be skipped."
  fi

  for build_spec in \
    "aarch64-apple-darwin:arm64" \
    "x86_64-apple-darwin:intel"
  do
    TARGET="${build_spec%%:*}"
    LABEL="${build_spec##*:}"
    npm exec tauri -- build \
      --target "$TARGET" \
      --bundles app,dmg \
      "${CONFIG_ARGS[@]}"

    BUNDLE_ROOT="src-tauri/target/$TARGET/release/bundle"
    APP_PATH="$BUNDLE_ROOT/macos/$APP_NAME.app"
    ZIP_PATH="dist/codex-config-v$VERSION-macOS-$LABEL.zip"
    DMG_PATH="$(find "$BUNDLE_ROOT/dmg" -maxdepth 1 -name '*.dmg' -print -quit)"

    rm -f "$ZIP_PATH"
    ditto -c -k --keepParent "$APP_PATH" "$ZIP_PATH"
    cp "$DMG_PATH" "dist/codex-config-v$VERSION-macOS-$LABEL.dmg"

    if [[ -f "$APP_PATH.tar.gz" ]]; then
      cp "$APP_PATH.tar.gz" "dist/codex-config-v$VERSION-macOS-$LABEL.app.tar.gz"
      cp "$APP_PATH.tar.gz.sig" "dist/codex-config-v$VERSION-macOS-$LABEL.app.tar.gz.sig"
    fi
  done

  echo
  echo "Built Apple Silicon and Intel packages under: $SCRIPT_DIR/dist"
else
  npm run build

  echo
  echo "Built Tauri bundles under: $SCRIPT_DIR/src-tauri/target/release/bundle"
fi
