# codex-config

`codex-config` is a lightweight OceanWay AI desktop helper for configuring Codex. It is built with Tauri and Rust, with a small HTML/CSS/JavaScript interface.

This repository only contains the Rust/Tauri version. The old Python/PyQt packaging version has been removed from the cloud repository.

## What It Does

- Writes the OceanWay provider to Codex config.
- Uses the stable API Key provider mode required by current Codex Desktop releases, while preserving unrelated existing login fields.
- Installs managed direct-image HTTP instructions for Codex, without an imagegen dependency, dedicated image CLI, or added MCP server.
- Reuses the previously saved OceanWay credential when the API Key field is left empty, without returning the full secret to the frontend.
- Uses `https://ocean-way.top` as the default Base URL.
- Preserves existing non-OceanWay Codex settings and providers.
- Creates a first-use backup before changing user config.
- Provides one-click diagnostics for provider, credential, image compatibility, Codex version, network, process state, and backup state.
- Copies a redacted support report that never includes complete API keys or bearer tokens.
- Can repair the saved configuration and restart Codex Desktop after confirmation.
- Offers an explicit, backed-up history visibility migration for users who need old local sessions to appear under the current provider.
- Restores the user's original files when they click restore.
- Reserves a typed account/quota/model-permission surface without claiming live data before the customer account API is connected.
- Checks and installs signed application updates from GitHub Releases.
- Supports macOS and Windows builds.

## User Guide

For non-technical users, see:

- [codex-config 使用说明](docs/USER_GUIDE.md)

## User Files

The app reads and writes:

```text
~/.codex/config.toml
~/.codex/auth.json
```

When the user explicitly clicks history migration, the app can also update:

```text
~/.codex/sessions/**/*.jsonl
~/.codex/archived_sessions/**/*.jsonl
~/.codex/state_5.sqlite
~/.codex/oceanway-history-migration-backup/
```

The OceanWay provider written to `config.toml` looks like this:

```toml
model_provider = "OceanWay"
model = "gpt-5.4"
model_reasoning_effort = "high"
disable_response_storage = true
cli_auth_credentials_store = "file"
forced_login_method = "api"

[model_providers.OceanWay]
name = "OceanWay"
base_url = "https://ocean-way.top"
wire_api = "responses"
requires_openai_auth = true
```

`auth.json` keeps unrelated existing login fields and stores the provider key under the standard API-key field:

```json
{
  "OPENAI_API_KEY": "user-api-key"
}
```

Existing ChatGPT tokens are retained, but `auth_mode` is removed so the new API key is selected. The original auth file and credential-store preference are backed up for restore. No actor-authorization image-extension header is added.

## Direct Image API

The conversational model is preserved. Images default to `gpt-image-2`; an explicitly requested image model takes precedence. Generation uses `/images/generations`, while references use `/images/edits` with original image bytes. Multiple requested images are scheduled with at most two requests in flight, not a fixed total-image limit.

Versioned instructions are merged into `developer_instructions` and the effective global `AGENTS.md` or `AGENTS.override.md`. This second route is necessary because desktop task-level developer instructions can replace the configured value. Existing text is preserved; malformed or duplicate ownership markers stop the write. Restore removes only the managed AGENTS block, preserving user additions.

The tool subprocess environment also receives:

```toml
[shell_environment_policy.set]
OPENAI_API_KEY = "user-api-key"
OPENAI_BASE_URL = "https://ocean-way.top"
```

These are not operating-system-wide environment variables. Instructions resolve the current provider before use and never send ChatGPT tokens to it. Keep the Key input empty to reuse the saved credential. Tools can access the configured Key, so use trusted projects and prompts.

After saving, fully restart Codex and create a new task. Saved rules and `/models` visibility do not establish successful natural-language generation. Higher-priority task policies and attachment-byte availability still apply. The optional image-test panel makes paid requests only when explicitly submitted, and is separate from conversation routing.

Windows restart binds process identity, excludes explicitly isolated instances, waits for old processes to exit and a new visible window to appear. A visible save dialog stops restart. Custom `CODEX_HOME` blocks automatic restart unless the dedicated acceptance harness explicitly binds an isolated desktop; it never falls back to the normal desktop.

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

Consumed restore snapshots are archived locally instead of deleted. If no snapshot exists, restore only removes configuration it can identify as tool-managed; it does not guess ownership of unrelated credentials.

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

## Diagnostics and Account Access

Starting with v1.3.0, the main window is organized as a configuration workspace:

- `问题诊断` runs read-only checks and can copy a redacted support report.
- `运维工具` contains restart, repair, image fallback synchronization, history migration, backup-directory access, and restore.
- `额度与权限` uses a backend response type ready for balance, plan, sync time, and per-model permission data. It intentionally returns `reserved` and empty values until the OceanWay customer account API is connected.

## Signed Auto Update

The updater endpoint is:

```text
https://github.com/Oceanway-AI/codex-config/releases/latest/download/latest.json
```

Update packages must be signed. The public key is committed in `src-tauri/tauri.conf.json`; the private key must remain outside the repository.

For this workstation, the generated private key is expected at:

```text
~/.tauri/oceanway-codex-config.key
```

Before using `.github/workflows/release.yml`, configure the repository secret:

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/oceanway-codex-config.key
```

Only set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the private key was generated with a password. Pushing a `v*` tag or manually running the release workflow builds signed updater artifacts into a draft GitHub Release. Review the draft, then publish it to make the new version discoverable.

## macOS Build

Build Apple Silicon and Intel packages locally:

```bash
chmod +x ./build.sh
./build.sh
```

Outputs are versioned under `dist/`, including:

```text
dist/codex-config-v1.3.0-macOS-arm64.dmg
dist/codex-config-v1.3.0-macOS-arm64.zip
dist/codex-config-v1.3.0-macOS-intel.dmg
dist/codex-config-v1.3.0-macOS-intel.zip
```

When the local updater private key exists, the script also produces signed `.app.tar.gz` updater archives and `.sig` files. The current local build uses ad-hoc app signing; public distribution should use an Apple Developer ID and notarization.

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

The validation workflow in `.github/workflows/build.yml` builds:

- `codex-config-macOS-arm64`
- `codex-config-macOS-intel`
- `codex-config-Windows`

The signed release workflow in `.github/workflows/release.yml` builds both macOS architectures and Windows, creates updater artifacts, and attaches them to a draft release.

Run validation from GitHub:

```text
Actions -> Build codex-config -> Run workflow
```

The generated artifacts can be downloaded from the completed workflow run page.

## Project Structure

```text
.github/workflows/build.yml   Pull-request build workflow
.github/workflows/release.yml Signed updater release workflow
src/                          Frontend UI
src-tauri/                    Rust backend and Tauri configuration
build.sh                      macOS build helper
build.ps1                     Windows build helper
package.json                  Tauri CLI dependency and npm scripts
BUILD.md                      Short build notes
```
