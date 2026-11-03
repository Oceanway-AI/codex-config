# Oceanway Config

基于 CC Switch v4.0.24 的 Oceanway 桌面配置应用。

**[下载最新 Windows 安装包](https://github.com/Oceanway-AI/codex-config/releases/download/oceanway-preview/Oceanway-Config-Windows-x64-setup.exe)** · **[macOS Apple Silicon](https://github.com/Oceanway-AI/codex-config/releases/download/oceanway-preview/Oceanway-Config-macos-arm64.zip)** · **[macOS Intel](https://github.com/Oceanway-AI/codex-config/releases/download/oceanway-preview/Oceanway-Config-macos-intel.zip)**

以上是本开发分支最近一次完整构建成功的测试版，构建成功后自动更新。[查看版本、提交与安装说明](https://github.com/Oceanway-AI/codex-config/releases/tag/oceanway-preview) · [查看最新构建状态](https://github.com/Oceanway-AI/codex-config/actions/workflows/build.yml?query=branch%3Acodex%2Foceanway-config-rebuild)。Windows 下载后双击安装；macOS 解压后将应用放入“应用程序”。

新版本源码位于 `apps/desktop/`。沿用 CC Switch 的应用框架、组件和会话管理，提供 Oceanway Key 接入、模型选择与配置流程。需求见 [项目需求入口](docs/PROJECT_REQUIREMENTS.md)。

当前版本提供文本模型接入和图片 MCP，支持多模型选择、批量生图及历史保留。构建流程、验证结果与后续维护见 [开发交接](docs/DEVELOPMENT_HANDOFF.md)。

## 开发

```powershell
pnpm --dir apps/desktop install --frozen-lockfile
pnpm dev
```

本地原生开发需要 Rust 与对应平台的 Tauri 编译环境。GitHub Actions 的 **Build Oceanway Config** 在开发分支推送后运行 Windows / macOS 构建，测试安装包在该次运行的 Artifacts 下载。

```powershell
pnpm typecheck
pnpm test:desktop
pnpm build
```

仅检查界面可运行 `pnpm preview:desktop` 并打开 `http://127.0.0.1:3012/fixture.html`。测试页面使用 `fixture-key`；模型目录来自 2026-11-03 同一个 Console 测试 Key 返回的脱敏快照：界面显示其中 36 个文本模型和 8 个图片模型。正式应用从当前 Key 实时获取模型。预览页面的保存和重启动作使用模拟结果。

## 来源

- 上游与修改记录：[UPSTREAM.md](apps/desktop/UPSTREAM.md)
- CC Switch MIT 许可证：[LICENSE](apps/desktop/LICENSE)
- 根目录原配置程序保留为旧版对照，使用 `dev:legacy` / `build:legacy` 命令。


旧版使用和开发记录见 [1.3.0 归档](docs/legacy/README-v1.3.0.md)。
