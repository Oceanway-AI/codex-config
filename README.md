# codex-config

`codex-config` is a lightweight OceanWay AI desktop helper for configuring Codex. It is built with Tauri and Rust, with a small HTML/CSS/JavaScript interface.

This repository only contains the Rust/Tauri version. The old Python/PyQt packaging version has been removed from the cloud repository.

## What It Does

- Writes the OceanWay provider to Codex config.
- Saves the API key as `OPENAI_API_KEY`.
- Uses `https://ocean-way.top` as the default Base URL.
- Preserves existing non-OceanWay Codex settings and providers.
- Creates a first-use backup before changing user config.
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
```

The OceanWay provider written to `config.toml` looks like this:

```toml
model_provider = "OceanWay"
model = "gpt-5.4"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.OceanWay]
name = "OceanWay"
base_url = "https://ocean-way.top"
wire_api = "responses"
requires_openai_auth = true
```

The API key is written to `auth.json`:

```json
{
  "OPENAI_API_KEY": "user-api-key"
}
```

## Restore Behavior

On first configuration, the app stores a snapshot in:

```text
~/.codex/oceanway-ai-backup/
```

When the user clicks restore, the app restores that original snapshot. This lets users who already had a custom Codex setup return to their previous state, while users who had no config return to an empty/default state.

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
