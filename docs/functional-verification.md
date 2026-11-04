# Oceanway Config 功能核验

更新：2026-11-04。工作包：WP-CODEXCONFIG-001。分支：`codex/oceanway-config-rebuild`。

## 本轮结果

使用用户授权的 Console 和 New API 测试 Key，实时拉取目录并实际调用全部文本、图片模型。视频不进入配置或测试。

| 项目 | Console | model.ocean-way.top |
| --- | --- | --- |
| 实时目录 | 36 文本、8 图片 | 36 文本、9 图片 |
| 文本实际回复 | 35/36 | 35/36 |
| 每款图片模型文生图 | 8/8 | 8/9 |
| 每款图片模型本地参考图 | 6/8 | 6/9 |
| Codex 自然语言一次生成两张 | 前轮通过 | 前轮通过 |
| Codex 本地附件一次编辑两张 | 通过 | 通过 |

两站文本失败项都是 `claude-sonnet-4-5`，返回 HTTP 503 / `model_not_found` / 无可用渠道。文本按各模型实际协议请求 Responses、Messages、Chat Completions 或 Gemini；初测协议错误由正确协议复测结果覆盖。

### 图片逐模型结果

| 图片模型 | Console 文生图 | Console 本地图 | New API 文生图 | New API 本地图 |
| --- | --- | --- | --- | --- |
| gemini-3-pro-image-preview | 通过 | 通过 | 通过 | 通过 |
| gemini-3.1-flash-image | 目录无此模型 | 目录无此模型 | 503 无可用渠道 | 503 无可用渠道 |
| gemini-3.1-flash-image-preview | 通过 | 通过 | 通过 | 通过 |
| gemini-nano-banana-2.1 | 通过 | 通过 | 通过 | 通过 |
| gpt-image-2 | 通过 | 通过 | 通过 | 通过 |
| gpt-image-2.5-flare | 通过 | 通过 | 通过 | 通过 |
| gpt-image-2.5-sunburst | 通过 | 通过 | 通过 | 通过 |
| grok-imagine-image | 通过 | 仅公布链接参考图 | 通过 | HTTP 400 能力校验失败 |
| grok-imagine-image-2.0 | 通过 | 仅公布链接参考图 | 通过 | HTTP 400 能力校验失败 |

原参考图为白底橙色帆船。成功编辑结果保留帆船主体和白底，按要求改成紫色船帆并增加绿色星星；原附件 SHA256 不变。逐模型的 28 张成功输出和 Codex 双站的 4 张附件编辑结果均核对实际文件、格式、尺寸、哈希、MCP 预览及视觉内容。

Console 更新后，三款 GPT Image 的原生本地编辑已通；New API 转发同样成功。Grok 的两款模型仍只有生成端点和 `reference_images` 链接输入。直接发送原文件 multipart，以及 xAI 官方 `image.url` data URI JSON，均返回 HTTP 400：`Media parameters do not match the published capability`，未返回任务回执。额外图片 ID `gemini-3.1-flash-image` 在 New API 返回 503；带 `-preview` 的型号正常。

## 新 Console 适配

MCP 已改为默认同步 HTTP 请求，省略 `async`，本机后台任务继续并行处理图片。新的 GPT 编辑能力使用 `/v1/images/edits`，支持 multipart 原文件及 JSON `images[].image_url` 的 data URI/链接；Gemini 使用原生 `inlineData`。移除强制 `response_format:url`、本地图上传前置限制和通用 OpenAI 请求中的旧 `reference_images` 字段。Grok 按公开能力保留其仍在使用的字段。

Console 新能力中的数量参数按 `parameter_bindings` 读取；可选文件大小限制按实际声明处理。同步响应已含图片时直接保存，`X-Task-ID` 不再误当待取图任务。同步交付返回 503/504 且带有匹配的同站任务地址时，保存回执并查询原任务，回归确认只有一次生成 POST。

以下 PR 已于 2026-11-04 合并，本轮使用更新后的线上服务复测：

- [Console PR #29](https://github.com/Oceanway-AI/oceanway-platform/pull/29)，提交 `559a1e049025816db9521bc59ad29beb45e22658`。
- [Media PR #13](https://github.com/Oceanway-AI/temp-media-gateway/pull/13)，提交 `3fb6f739f57eee6da23aabd38aaba56874045d64`。

新版请求结构已用这两个提交的真实契约完成本地回归，GPT/Gemini 的真实参考图请求已通过。

### Grok 本地图修复点

11 月 4 日 11:40（北京时间）再次复测：Console 与 `model.ocean-way.top` 的两款 Grok，分别使用 xAI 原生 JSON 和标准 Images 编辑输入，共 8 次本地图请求均返回 HTTP 400：`Media parameters do not match the published capability`，没有图片或任务回执。公开能力 revision 仍为 `bd8ed0e3273a487248aa0bf90809cbb0088cd2454f16d65e8cdeda011edfddbe`，仅含生成端点和链接参考图。此次接口证据保存在 `.artifacts/grok-recheck-20261104/local-edits-matrix.json`。

当前失败发生在 Console 的能力校验阶段。xAI 官方支持 `POST /v1/images/edits` 的 JSON `image: {"type":"image_url","url":"data:image/png;base64,..."}`；单张参考图可直接随请求发送。官方不接受 OpenAI SDK 使用的 multipart 编辑格式。

服务端需补齐三处：

1. Media 的 [officialGrokImageMetadata](https://github.com/Oceanway-AI/temp-media-gateway/blob/647fbbf5200cfa3003f45fbe4e1ff4bb229843f5/service/image_model_metadata_xai.go#L13) 当前只返回 `/v1/images/generations`；为两款 Grok 增加真实的编辑端点、参考图格式和数量声明。[mediaImageReferenceInputs](https://github.com/Oceanway-AI/temp-media-gateway/blob/647fbbf5200cfa3003f45fbe4e1ff4bb229843f5/service/media_generation_profile.go#L405) 当前仍声明 `operation=generate`、`transport=url-array`。
2. Console 的 [ValidatePlatformMediaParametersForPath](https://github.com/Oceanway-AI/oceanway-platform/blob/c97926f975f306d47356f0af928cec79c0dcb0e8/service/platform_media_parameter_validation.go#L39) 按发布端点校验，当前 Grok 的 `/images/edits` 无匹配项；同步编辑能力，并支持对应的 Base64 参考图校验。现有 [链接转换](https://github.com/Oceanway-AI/oceanway-platform/blob/c97926f975f306d47356f0af928cec79c0dcb0e8/controller/platform_xai_reference_image_request.go#L35) 仅处理生成入口的 HTTPS 链接。
3. 与当前 MCP 对接的建议契约是对外提供标准 Images 编辑输入：JSON `images[].image_url` 和 multipart `image[]`。Media 按所选 Grok 渠道转换为 xAI 的 JSON `image` / `images`，保留原图字节；New API 继续转发相同请求。若服务选择直接开放 xAI 原生格式，需一并发布该参考输入声明并调整 MCP 的序列化。

Grok 的客户端请求格式和服务端能力声明需要一致。同步请求省略 `async`，本地图随请求传递，结果直接返回。修复后复测两站、两款 Grok 的文生图、本地图、批量和 Codex 附件流程。

官方依据：[xAI Image Editing](https://docs.x.ai/developers/model-capabilities/images/editing)。复现请求、400 响应和服务端源码快照保存在本轮证据目录的 `direct-edits/` 与 `source-review/`。

## 真实应用与历史

在独立 Windows HOME 中操作现有 11 月 3 日原生应用：填写地址和 Key、拉模型、多选、点击一键配置。空白 Codex 配置成功，自动注册 `oceanway_images`；独立 Codex 0.166.0 的模型菜单返回所选 7 个 GPT，真实对话回复成功。

随后从 New API 切到 Console，再切回 New API。每次通过应用页面保存并重开独立 Codex，默认历史列表均能找到原会话，保存前后正文哈希一致。原会话在两个服务下都能回复此前暗号“蓝鲸842”。

最初带其他供应商旧会话的测试目录在当前 Codex 仍运行时触发退出提示；该次配置保留原会话。之后另用空白 Codex 目录验证新用户流程。既有跨官方供应商的完整往返验收沿用前轮记录。

Claude Desktop、DeepSeek Harness、Hermes、Kimi 的 New API 地址、模型和图片 MCP 已从真实配置文件回读。Claude Desktop 使用 `configLibrary` 中的 Gateway 配置，Kimi 的 MCP 位于单独 `mcp.json`。本轮完整自然语言与图片验收使用 Codex；其他 Agent 的实际对话记录见前轮证据。

Codex 图片测试使用最新源码编译的独立 MCP。当前轮以产品实际写入的 `[features] image_generation=false` 配置测试：Console 本地附件一次 `count=2` 约 87 秒完成，New API 约 74 秒完成。均使用用户指定的 `gpt-image-2`，一次 MCP 批次完成两张编辑，通过 `get_image_job` 取得真实文件和两张预览，原附件与 MCP 接收字节的哈希一致。

本轮初次 New API 自然语言测试遗漏了上述产品配置，Agent 未调用 MCP、未返回图片，记录为失败。补齐测试环境配置后使用新运行标识复测通过，原失败证据保留；产品本身已有该配置。

Windows 测试沿用本机代理，WSL 的 New API 测试经临时 loopback 桥接；Console 附件双图测试采用 WSL 直连。本轮不是国内网络全面连通性验收。

## 回归、证据与发布

- 图片模块 114 项、自定义地址子进程 1 项、stdio 5 项，共 **120 项通过**。
- 本轮线上更新复测证据：`.artifacts/console-update-retest-20261104/`。
- `text-native-matrix.json`：两站 72 个文本请求，70 个成功。
- `all-model-mcp-acceptance/{console,newapi}/post-deploy/run.json`：两站逐模型图片结果。
- `codex-local-attachment-*-product-config-result.json`、对应事件与图片：独立 Agent 附件双图调用。
- `console-visual-results.jpg`、`newapi-visual-results.jpg`、`agent-outputs-review.jpg`：真实输出对照；`output-files-audit.json`：32 张图片的文件校验。
- `capability-summary.json`、`server-pr-status.json`、`direct-edits/`、`source-review/`：线上能力与 Grok 复现证据。
- GUI、历史和 120 项回归证据在 `.artifacts/full-flow-retest-20261104/` 的 `gui-*`、`tests.log`；本轮未改变产品源码。

本次发布包含已验证的 Console / New API 图片协议适配，由 GitHub Actions 构建 Windows x64、macOS Apple Silicon 和 macOS Intel，三平台检查完成后更新 `oceanway-preview`。Grok 本地图能力与上述无可用渠道的模型由平台继续处理。安装包对应提交和构建结果以 [GitHub Release](https://github.com/Oceanway-AI/codex-config/releases/tag/oceanway-preview) 为准。统一需求与 MCP 设计见 [PROJECT_REQUIREMENTS.md](PROJECT_REQUIREMENTS.md#image-mcp)。

前轮测试 GUI 进程树已清理。Windows 两个临时目录的递归删除被自动审批以 `blocked by policy` 拦截，目录仍保留，位置记录在 `.artifacts/full-flow-retest-20261104/cleanup-result.json`。本轮环境清理、配置哈希和凭据扫描结果记录在本轮目录的 `cleanup-result.json`。

前轮记录保存在 `.artifacts/install-acceptance-20261103/`、`.artifacts/images-only-build-20261103/` 和 `.artifacts/concurrent-download-baseurl-20261103/`，包括其他 Agent 对话、六次供应商切换、并行下载和安装包验收。Windows 已有真实安装记录；Mac 实机与 Hermes/Claude Desktop 的完整桌面验收仍未完成。
