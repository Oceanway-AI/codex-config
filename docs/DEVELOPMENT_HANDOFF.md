# Oceanway Config 开发交接

更新：2026-11-04。分支：`codex/oceanway-config-rebuild`。

现行需求及其修订从 [Oceanway Config 项目文档](https://docs.oceanway.site/docs/03_项目/codex-config) 进入。本分支原有功能、图片 MCP 工具契约、API 接入和验收材料见 [重建需求基线](PROJECT_REQUIREMENTS.md)。实际调用与安装结果见 [功能核验](functional-verification.md)，客户操作见 [使用说明](USER_GUIDE.md)。

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

## 2026-11-25 图片 MCP 适配

本轮为现有图片能力修复，采用 PRD DEV-X-07-001 v1.1、FS DEV-X-07-002 v1.2 的 CFG-A19/20/21/53/54。已读取 oceanway-devops 的开发交付、测试及变更步骤。源码基线为已发布的 `7cb0ad565c0a9cb7dff717b6d7811c57ad769f7a`，工作分支为 `codex/image-mcp-catalog-20261125`。

当前实现改用摘要与匹配修订的详情，读取原生请求变体；GPT 本地图采用 multipart，Gemini 采用 inlineData，Grok 尺寸采用其公布字段。New API 使用客户站点和 Key。任务回执保存并使用同站 Location，已有回执保持可读。

回归命令仍为上方独立图片模块测试。新增 `image_protocol_tests.rs` 覆盖当前 10 个模型的 36 个请求准备组合、自定义路径与原图字节；目录测试覆盖分页及修订匹配；异步测试覆盖 202/503/504 的原任务恢复。真实接口批次与图片保存在 `.artifacts/image-mcp-20261125/`，最终结果沿功能核验记录。

最终回归共 121 项通过，真实调用结果及服务端失败见[本轮核验](functional-verification.md#2026-11-25console-与-new-api-图片-mcp)。本轮源码尚未推送或发布安装包。Docs 的 F04 规范已本地修订；全仓检查受原有 9 篇 Platform 文档的 front matter 解析失败及 Windows symlink 测试权限影响，Docs 修改待同步，日志保存在本轮证据目录。
