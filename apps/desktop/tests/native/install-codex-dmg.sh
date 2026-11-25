#!/bin/bash
set -euo pipefail
[[ "${RUNNER_ENVIRONMENT:-}" == github-hosted && "${RUNNER_OS:-}" == macOS ]]
mount="$(mktemp -d "$RUNNER_TEMP/codex-volume.XXXXXX")"
hdiutil attach -readonly -nobrowse -mountpoint "$mount" "$1"
trap 'hdiutil detach "$mount"' EXIT
apps=("$mount"/*.app)
[[ ${#apps[@]} == 1 && -d "${apps[0]}" ]]
[[ "$(plutil -extract CFBundleIdentifier raw "${apps[0]}/Contents/Info.plist")" == com.openai.codex ]]
codesign --verify --deep --strict "${apps[0]}"
mkdir -p "$HOME/Applications"
target="$HOME/Applications/$(basename "${apps[0]}")"
[[ ! -e "$target" ]]
ditto "${apps[0]}" "$target"
