# Oceanway Config 功能核验

更新：2026-11-03。工作包：WP-CODEXCONFIG-001。开发分支：`codex/oceanway-config-rebuild`。三平台安装包与对应源码、构建记录见 [oceanway-preview](https://github.com/Oceanway-AI/codex-config/releases/tag/oceanway-preview)。

## 当前用户可见功能

| 入口 | 功能 |
| --- | --- |
| 登录 Oceanway 官网 | 打开 https://oceanway.tech，登录后进入 Console 查看调用情况 |
| 连接 Oceanway | 默认 Console，支持自定义服务地址；使用保存的地址与 Key 获取实时文本和图片目录 |
| 六款应用 | Codex、Claude Desktop、Claude Code、DeepSeek Harness、Hermes、Kimi Code Desktop 全部直接展示 |
| 模型选择 | 多选兼容对话模型、设置默认模型；所选模型写入对应 Agent 配置 |
| 下载安装 | 不同应用可同时从 Oceanway 下载完整包，独立进度、取消和重试；校验后由用户打开安装包，再检测安装结果 |
| 会话 | 从本机 Agent 读取历史，搜索、阅读、复制、导出；删除需用户确认 |
| 高级功能 | 沿用 CC Switch 的 MCP、Skills、提示词管理 |
| 设置 | 外观、语言、应用目录、网络、配置备份、导出和日志 |

## 并行下载与自定义地址回归（2026-11-03）

全量前端 280 个文件、3565 项通过；TypeScript 与渲染器构建通过。WSL 原生 Oceanway 服务 142 项、独立图片模块 100 项、MCP 子进程 6 项、Codex 配置投影 9 项通过。真实软件下载测试仍按既有规则只在一次性运行器执行。

本地 HTTP 测试确认两个原生下载任务同时接收字节，取消一个后另一个继续并校验完成。前端使用实际任务控制器验证独立进度、取消、打开安装包与页面切换。自定义地址覆盖模型查询、六款 Agent 配置、Codex 空白身份与 Key 轮换、图片 MCP 保存读回及旧记录兼容。独立 MCP 进程使用本地兼容 API 查询全部图片模型并生成两张实际 PNG。

默认 Console 真实模型查询仍返回 36 个文本与 8 个图片模型；额外 4 个视频模型继续不进入配置流程，该查询没有提交付费生成。现用 Codex/Claude 四个配置文件 SHA256 未变。完整证据在 `.artifacts/concurrent-download-baseurl-20261103/`，文档仓记录为 PROC-02-030。更新安装包通过开发分支构建发布，状态以 Release 对应源码与运行记录为准。

用户指定的自定义站点 `https://model.ocean-way.top` 实际返回 49 个模型，其中 36 个文本、9 个图片、4 个视频。Windows 应用从 GUI 保存该地址与 Key 后，独立 Codex 进程真实完成文本对话。随后发现图片兼容错误：同步图片响应同时带有 `X-Task-ID`，旧 MCP 把该编号当成待查询任务，而站点没有对应查询路由，返回 HTTP 404。使用同一地址与 Key 直接请求标准图片接口，40.86 秒返回一张 1024×1024 PNG，证明此次问题不是该图片渠道无法生成。

修复后优先保存响应内的图片；自定义地址使用标准 Images 请求，默认 Console 继续按公开能力请求异步任务。回归覆盖同步图片带任务头、正文含任务编号、Gemini 内联图片及无效图片；已有异步恢复测试继续检查不会重复生成。成品在 Codex 中的复测及发布结果登记至 PROC-02-030。

## 上轮 Windows 安装版验收

[构建 41613224876](https://github.com/Oceanway-AI/codex-config/actions/runs/41613224876) 的 Windows x64、macOS Apple Silicon、macOS Intel 与发布任务全部成功，下载后的三个包均匹配发布的 SHA256。Mac 完成构建和包内 MCP 检查，尚未实机操作验收。

Windows 使用下载的 NSIS 安装器实际安装，在全新独立 HOME 中从 GUI 粘贴 Console Key，选择 7 个 GPT 对话模型和 8 个图片模型后点击一键配置。应用自动保存 API 身份并注册图片 MCP，没有手工补登录文件。原生模型目录包含所选 7 个 GPT。

首次欢迎与 Windows 设置按客户端引导完成；设置检查曾短暂失败，点击重试后输入框恢复。桌面新会话真实回复“Oceanway 连接成功”。随后只用自然语言要求两张图片，Agent 自动调用 `oceanway_images.generate_images`，一次提交 `count=2`，查询同一任务后在原会话返回两张 1024×1024 PNG。图片预览与系统另存为均实测。

再次一键配置时首次遇到 Windows 原子文件替换错误 `os error 1175`，返回调整后重试成功，根因尚未确认。重新配置前后原会话正文 SHA256 一致；重开桌面后聊天和图片仍可见，原会话正确返回先前暗号“蓝鲸731”。本轮是 Oceanway 重配，前轮跨供应商往返结果见下文。

新站使用的简明 Word 手册为 6 页、14 张真实截图，覆盖安装、Key、模型多选、配置、首次引导、对话和生图。WPS 原生打开与导出、OOXML 检查、全部页面视觉检查通过。15 页详细稿保留在本地，不作为默认用户手册。

本轮证据位于 `.artifacts/images-only-build-20261103/`：`final-desktop-acceptance.json`、`final-gui-config-result.json`、`final-model-catalog-result.json`、`final-history-reconfigure.json`、`final-ui-image-download.json`、`qa/final-delivery-audit.json`、`qa/quickstart/` 和 `delivery/`。

独立测试进程、HOME、工作目录和本轮临时安装已清理。当前用户 Codex/Claude 的四个配置及身份文件 SHA256 均未改变；99 个保留文本文件未检出测试 Key。`old-plugin-temp` 仅剩空测试目录，其删除被自动审批以 `blocked by policy` 拦截，未绕过，仍保留。

## 前轮源码真实接入结果

前轮测试使用独立的 Windows/WSL HOME 和用户指定的 Console 测试 Key。Windows 当前用户的 Codex、Claude 配置与登录文件用 SHA256 核对。原生配置由生产 writer 生成；前轮 Windows 媒体进程临时桥接到已编译的 Linux 应用本体，以适应本机缺少 MSVC 的环境。本轮安装包使用随包的 Windows 原生 MCP。

| 应用 | 文本 | 模型与图片工具 | 历史 |
| --- | --- | --- | --- |
| Codex 0.166.0 app-server | 默认模型及第二个模型 `gpt-6-sol` 均真实回复 | 原生目录列出所选 7 个 GPT；自然语言一次调用 MCP 生成 2 张真实图片 | 官方、Oceanway、其他已保存供应商共 6 次切换，默认列表可见，正文不变；旧真实会话可续聊 |
| Claude Code 2.1.316 | 真实回复 | 自然语言调用 MCP 返回全部 8 个图片模型 | 重新配置后旧会话能回复原暗号 |
| DeepSeek Harness Desktop 0.2.1-rc.5 | 默认模型及菜单切换后的 `deepseek-v4-flash` 均真实回复 | 原生菜单显示 3 个 DeepSeek 模型；自然语言 MCP 查询返回全部 8 个图片模型 | 重开后旧会话可见，并在旧会话继续完成模型切换测试 |
| Kimi Code Desktop | 默认 Kimi 及菜单切换后的 `deepseek-v4.1-flash` 均真实回复 | 原生菜单显示 11 个所选兼容模型；自然语言 MCP 查询返回全部 8 个图片模型 | 重开后旧会话可见并继续对话 |
| Hermes CLI 0.22.x | 真实回复 | 在隔离目录补充其缺失的 Python MCP 依赖后，自然语言查询返回全部 8 个图片模型 | 原配置写入保留会话目录；桌面版完整验收待完成 |
| Claude Desktop 1.37937.3 | Gateway 真实回复，审计记录为 Sonnet 5.5 | MCP 注册和重启持久化通过；完整桌面工具调用与原生模型菜单仍待验收 | 隔离旧任务仍可见 |

Claude Desktop 的 Cowork 隔离测试遇到系统虚拟机服务与隔离用户目录不一致，未修改真实用户目录或系统服务。Hermes 的隔离依赖补充不是发行安装器的修复；发行版需单独验收。

## 历史保留

复用 CC Switch 原生适配器及 Codex 统一历史能力，支持切回已保存的官方或其他供应商。接入默认使用 Console，也支持用户填写的兼容服务地址。统一历史设置在配置成功后开启，配置预检查失败时保留此前设置。

Codex 往返顺序为 Oceanway → 官方 → Oceanway → 其他已保存供应商 → 官方 → Oceanway。每次重开独立 app-server，调用未指定供应商筛选参数的默认 `thread/list`。旧会话均可见，正文 SHA256 一致。历史兼容只调整必要的供应商元数据，保留原文件备份、ID、正文和附件引用。

Windows 重新应用配置时，Claude、Kimi、DeepSeek 共 8 个历史文件逐字节不变。Claude Code 用原 session ID 续聊，正确返回切换前暗号。会话页沿用本地读取；Claude Desktop 入口目前显示本地 Claude Code 记录。

切换测试覆盖原生配置路由与默认历史列表。原地续聊不是本期必过条件。

## 图片

当前同一个 Console Key 的可用目录中包含 **36 个文本模型、8 个图片模型**。分类和协议以平台公开能力为准，权限以 Key 的模型目录为准。

`get_image_models` 实时查询这个 Key 的全部图片模型，并返回默认模型；用户可在对话中指定目录内任意模型，旧的默认值和已选清单不再限制发现范围。DeepSeek 的 MCP 工具发现补齐缓存字段后，官方 SDK 及桌面会话均通过。

独立 Codex 调用已编译应用自身的 `--image-mcp-stdio`，一次 `generate_images` 提交两张图片，持续查询原任务，101.81 秒取得两张 1024×1024 PNG；没有重新提交生成请求。图片证据保存在本地验收目录。

## 检查与证据

| 检查 | 结果 |
| --- | --- |
| 前轮 Oceanway 原生定向回归 | 140 项通过，3 项手动验收忽略 |
| 图片模块与独立 stdio | 当前 98 项模块测试、5 项 stdio 测试通过；工具列表严格为 5 个图片工具 |
| 模型选择前端回归 | 当前 26 项通过；Key 含其他能力时，文本与图片配置仍可完成 |
| TypeScript 与 renderer build | 通过 |
| Linux 完整应用本体编译 | 通过 |
| Windows/Mac 新安装包 | 三平台构建、包内 MCP 检查和发布成功；Windows 实际安装及桌面验收通过上述流程 |

前轮源码证据目录为 `.artifacts/install-acceptance-20261103/`：

- `provider-history-roundtrip.json`：6 次原生供应商切换。
- `history-switch-preservation.json`、`history-after-switch.json`：Codex 旧会话保留和真实续聊。
- `windows-reconfigure-history.json`、`claude-after-reconfigure-resume.json`：Windows 历史哈希与 Claude 续聊。
- `codex-native-model-switch.json`：原生目录和第二个模型的真实回复。
- `native-desktop-session-results.json`、`native-session-evidence/`：Kimi、DeepSeek 原生旧会话中的模型切换与完整图片目录。
- `claude-native-mcp.json`、`hermes-native-mcp.json`、`dsh-sdk-mcp-result.json`：图片目录发现。
- `native-app-media-results.json`：自然语言批量图片生成。
- `cleanup-windows-result.json`、`cleanup-wsl-result.json`、`final-cleanup-audit.json`：前轮隔离环境清理、凭据检查与当前用户配置哈希。

前轮测试进程、独立 HOME、临时运行依赖及 WSL 验收目录已清理；脱敏证据与两张生成图片保留。前轮当前用户的四个 Codex/Claude 配置和登录文件 SHA256 与测试前一致，189 个保留文本文件未检出测试 Key。

国内下载已有电信、联通、移动节点的分段读取证据；本机公司/校园 Wi-Fi 直连仍有握手超时，不能据分段读取判定所有地区完整下载已通过。最新全功能、全目标平台安装验收仍未完成，工作包保持 doing。
