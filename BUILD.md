# Build

This repository is a Tauri/Rust desktop app. It does not use Python packaging.

## Requirements

- Node.js and npm
- Rust and Cargo from `https://rustup.rs`

## Development

```bash
npm install
npm run dev
```

## Test

```bash
cd src-tauri
cargo test
```

## macOS (Apple Silicon + Intel)

```bash
./build.sh
```

Output:

```text
dist/codex-config-v<version>-macOS-arm64.dmg
dist/codex-config-v<version>-macOS-arm64.zip
dist/codex-config-v<version>-macOS-intel.dmg
dist/codex-config-v<version>-macOS-intel.zip
```

If `~/.tauri/oceanway-codex-config.key` exists, the script also creates signed updater archives and signatures. The updater private key must never be committed.

## Windows

```powershell
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

Output:

```text
src-tauri\target\release\bundle\
```

## GitHub Actions

The workflow builds macOS and Windows artifacts from the repository root:

```text
.github/workflows/build.yml
```

Signed release artifacts and `latest.json` are created by:

```text
.github/workflows/release.yml
```
