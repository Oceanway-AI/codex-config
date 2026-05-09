# Build Tauri App

This project now uses Tauri instead of Python packaging. The UI is plain HTML/CSS with two real input fields, and the native backend is Rust. This avoids bundling Python, Tkinter, PyQt, or the Qt runtime.

The app writes the same Codex files as the previous version:

- `~/.codex/config.toml`
- `~/.codex/auth.json`
- API key stored as `OPENAI_API_KEY`
- `requires_openai_auth = true`

The desktop UI also supports reading current status, testing the Base URL/API
Key pair, opening the Codex config directory, and toggling API Key visibility.

## Prerequisites

Install:

- Node.js / npm
- Rust / Cargo from `https://rustup.rs`

## macOS

Run:

```bash
chmod +x ./build.sh
./build.sh
```

The macOS app is created under:

```text
src-tauri/target/release/bundle/macos/codex-config.app
```

The distributable zip is created at:

```text
dist/codex-config-macOS.zip
```

## Windows

Run from PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

The Windows bundle is created under:

```text
src-tauri\target\release\bundle
```

## Development

Install dependencies:

```bash
npm install
```

Start Tauri dev mode:

```bash
npm run dev
```

Build release bundles:

```bash
npm run build
```

## Notes

Build on the same operating system you want to distribute for. Build the Windows package on Windows and the macOS app on macOS.
