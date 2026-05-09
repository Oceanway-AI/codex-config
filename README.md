# OceanWay AI Codex 配置工具

这个仓库现在保留两部分：

```text
setup-codex-provider.py      旧 Python 版本，保留作参考
tauri-oceanway-config/       新 Rust/Tauri 轻量桌面版
```

建议后续只维护和分发 `tauri-oceanway-config/`。

## 使用 Rust/Tauri 版本

进入独立目录：

```bash
cd tauri-oceanway-config
```

开发运行：

```bash
npm install
npm run dev
```

macOS 构建：

```bash
./build.sh
```

Windows 构建：

```powershell
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

根目录的 `build.sh` 和 `build.ps1` 只是转发到 `tauri-oceanway-config/`，方便你仍然可以在根目录执行构建。
