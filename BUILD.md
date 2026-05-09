# Build

Rust/Tauri 版本已经拆到独立目录：

```text
tauri-oceanway-config/
```

构建前需要安装：

- Node.js / npm
- Rust / Cargo

macOS:

```bash
cd tauri-oceanway-config
chmod +x ./build.sh
./build.sh
```

Windows:

```powershell
cd tauri-oceanway-config
powershell -ExecutionPolicy Bypass -File .\build.ps1
```
