# Oceanway Config 开发交接

日期：2026-11-03。工作包：WP-CODEXCONFIG-001。代码与文档工作分支均为 `codex/oceanway-config-rebuild`。

当前范围为 Oceanway 文本模型配置、图片 MCP、六款应用安装管理和历史保留。需求正文见 [项目需求入口](PROJECT_REQUIREMENTS.md)，实际验证见 [功能核验](functional-verification.md)。

## 配置与图片

- 对话模型可多选并设默认，写入 Agent 原生配置；进入 Agent 后可切换其他所选兼容模型。
- 文本与图片统一使用用户保存的 Console Key，服务地址为 `https://console.oceanway.tech/v1`。
- 图片 MCP 提供 `get_image_models`、`generate_images`、`get_image_job`、`cancel_image_job`、`retry_image_job` 五个工具，使用 [Oceanway 文档](https://docs.oceanway.tech/docs/)公布的图片协议。
- `get_image_models` 实时查询当前 Key 的全部可用图片模型。用户指定模型时按所选 ID 调用，未指定时使用默认模型；旧勾选清单不限制发现范围。
- 一键配置注册安装后的应用本体作为 MCP 进程，回读 Agent 配置并检查工具发现；生图不依赖开发目录、Python 或独立图片 CLI。
- 重新配置时替换带 Oceanway 标记的托管规则，保留用户原有指令。多张图片一次提交，持续查询同一个任务并陆续返回已完成图片。

## 历史与应用管理

复用 CC Switch 供应商切换和历史管理。Codex 成功配置后开启统一历史；切回已保存的官方或其他供应商时继续保留旧会话。历史兼容保留原文件备份、ID、正文和附件。

首页直接展示 Codex、Claude Desktop、Claude Code、DeepSeek Harness、Hermes 和 Kimi Code Desktop。安装包由 Oceanway 提供，下载显示进度并校验，完成后由用户打开安装包，再检测真实安装结果。安装状态归属对应应用。

侧栏官网入口统一为 `https://oceanway.tech`，用户登录后可到 Console 查看调用情况。

## 验证与后续验收

独立环境已验证 Codex 文本与第二模型切换、一次提交两张真实图片、六次供应商往返后的旧会话可见性和正文哈希；Claude Code 重配后可续聊；Kimi、DeepSeek 在旧会话中完成原生模型切换和全部图片模型查询。具体版本、测试方式和证据见功能核验。

Claude Desktop 与 Hermes 桌面完整图片调用仍需目标环境验收。Hermes CLI 曾在隔离测试目录补充 Python MCP 依赖后查询成功，发行安装环境仍需核对。干净系统安装、升级、卸载及国内网络完整下载按工作包继续验收。

真实测试使用独立 HOME，保留脱敏记录，结束后清理所属进程、配置和临时运行依赖。用户现用 Agent 的配置与历史不得用于测试写入。

## 构建与交付

[build.yml](../.github/workflows/build.yml) 覆盖 Windows x64、macOS Apple Silicon 与 macOS Intel，执行前端与原生回归、安装包构建和包内 MCP 检查。开发分支推送会触发；也可手动运行：

```shell
gh workflow run build.yml --repo Oceanway-AI/codex-config --ref codex/oceanway-config-rebuild
```

三个目标全部通过后，工作流更新 `oceanway-preview` Release，提供 Windows 安装器、两种 Mac ZIP、源码提交、构建链接与 SHA256SUMS。固定下载入口位于 README。

使用新产物核验“下载安装 Agent → 检测 → 填 Key → 多模型配置 → 文本与图片实际调用 → 重启 → 旧会话仍可见”。包内 MCP 的工具列表必须严格为上述五个图片工具；检查使用独立数据目录且不提交收费任务。构建结果和版本验收写入功能核验及文档仓过程记录。
