# Oceanway Config 构建

当前桌面应用位于 `apps/desktop`，使用 React、Tauri 2 和 Rust。以下命令均在该目录执行，除非另有说明。

## 环境

- Node.js 22、pnpm 10.12.3。
- Rust 1.94.1，与 GitHub Actions 保持一致。
- Windows：Visual Studio C++ 桌面构建工具、Windows SDK、WebView2。
- macOS：Xcode Command Line Tools；对应目标为 `aarch64-apple-darwin` 或 `x86_64-apple-darwin`。
- Python 3 用于包内 MCP smoke；应用本体使用随包的原生 MCP。

## 开发与预览

```sh
cd apps/desktop
pnpm install --frozen-lockfile
pnpm dev
```

`pnpm dev` 启动 Tauri 桌面开发版。只预览界面可运行：

```sh
pnpm exec vite --port 3012 --host 127.0.0.1
```

打开 `http://127.0.0.1:3012/fixture.html` 使用模拟 IPC 预览。该页面用于界面测试，不写入真实 Agent 配置。

## 验证

```sh
pnpm typecheck
pnpm exec vitest run --maxWorkers=4 --minWorkers=1
pnpm build:renderer
```

原生测试在隔离副本或临时测试 home 中执行，`CC_SWITCH_TEST_HOME` 指向临时目录。进入 `apps/desktop/src-tauri` 后运行：

```sh
cargo test --locked --lib oceanway -- --test-threads=1
cargo test --locked --lib session_manager::providers::codex -- --test-threads=1
cargo test --locked --lib session_manager::providers::claude -- --test-threads=1
cargo test --locked --lib services::prompt::tests -- --test-threads=1
cargo test --locked --lib live::project::codex::tests -- --test-threads=1
```

当前功能验证结果见 [功能核验记录](docs/functional-verification.md)。

## Windows x64

在 `apps/desktop` 运行：

```powershell
pnpm tauri build --bundles nsis
python src-tauri/src/oceanway_images/scripts/mcp_bundle_smoke.py --executable src-tauri/target/release/oceanway-config.exe
```

输出：

- 安装程序：`apps/desktop/src-tauri/target/release/bundle/nsis/*.exe`。
- 原生可执行文件：`apps/desktop/src-tauri/target/release/oceanway-config.exe`。

MCP smoke 使用脚本创建的隔离 home，检查初始化、四个工具发现、输入关闭后退出、含中文及空格的 home、不同工作目录、无运行时 PATH 和应用数据不变。它不发起图片生成请求。

## macOS

分别在 Apple Silicon 或 Intel runner 上执行对应目标：

```sh
pnpm tauri build --target aarch64-apple-darwin --bundles app
pnpm tauri build --target x86_64-apple-darwin --bundles app
```

应用位于 `src-tauri/target/<target>/release/bundle/macos/Oceanway Config.app`。包内工具检查命令：

```sh
python src-tauri/src/oceanway_images/scripts/mcp_bundle_smoke.py --executable "src-tauri/target/<target>/release/bundle/macos/Oceanway Config.app/Contents/MacOS/oceanway-config"
```

CI 使用 `ditto -c -k --keepParent` 将 `.app` 打包为：

- `apps/desktop/dist/Oceanway-Config-macos-arm64.zip`。
- `apps/desktop/dist/Oceanway-Config-macos-intel.zip`。

## GitHub Actions

[build.yml](.github/workflows/build.yml) 在 `codex/**`、main/master、PR 或手动触发时构建 Windows x64、macOS arm64 和 macOS Intel，依次执行依赖安装、typecheck、全量前端测试、五组原生测试、安装包构建和包内 MCP smoke。五组原生测试分别作为独立步骤执行，任一失败都会停止该平台的后续打包。

产物名称为 `Oceanway-Config-<platform>-<commit>`；Windows 另有 `Oceanway-Config-windows-native-<commit>` 供隔离集成测试。Actions 产物保留 14 天。

[release.yml](.github/workflows/release.yml) 当前是手动复用同一构建流程。它生成 Actions 产物；正式 GitHub Release、签名更新包和更新清单尚未发布。
