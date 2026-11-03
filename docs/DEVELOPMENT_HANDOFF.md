# Oceanway Config 开发交接

更新：2026-11-04。分支：`codex/oceanway-config-rebuild`。

产品功能、图片 MCP 工具契约、API 接入和验收统一见 [需求与功能说明](PROJECT_REQUIREMENTS.md)。实际调用与安装结果见 [功能核验](functional-verification.md)，客户操作见 [使用说明](USER_GUIDE.md)。

## 开发命令

```powershell
pnpm --dir apps/desktop install --frozen-lockfile
pnpm dev
pnpm typecheck
pnpm test:desktop
pnpm build
```

独立图片模块及 MCP 子进程测试：

```shell
cargo test --manifest-path apps/desktop/src-tauri/src/oceanway_images/test-harness/Cargo.toml
```

## 构建与交付

[build.yml](../.github/workflows/build.yml) 构建 Windows x64、macOS Apple Silicon 和 macOS Intel，并执行包内 MCP 检查。三个目标完成后更新 [oceanway-preview](https://github.com/Oceanway-AI/codex-config/releases/tag/oceanway-preview)，记录源码提交、构建链接和 SHA256。

功能联调和本地验证完成后，再推送需要构建的代码或手动触发工作流。仅整理文档的提交使用 `[skip ci]`。

```shell
gh workflow run build.yml --repo Oceanway-AI/codex-config --ref codex/oceanway-config-rebuild
```

真实接入测试使用独立数据目录和 Agent 进程，按需求文档的验收编号记录结果，完成后清理测试环境。用户手册使用实际安装包的操作截图。
