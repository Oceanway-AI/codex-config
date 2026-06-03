# codex-config

`codex-config` is a lightweight OceanWay AI desktop helper for configuring Codex. It is built with Tauri and Rust, with a small HTML/CSS/JavaScript interface.

This repository only contains the Rust/Tauri version. The old Python/PyQt packaging version has been removed from the cloud repository.

## What It Does

- Writes the OceanWay provider to Codex config.
- Uses a ChatGPT-login-preserving provider token when the user is already signed in, and falls back to `OPENAI_API_KEY` when no ChatGPT login is detected.
- Lets users save multiple named local API key profiles, such as subscription keys and balance keys.
- Lets users delete saved profiles.
- Uses `https://ocean-way.top` as the default Base URL.
- Preserves existing non-OceanWay Codex settings and providers.
- Creates a first-use backup before changing user config.
- Offers an explicit, backed-up history visibility migration for users who need old local sessions to appear under the current provider.
- Restores the user's original files when they click restore.
- Supports macOS and Windows builds.

## User Guide

For non-technical users, see:

- [codex-config 使用说明](docs/USER_GUIDE.md)

## User Files

The app reads and writes:

```text
~/.codex/config.toml
~/.codex/auth.json
~/.codex/oceanway-ai-keys.json
```

When the user explicitly clicks history migration, the app can also update:

```text
~/.codex/sessions/**/*.jsonl
~/.codex/archived_sessions/**/*.jsonl
~/.codex/state_5.sqlite
~/.codex/oceanway-history-migration-backup/
```

For users who are already signed in to ChatGPT, the OceanWay provider written to `config.toml` looks like this:

```toml
model_provider = "OceanWay"
model = "gpt-5.4"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.OceanWay]
name = "OceanWay"
base_url = "https://ocean-way.top"
wire_api = "responses"
experimental_bearer_token = "user-api-key"
requires_openai_auth = true
```

In that mode, `auth.json` keeps the ChatGPT login state and does not store the third-party key:

```json
{
  "auth_mode": "chatgpt",
  "OPENAI_API_KEY": null
}
```

If no ChatGPT login is detected before configuration, the app uses the fallback API key mode. The API key is written to `auth.json` without removing other existing auth fields:

```json
{
  "OPENAI_API_KEY": "user-api-key"
}
```

When the last saved key profile is deleted, `oceanway-ai-keys.json` is removed instead of leaving an empty key store behind.

## History Visibility Migration

History migration is optional and is not run during one-click configuration. It is only available when the current provider is OceanWay.

When the user clicks `迁移历史`, the app first scans local Codex history and asks for confirmation. If confirmed, it changes only provider metadata for existing local session records so sessions created under a previous provider can appear under OceanWay. It does not rewrite conversation content.

Before changing anything, the app creates a backup in:

```text
~/.codex/oceanway-history-migration-backup/
```

The migration updates the first `session_meta` line in matching JSONL files and matching rows in `state_5.sqlite` by thread id. If a session contains encrypted content, the app warns the user because the session may become visible in the list but may not be resumable or compactable under a different provider.

## Restore Behavior

On first configuration, the app stores a snapshot in:

```text
~/.codex/oceanway-ai-backup/
```

When the user clicks restore, the app restores that original snapshot. This lets users who already had a custom Codex setup return to their previous state, while users who had no config return to an empty/default state.

Restore also undoes recorded history visibility migrations by using the migration manifest. It only restores files and database rows that were changed by this tool, so sessions created after the migration are left alone. The app does not provide a default flow for migrating OceanWay-created sessions into OpenAI Official.

If no snapshot exists, restore falls back to removing only the OceanWay provider and `OPENAI_API_KEY`.

## Development

Install dependencies:

```bash
npm install
```

Run the desktop app in development mode:

```bash
npm run dev
```

Run Rust tests:

```bash
cd src-tauri
cargo test
```

## macOS Build

Build locally on macOS:

```bash
chmod +x ./build.sh
./build.sh
```

Outputs:

```text
src-tauri/target/release/bundle/macos/codex-config.app
dist/codex-config-macOS.zip
```

For public distribution, macOS builds should eventually be signed and notarized with an Apple Developer ID.

## Windows Build

Build locally on Windows PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

Outputs are created under:

```text
src-tauri\target\release\
src-tauri\target\release\bundle\
```

GitHub Actions can also build the Windows installer when you do not have a Windows machine.

## GitHub Actions

The workflow in `.github/workflows/build.yml` builds:

- `codex-config-macOS`
- `codex-config-Windows`

Run it from GitHub:

```text
Actions -> Build codex-config -> Run workflow
```

The generated artifacts can be downloaded from the completed workflow run page.

## Project Structure

```text
.github/workflows/build.yml   GitHub Actions build workflow
src/                          Frontend UI
src-tauri/                    Rust backend and Tauri configuration
build.sh                      macOS build helper
build.ps1                     Windows build helper
package.json                  Tauri CLI dependency and npm scripts
BUILD.md                      Short build notes
```
