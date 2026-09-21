# codex-config

`codex-config` is a lightweight OceanWay AI desktop helper for configuring Codex. It is built with Tauri and Rust, with a small HTML/CSS/JavaScript interface.

This repository only contains the Rust/Tauri version. The old Python/PyQt packaging version has been removed from the cloud repository.

## What It Does

- Writes the OceanWay provider to Codex config.
- Preserves an existing ChatGPT login and uses a provider token, or API-key authentication when no login exists.
- Installs versioned direct-image HTTP rules in `developer_instructions`, preserving the user's existing instructions. No imagegen skill, built-in image tool, dedicated image CLI, or MCP is required by this route.
- Defaults to `gpt-image-2`, honors an explicit model and any requested image count, and supports original-byte reference images and subsequent edits.
- Reuses the previously saved OceanWay credential when the API Key field is left empty, without returning the full secret to the frontend.
- Uses `https://ocean-way.top` as the default Base URL.
- Preserves existing non-OceanWay Codex settings and providers.
- Creates a first-use backup before changing user config.
- Separates rule configuration, model visibility, generation testing and reference-image testing. Only the explicit image-test action submits billable requests.
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

```toml
model_provider = "OceanWay"
model = "gpt-5.4"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.OceanWay]
name = "OceanWay"
base_url = "https://ocean-way.top"
wire_api = "responses"
requires_openai_auth = false
```

The app no longer adds local-image-extension headers. A legacy header is removed only when its value matches this tool's old managed value. API-key authentication preserves other existing auth fields:

```json
{
  "OPENAI_API_KEY": "user-api-key"
}
```

After configuration, save ongoing work, restart Codex Desktop and create a new task. The conversational model is unchanged; it is instructed to call the provider's image endpoint through general HTTP tools. Saving rules does not prove that a particular desktop build has loaded them.

## Direct Image API (1.4.0-beta.1)

Both authentication modes receive a versioned, marker-delimited `developer_instructions` block and the following tool-subprocess environment:

```toml
[shell_environment_policy.set]
OPENAI_API_KEY = "user-api-key"
OPENAI_BASE_URL = "https://ocean-way.top"
```

Rules read the current provider and credentials at runtime. They contain variable names, never the user's Key. Changing providers disables these OceanWay-specific rules rather than silently using an old endpoint. System skills and permission controls are not changed.

Text-only generation uses `/images/generations`; references use multipart `/images/edits` with ordered `image[]` parts. A root URL receives `/v1`, an existing API prefix is retained. Original reference bytes are required. If a Codex attachment is visible but inaccessible as a file/byte stream, Codex must request a readable file instead of silently generating from a description.

Requested N images are scheduled as N slots, with at most two requests in flight. The test panel conservatively uses one image per request until service batching is proven; this does not cap the total. Defaults: one image if count is omitted, 1024x1024 if size is omitted, no forced quality parameter. Successful results are retained; retry only resubmits missing slots after an explicit action. Timeout is ambiguous and is never automatically retried. Cancellation stops the queue but cannot undo provider work already submitted.

`直接图片 API` can synchronize rules, inspect model visibility, and explicitly test a model/prompt/count with optional references. Generated test files are stored under `$CODEX_HOME/oceanway-image-tests/`; conversation rules save outputs under the current project's `output/images/`. The API accepts Base64 and public HTTPS URL outputs; download requests never receive the provider Key.

Leaving the API Key field empty reuses the saved credential. Entering a new Key updates both authentication and the HTTP environment. The frontend and diagnostic reports never receive it. Commands launched by Codex can read the subprocess environment: use trusted repositories/tasks.

This is a test-branch build, not a production release. Browser fixtures are simulations, never proof of live success. Desktop natural-language triggering, upload/paste/drop attachment access, and paid provider generation/edit tests require explicit user acceptance. See [beta acceptance checklist](docs/DIRECT_IMAGE_BETA.md).

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

If no snapshot exists, restore removes the OceanWay provider and managed instruction block. HTTP environment entries are removed only when they match the saved OceanWay credentials and endpoint.

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
- `运维工具` contains restart, repair, direct-image rule synchronization/testing, history migration, backup-directory access, and restore.
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
