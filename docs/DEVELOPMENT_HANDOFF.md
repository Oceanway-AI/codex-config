# Oceanway Config 开发交接

日期：2026-11-03。工作包：WP-CODEXCONFIG-001。代码与文档工作分支均为 `codex/oceanway-config-rebuild`。

当前范围为 Oceanway 文本模型配置、图片 MCP、六款应用安装管理和历史保留。需求正文见 [项目需求入口](PROJECT_REQUIREMENTS.md)，实际验证见 [功能核验](functional-verification.md)。

## 配置与图片

- 对话模型可多选并设默认，写入 Agent 原生配置；进入 Agent 后可切换其他所选兼容模型。
- 文本与图片统一使用用户保存的地址和 Key。地址默认 `https://console.oceanway.tech/v1`，支持自定义和恢复默认；旧记录缺少地址时仍使用 Console。修改地址或 Key 后重新查询模型。
- 自定义地址从认证的 `/models` 读取目录，Agent adapters 和图片 MCP 使用同一保存地址；Anthropic 适配去除末尾 `/v1`，保留路径前缀。自定义生图使用 OpenAI Images 兼容协议。
- 图片 MCP 提供 `get_image_models`、`generate_images`、`get_image_job`、`cancel_image_job`、`retry_image_job` 五个工具，使用 [Oceanway 文档](https://docs.oceanway.tech/docs/)公布的图片协议。
- `get_image_models` 实时查询当前 Key 的全部可用图片模型。用户指定模型时按所选 ID 调用，未指定时使用默认模型；旧勾选清单不限制发现范围。
- 一键配置注册安装后的应用本体作为 MCP 进程，回读 Agent 配置并检查工具发现；生图不依赖开发目录、Python 或独立图片 CLI。
- 重新配置时替换带 Oceanway 标记的托管规则，保留用户原有指令。多张图片一次提交，持续查询同一个任务并陆续返回已完成图片。

## 历史与应用管理

复用 CC Switch 供应商切换和历史管理。Codex 成功配置后开启统一历史；切回已保存的官方或其他供应商时继续保留旧会话。历史兼容保留原文件备份、ID、正文和附件。

首页直接展示 Codex、Claude Desktop、Claude Code、DeepSeek Harness、Hermes 和 Kimi Code Desktop。安装包由 Oceanway 提供，下载显示进度并校验，完成后由用户打开安装包，再检测真实安装结果。安装状态归属对应应用。

下载按应用独立管理，不同应用可同时进行，分别轮询、取消和重试；同一应用重复点击复用任务。打开安装包和检测结果不阻塞其他下载，切换页面保留任务。

侧栏官网入口统一为 `https://oceanway.tech`，用户登录后可到 Console 查看调用情况。

## 验证与后续验收

最新 Windows 安装包已实际安装，从空白独立 HOME 经 GUI 保存 Key、7 个 GPT 和 8 个图片模型，正常完成 Codex 首次引导。桌面真实文本回复、自然语言一次生成两张图、图片预览下载、重配后历史与旧会话续聊通过。新用户 API 身份由 CC Switch guarded 写入事务初始化，覆盖新用户、重复配置与 Key 轮换，并保留原官方身份。

重配首次出现 `os error 1175` 文件替换失败，GUI 重试成功，原因未确认。首次 Windows 设置检查也曾需要点击重试。具体记录见功能核验；不要将这些情况记为已修复。

独立环境已验证 Codex 文本与第二模型切换、一次提交两张真实图片、六次供应商往返后的旧会话可见性和正文哈希；Claude Code 重配后可续聊；Kimi、DeepSeek 在旧会话中完成原生模型切换和全部图片模型查询。具体版本、测试方式和证据见功能核验。

Claude Desktop 与 Hermes 桌面完整图片调用仍需目标环境验收。Hermes CLI 曾在隔离测试目录补充 Python MCP 依赖后查询成功，发行安装环境仍需核对。干净系统安装、升级、卸载及国内网络完整下载按工作包继续验收。

真实测试使用独立 HOME，保留脱敏记录，结束后清理所属进程、配置和临时运行依赖。用户现用 Agent 的配置与历史不得用于测试写入。

## 构建与交付

源码 `3d60b9f3670466de87b3f2774158b6a1e880ea7c` 已通过 [41613224876](https://github.com/Oceanway-AI/codex-config/actions/runs/41613224876) 的三个平台构建与发布任务，产物在 [oceanway-preview](https://github.com/Oceanway-AI/codex-config/releases/tag/oceanway-preview)。Windows 已完成安装版桌面测试；Mac 目前仅完成 CI 构建与包内 MCP 检查。

本地 `.artifacts/images-only-build-20261103/delivery/` 保存已校验的三个安装包与 `Oceanway_Config_简明操作手册_20261103.docx`。简明手册 6 页、14 张真实操作截图，WPS 原生检查和逐页视觉检查通过。完整验收写入文档仓 PROC-02-029。测试环境已清理，自动审批拦截的 `old-plugin-temp` 空目录保留。

[build.yml](../.github/workflows/build.yml) 覆盖 Windows x64、macOS Apple Silicon 与 macOS Intel，执行前端与原生回归、安装包构建和包内 MCP 检查。开发分支推送会触发；也可手动运行：

```shell
gh workflow run build.yml --repo Oceanway-AI/codex-config --ref codex/oceanway-config-rebuild
```

三个目标全部通过后，工作流更新 `oceanway-preview` Release，提供 Windows 安装器、两种 Mac ZIP、源码提交、构建链接与 SHA256SUMS。固定下载入口位于 README。

使用新产物核验“下载安装 Agent → 检测 → 填 Key → 多模型配置 → 文本与图片实际调用 → 重启 → 旧会话仍可见”。包内 MCP 的工具列表必须严格为上述五个图片工具；检查使用独立数据目录且不提交收费任务。构建结果和版本验收写入功能核验及文档仓过程记录。
