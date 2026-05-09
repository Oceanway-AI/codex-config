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

## macOS

```bash
./build.sh
```

Output:

```text
src-tauri/target/release/bundle/macos/codex-config.app
dist/codex-config-macOS.zip
```

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
