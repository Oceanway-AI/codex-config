# Oceanway Config 需求与功能说明

版本：v1.1 · 更新：2026-11-25 · 负责人：zao

项目：`Oceanway-AI/codex-config` · 分支：`codex/oceanway-config-rebuild`

本文件保留此重建分支原有需求、功能行为、图片 MCP 设计和验收要求。后续提需先在 [Oceanway Config 项目文档](https://docs.oceanway.site/docs/03_项目/codex-config) 查重，沿现行 PRD（DEV-X-07-001）及 FS（DEV-X-07-002）修订，并关联这里的原始章节与采用提交。

阅读入口：[产品与流程](#product) · [功能要求](#features) · [图片 MCP 设计](#image-mcp) · [验收](#acceptance) · [变更记录](#changes)

<a id="product"></a>

## 1. 产品与使用流程

Oceanway Config 是基于 CC Switch 改造的桌面配置工具。客户填入 API Key、选择应用，即可在自己的 Agent 中对话和生图，原有会话继续保留。

沿用 CC Switch 的界面框架、组件、配置适配、MCP 管理和历史处理，统一使用 Oceanway 品牌。官网为 https://oceanway.tech，默认 API 地址为 `https://console.oceanway.tech/v1`，用户可修改地址。产品覆盖文本模型接入和图片生成。

### 1.1 应用范围

| 应用 | 安装形态 | 默认选择的对话模型系列 |
| --- | --- | --- |
| Codex | 桌面版 | GPT |
| Claude Desktop | 桌面版 | Claude |
| Claude Code | 终端版 | Claude |
| DeepSeek Harness | 桌面版 | DeepSeek |
| Hermes | 桌面版 | 当前 Key 可用的兼容推荐模型 |
| Kimi Code Desktop | 桌面版 | Kimi |

目标系统为 Windows x64、macOS Apple Silicon 和 macOS Intel。安装卡片按厂商实际提供的系统、架构和最低版本展示可下载包。六款应用全部直接展示。

### 1.2 三步完成配置

1. **连接 Oceanway**：粘贴 Key，使用默认地址或填写自己的地址，点击“连接并继续”。成功后显示实际地址和连接状态，提供“更换 Key 或地址”。
2. **选择应用**：选择要使用的 Agent。已安装的应用直接配置；未安装的点击“下载安装”，装好后重新检测。对话模型和图片默认模型预先选好，用户可展开调整。
3. **开始使用**：点击“一键配置”，结果页逐个显示完成情况和“打开应用”。进入 Agent 后，用自然语言对话、生成图片或编辑上传的图片。

首次运行 Agent 的欢迎页、系统权限及工具授权按该 Agent 的界面引导完成。

<a id="features"></a>

## 2. 功能要求

| 需求编号 | 功能 | 用户得到的结果 |
| --- | --- | --- |
| CFG-R01 | API 接入 | 默认 Console；地址可编辑、保存、恢复默认，兼容 New API 转接站 |
| CFG-R02 | 打开并加载配置 | 配置后打开对应应用；桌面按需重启，Claude Code 打开新终端 |
| CFG-R03 | 模型多选 | 每个 Agent 可接入多个对话模型、设置默认模型，并在 Agent 内切换 |
| CFG-R04 | 图片 MCP | 自动注册，支持自然语言文生图、本地图生图、链接参考及多张并行生成 |
| CFG-R05 | 历史保留 | 更换服务、Key、模型和重启后，原有会话及附件仍能找到 |
| CFG-R06 | 应用安装 | 从 Oceanway 下载完整安装包，多个应用可同时下载，安装后正确识别 |
| CFG-R07 | Oceanway 界面 | 沿用 CC Switch，简化主流程，统一名称、图标和官网入口 |
| CFG-R08 | 配置与恢复 | 复用 CC Switch 的配置备份、回读、检查和恢复 |
| CFG-R09 | 分发与更新 | 提供 Windows、Mac 安装包，升级保留用户配置和图片任务 |
| CFG-R10 | 变更追踪 | 每次功能改动对应本文件中的需求及验收编号，关联源码提交 |

### 2.1 地址与 Key

- 对话配置、模型列表和图片 MCP 统一使用用户保存的地址与 Key。自定义地址保留端口和路径前缀。
- 修改地址或 Key 后重新获取模型，保留仍可使用的选择。旧配置缺少地址时采用默认 Console。
- Key 输入默认遮罩；本地保存，普通状态和日志使用脱敏信息。
- 连接失败时显示原因及重试入口。只读配置检查展示配置回读、模型目录和 MCP 工具发现结果；真实生成由用户在 Agent 中发起。

### 2.2 多模型选择

- 列表来自当前 Key 的真实模型目录，显示对应 Agent 可用的完整模型集合。按表中系列默认勾选，用户可增减、跨系列选择或添加兼容模型 ID。
- 对话模型支持多选和一个默认值；写入 Agent 的模型列表，用户进入 Agent 后可从原生菜单切换。重新打开配置程序、切换页面和重新检测应用均保留有效选择。
- 图片模型单独选择并设置默认值。MCP 每次查询返回当前 Key 的全部可用图片模型，用户在对话中指定模型；默认值与旧勾选清单作为偏好，不限制模型发现。
- 对话与图片分别分类。无推荐系列时展开模型选择；无图片权限时仍可完成文本配置。

### 2.3 下载、安装与检测

安装包与 Oceanway Config 独立分发，存放于 Oceanway 下载服务器。点击“下载安装”后展示已下载大小、百分比和取消按钮；大小与 SHA256 校验完成后显示“打开安装包”。用户按默认安装向导完成安装，再点击“检测安装结果”。Claude Code 下载后提供“安装 Claude Code”，写入默认用户目录并配置终端路径。

不同应用分别维护下载任务和进度，可以同时下载、取消、重试及打开安装包；同一应用重复点击复用当前任务。页面切换后继续下载，安装状态仅显示在对应 Agent 区域。已下载且校验通过的包可以再次打开。

Windows 使用完整安装器，Mac 使用厂商 DMG、PKG 或应用包。国内直连网络从 Oceanway 服务器获取所需文件。连接超时、下载停滞、文件损坏或安装取消时结束等待，显示原因及对应操作。

检测覆盖默认目录、自定义目录、开始菜单快捷方式、卸载注册信息和实际可执行文件。桌面版核对桌面应用，Claude Code 核对 CLI 版本。重新检测后保留 Key、模型和用户选择，并识别新安装的 Kimi、Hermes 等应用。

### 2.4 一键配置与打开应用

按选中应用执行“检查 → 备份 → 写入 → 回读”，同时注册图片 MCP 和默认调用规则。结果分别显示文本配置、图片工具配置及启动状态，成功项目保留，失败项目提供原因和重新配置入口。

Windows、Apple 芯片 Mac 和 Intel Mac 使用同一流程。已有 Codex 的用户直接配置；未安装的用户从 Oceanway 下载安装包，装好后点“检测安装结果”。检测和启动使用同一份本机桌面应用。Windows 支持系统注册的安装包和指向实际程序的快捷方式；Mac 支持“应用程序”目录和系统已登记的自定义位置。

选中 Codex 时，一键配置先检查当前用户的 Codex 桌面、终端任务和后台进程。即使用户已从托盘退出，也要检查残留进程。发现 Codex 仍在运行时，提示用户保存工作；确认后关闭 Codex，并复查退出时重新启动的后台进程。连续检查确认退出后才写配置；持续被其他程序拉起时，停止配置并提示重试。配置完成后点击“打开 Codex”，从本机安装入口启动，检测到新窗口后才显示成功。如果用户中途又打开了 Codex，先提示保存并重启。其他桌面应用需要重启时先提示保存工作；Claude Code 使用新终端。配置恢复沿用 CC Switch，保留用户后续新增的设置和文件。

### 2.5 历史会话

复用 CC Switch 的历史管理与兼容处理。Codex 采用统一历史能力，Claude Code 保留原项目会话；官方服务、Oceanway 和其他已保存供应商往返切换后，默认历史列表仍可找到原会话。

保留会话 ID、正文、工具记录、附件及原文件备份。必要的供应商标记和索引调整随配置自动完成，用户无需执行迁移。验收以历史完整、可见、可打开为准；用户可使用 Agent 提供的深度链接或分支继续工作。

会话页支持查找、阅读、复制和导出本地记录。已有续聊入口沿用 Agent 的能力；删除记录时明确确认所选会话。

### 2.6 界面与其他功能

侧栏顶部显示醒目的“登录 Oceanway 官网”按钮，打开 https://oceanway.tech。调用用量由官网的 Console 查看。首页直接展示六款应用，模型选择和技术详情按需展开；操作文案使用简短中文。

沿用 CC Switch 的 MCP、Skills、提示词、设置和应用管理。会话、安装管理直接可用，高级功能内提供 MCP、Skills 和提示词管理。每个可见按钮都有对应的实际操作和结果反馈。

窗口、托盘、菜单栏、快捷方式、系统应用列表与安装器统一 Oceanway 名称和图标。关于页展示产品、版本及官网；CC Switch 许可证保留在源码和分发包。Oceanway 使用独立应用标识、数据目录和更新地址。

<a id="image-mcp"></a>

## 3. 图片 MCP 设计

### 3.1 调用链与职责

```text
用户自然语言或上传本地图
  → Agent
  → 本机 oceanway_images MCP
  → 用户保存的 API 地址与 Key
  → 图片模型
  → MCP 保存原图并返回预览
  → Agent 当前对话
```

默认地址直接接 Console。用户填写 New API 地址时，请求发送到该站，由该站转发至 Console；MCP 按这个站点对外提供的 API 协议调用。

参考图片字节随生成请求直接发送。API 参考输入与 Console/Studio 素材库分开：同步请求直接返回图片，异步请求可暂存结果并通过原任务取回。用户只需上传到 Agent，图片 MCP 完成后续处理。

`generate_images` 返回本地任务 ID，用于多图进度、取消和查询。MCP 在后台并行执行同步 HTTP 请求，省略 `async`；服务返回异步任务回执时，按原任务继续查询。

### 3.2 安装与自动注册

配置应用提供随包运行的本地 MCP 可执行组件，支持 Windows 和 Mac。一键配置通过 CC Switch 的对应适配器写入 Agent 的实际 MCP 配置，并添加 Oceanway 管理的生图规则。更新时替换这部分规则，保留用户其他配置。

服务名固定为 `oceanway_images`，传输方式为 `stdio`。现有入口为安装后的应用本体加 `--image-mcp-stdio`；后续实现交付同一工具契约，安装配置记录实际可执行文件绝对路径。

```json
{
  "command": "<安装目录>/oceanway-config.exe",
  "args": ["--image-mcp-stdio"],
  "env": {"OCEANWAY_CONFIG_HOME": "<Oceanway应用数据目录>"}
}
```

上例为服务项，外层结构由各 Agent 适配器生成；Mac 使用对应包内可执行文件路径。凭据在应用数据目录的 `image-provider.json` 中读取，字段为 `version`、`baseUrl`、`apiKey`、`defaultModel`、`allowedModels`。缺省地址采用 Console，模型偏好与实时权限目录共同确定可选值。

配置完成后，回读 Agent 配置并执行 MCP `initialize` 和 `tools/list`，发现下列五个工具后显示图片工具配置完成。stdout 承载 MCP 消息，运行日志写 stderr。最终安装包在无开发环境、含中文和空格的用户路径下运行。

### 3.3 工具契约

| 工具 | 输入 | 返回与用途 |
| --- | --- | --- |
| `get_image_models` | `{}` | `models` 为全部可用图片模型 ID，`default_model` 为保存的默认值或 `null` |
| `generate_images` | 下表中的生成参数 | 建立本地批次，返回任务状态及已完成图片 |
| `get_image_job` | `job_id`；`wait_seconds` 默认 20，范围 0～30 | 查询原批次，进度变化或有新图片时提前返回 |
| `cancel_image_job` | `job_id` | 停止未开始项，已发送请求完成后保存结果 |
| `retry_image_job` | `job_id` | 用户要求重试后恢复可重试的未完成项，保留已成功图片 |

`generate_images` 参数使用 snake_case：

| 字段 | 类型、默认值 | 规则 |
| --- | --- | --- |
| `workspace_directory` | string，必填 | 当前任务工作目录的绝对路径 |
| `prompt` | string | 整批共用描述；填写 `prompts` 时可省略 |
| `prompts` | string[]，默认 `[]` | 每张图片的独立描述，长度等于 `count`，按顺序对应 |
| `model` | string，可省略 | 当前 Key 可用的模型 ID；省略时使用图片默认模型 |
| `count` | integer，默认 1 | 用户请求的总张数，至少 1 |
| `size` | string，默认 `1024x1024` | 输出尺寸；`auto` 表示使用模型默认值 |
| `reference_paths` | string[]，默认 `[]` | Agent 上传附件或上次生成图片的实际绝对路径 |
| `reference_urls` | string[]，默认 `[]` | 用户提供的实际 HTTPS 图片链接 |

图生图示例，一次生成两种修改版本：

```json
{
  "workspace_directory": "C:/Users/Alice/Pictures/Oceanway",
  "model": "gpt-image-2",
  "count": 2,
  "size": "1024x1024",
  "reference_paths": ["C:/Users/Alice/Pictures/boat.png"],
  "prompts": [
    "保留原来的船形和白底，把船帆改成紫色，增加绿色星星。",
    "保留原来的船形和白底，把船帆改成蓝色，增加黄色星星。"
  ]
}
```

### 3.4 模型目录与协议选择

使用客户保存的 Key 读取 `{baseUrl}/models`，返回该 Key 可用的完整图片模型清单。默认 Console 使用图片目录的 `summaries` 分页获取摘要，按模型选择读取 `detail`，并核对价格、能力及连接修订号。公开目录使用匿名请求，短时缓存按修订更新。

相同 Console 模型 ID 经 New API 转接时，可复用 Console 公布的原生请求格式。模型别名采用该站提供的能力或端点元数据。生成、任务查询均使用客户填写的 Base URL 和该站 Key；模型能力中的路径用于拼接用户服务地址。

例如 `https://example.com/customer/v1` 的 Images 入口为 `/customer/v1/images/generations`，Gemini 入口为 `/customer/v1beta/models/{model}:generateContent`。路径前缀和端口保留。

采用 `media_generation_profile.variants[].native_request_variant` 中的协议、方法、路径、编码、默认值、参数与参考图限制。目录中的 `/n`、`/generationConfig/candidateCount` 是 JSON Pointer，实际请求分别为 `{"n":1}` 和 `{"generationConfig":{"candidateCount":1}}`。

### 3.5 文生图与参考图输入

| 协议 | 文生图 | 参考图 |
| --- | --- | --- |
| GPT Images | `POST {baseUrl}/images/generations`，JSON | `POST {baseUrl}/images/edits`，multipart `image[]` 原文件字节，附带 model、prompt、n 及尺寸等参数 |
| Gemini Image | `POST {geminiBase}/models/{model}:generateContent`，JSON | `contents[].parts[].inlineData` 携带 MIME 与原文件 Base64 |
| Grok Image | `POST {baseUrl}/images/generations`，JSON，尺寸转换为 `aspect_ratio` 与 `resolution` | 按发布的模型能力显示支持范围；2026-11-25 目录的两款 Grok 为文生图 |

以上格式用于 Console 当前公布的模型，以及 New API 对这些模型的相应协议转发。其他兼容站的图片别名沿该站的 Images 或 Gemini 协议调用。

本地附件读取原图字节。Console 原生编辑的链接参考图先下载并核对图片格式，再与本地图一起通过上述方式发送。保留输入顺序，检查数量、MIME、单图与总字节限制。原附件保留，生成结果另存。

GPT 文生图核心请求：

```json
{"model":"gpt-image-2","prompt":"一艘橙色帆船，白色背景","n":1,"size":"1024x1024"}
```

GPT 编辑的 multipart 表单包含 `model=gpt-image-2`、`prompt=保留船形，把帆改成紫色`、`n=1`、`size=1024x1024`、`image[]=原图文件`；请求边界由 HTTP 客户端生成。

Gemini 参考图请求：

```json
{
  "contents": [{"role":"user","parts":[
    {"text":"保留原船形，把帆改成紫色"},
    {"inlineData":{"mimeType":"image/png","data":"<原图Base64>"}}
  ]}],
  "generationConfig":{"candidateCount":1,"responseModalities":["TEXT","IMAGE"]}
}
```

尺寸按所选模型转换为原生字段。每个请求生成一张；用户要求多张时由 MCP 队列并行处理。目录中的 GPT/Grok `n` 可接受 1～10，Gemini 当前 `candidateCount` 为 1。

### 3.6 同步与异步结果

默认发送同步生成请求，原图字节随请求传递。MCP 在后台执行，通过 `get_image_job` 返回进度、预览和保存位置。

同步 OpenAI Images 读取 `data[].b64_json` 或 `data[].url`；Gemini 读取 `candidates[].content.parts[].inlineData`，兼容 `inline_data`。优先保存响应中已有的图片，核对格式、尺寸和 SHA256。

异步响应保存 `X-Task-ID`、同站 `Location`、服务地址、原 Key 指纹与请求编号。使用原 Key 查询 `Location`，完成后读取该任务的 `outputs/{index}/content`。新版常见查询路径为 `/v1/tasks/{id}`，已有 `/images/{id}` 回执继续可恢复。自定义站的任务读取沿该站地址进行。

同步等待返回 HTTP 503/504 并带有效任务地址时，查询原任务取回结果。服务返回的轮询间隔按 `Retry-After` 执行。任务恢复继续查询和下载原结果。错误记录 HTTP 状态、错误码及请求编号，成功图片继续展示。

### 3.7 批量、状态与恢复

用户请求多张图时，Agent 一次调用 `generate_images(count=N)`；需要不同内容时传入 N 条 `prompts`。本地队列单个 MCP 进程同时执行最多 2 个生成请求，每个请求生成一张，空闲后自动接下一张。任何一张完成即保存并回传，其余任务继续。

本地任务状态沿用：`running`、`completed`、`completed_with_warnings`、`partial`、`failed`、`paused`、`interrupted`、`cancelled`。单项另记录排队、执行、远端待完成、成功、失败或结果未知。返回总数、完成数、失败数及每项文件或错误。

`get_image_job` 等待默认 20 秒、最长 30 秒；有本会话尚未展示的新图片时立即返回。同一 MCP 会话按图片去重展示。上游查询遵循 `Retry-After`，普通查询逐步延长间隔至 15 秒。

用户取消时停止排队项，已发送项完成后保存。失去 Agent 连接时停止派发新请求；重启读取任务记录，已取得远端 ID 的任务继续查询下载，其余未完成项由用户请求重试。结果未知项先核对原任务；成功图片在重试中复用。

### 3.8 保存与回传

原图保存到 `<workspace_directory>/output/images/<job_id>/`，记录文件路径、格式、尺寸、字节数、SHA256、参考文件哈希和请求编号。每张图独立落盘，任务记录支持重启恢复。

工具响应同时提供 `structuredContent` 和 MCP `content`。输入字段采用 snake_case；任务输出沿用 camelCase，主要字段如下：

```json
{
  "id":"image-example",
  "status":"running",
  "model":"gpt-image-2",
  "mode":"edit",
  "total":2,
  "completed":1,
  "failed":0,
  "cancelled":0,
  "outputDirectory":"C:/Users/Alice/Pictures/Oceanway/output/images/image-example",
  "items":[{
    "index":1,
    "status":"completed",
    "outputs":[{"path":"C:/Users/Alice/Pictures/Oceanway/output/images/image-example/1.png","width":1024,"height":1024,"bytes":123456,"sha256":"<原图SHA256>"}]
  }]
}
```

`content` 中包含文字摘要与 `type:"image"` 图片预览，图片块携带 `mimeType` 和 Base64 `data`。预览按消息大小缩小，原图字节保持完整；本轮未回传的图片可在后续查询取得。图形 Agent 在原对话显示图片，终端提供可打开的本地路径。

### 3.9 Agent 默认调用规则

配置程序写入的规则应让 Agent 按以下顺序操作：

1. 用户要生成或修改图片时，使用 `oceanway_images`。
2. 未指定模型采用默认图片模型；用户要查看或切换时调用 `get_image_models`，按所选 ID 生成。
3. 本地附件传实际文件路径，链接传真实 URL；编辑要求作为 prompt。
4. 多张图一次提交总数量，持续查询同一批次，图片完成一张展示一张。
5. 展示生成图片、保存位置和未完成项原因；用户要求重试时调用原批次的重试工具。

用户首次授权 MCP 后，即可继续用自然语言完成上述流程。

<a id="acceptance"></a>

## 4. 验收要求

| 编号 | 场景与通过标准 |
| --- | --- |
| CFG-A01～06、08～09 | 配置回读正确、原设置有备份、恢复保留新增内容；Key 遮罩、诊断脱敏，检查与真实调用分别展示 |
| CFG-A11、33～35、52 | 默认 Console 与自定义 New API 都能通过实际 Agent 文本对话；地址和 Key 修改后使用新目录，保存重开及自定义路径前缀正确 |
| CFG-A12～14、44 | 默认家族正确，对话模型可多选、设默认、在 Agent 内切换；图片目录包含 Key 的全部模型，指定模型原样调用 |
| CFG-A15～16 | 配置后逐应用打开；桌面加载新配置，Claude Code 新终端可用，首次引导有清晰步骤。Codex 在 Windows、Apple 芯片 Mac、Intel Mac 均验收两条路径：从 Oceanway 新装、使用已有安装。后台残留能清理；重启后旧进程退出且出现新窗口 |
| CFG-A17～18、40～41 | 全新电脑安装应用后自动注册 MCP，重开 Agent 可发现五个工具；自然语言生成的实际图片回到原会话 |
| CFG-A19、53 | 对 Console 与 New API 各自实时目录中的每款图片模型，逐一验证文生图、本地参考图和支持的链接参考；核对实际图片、指定模型、原图内容及修改效果 |
| CFG-A20、42 | 一次请求多张，至少两个生成请求同时执行，完成即回传；取消、重启、查询及失败重试保留已有结果、复用原任务 |
| CFG-A21、54 | 同步直接返回与异步取回均完成实际图片保存；参考字节随 API 请求发送，API 使用不依赖用户素材库额度；图片失败时文本仍可使用 |
| CFG-A07、22、27、36～39 | 官方、Oceanway、其他已保存供应商往返切换后，旧会话默认可见、ID 和正文完整、附件可打开，兼容前备份可恢复 |
| CFG-A43、46～49、51 | 国内直连下载对应完整包；两款应用同时下载、独立进度和取消；默认安装后重新检测成功，Kimi/Hermes 和自定义路径可识别 |
| CFG-A31、45、50 | 六款应用直接展示；安装状态属于对应应用；官网按钮有效；所有窗口、托盘及系统名称为 Oceanway，所有可见功能实际可用 |
| CFG-A10、28～30 | Windows、Mac 对应架构完成安装、升级、恢复和卸载；升级保留 Key、模型、任务，卸载保留 Agent 配置、历史与生成图片 |
| CFG-A32 | 功能变动在本文件更新，提交引用需求编号与对应验收记录 |

测试使用独立配置目录和 Agent 进程。每次记录实际版本、服务地址、模型 ID、请求/任务编号、结果文件及通过情况；保留脱敏证据，清理临时进程、测试安装和凭据。

图片全模型验收先完成 Console，再通过 New API 转接重测；自然语言端到端验收至少覆盖 Codex 与 Claude Code，其余四款应用逐项核对 MCP 注册、调用和图片展示。平台接口更新后重测受影响项。发布前完成客户端接入验证，模型能力与渠道状态记录在功能核验中，由平台维护。

## 5. 实现入口与维护

| 内容 | 当前代码位置 |
| --- | --- |
| CC Switch 桌面主体 | `apps/desktop/` |
| Oceanway 配置与 Agent 接入 | `apps/desktop/src-tauri/src/oceanway.rs` |
| 图片模型、凭据及端点 | `apps/desktop/src-tauri/src/oceanway_images/provider.rs` |
| Console 公开能力与修订缓存 | `apps/desktop/src-tauri/src/oceanway_images/catalog.rs` |
| MCP 五个工具和消息 | `apps/desktop/src-tauri/src/oceanway_images/mcp_stdio.rs` |
| 图片协议转换 | `apps/desktop/src-tauri/src/oceanway_images/image_protocol.rs` |
| 本地批次、文件与恢复 | `apps/desktop/src-tauri/src/oceanway_images/image_api.rs`、`image_engine_store.rs` |
| 远端异步任务 | `apps/desktop/src-tauri/src/oceanway_images/async_images.rs` |

构建命令见 [开发交接](DEVELOPMENT_HANDOFF.md)，实际结果见 [功能核验](functional-verification.md)，客户操作见 [使用说明](USER_GUIDE.md)。新增 MCP 实现按本文件第 3、4 章交付，接入配置程序时保持五个工具及任务字段兼容。

<a id="changes"></a>

## 6. 变更记录

| 版本 | 日期 | 变动 |
| --- | --- | --- |
| v1.1 | 2026-11-25 | 对齐 DEV-X-07-002 v1.2，更新原生请求能力、GPT multipart、Gemini 参数、Grok 公开能力和任务 Location；Console 与 New API 分别使用原站 Key，验收沿 CFG-A19/20/21/53/54 |
| v1.0 | 2026-11-04 | 将需求、功能、MCP 设计和验收合并至 codex-config 本文件；整理六款应用、模型多选、并行下载、自定义地址、历史保留及 API 直接生图流程 |

沿用 CFG-R01～10 和现有 CFG-A 编号；CFG-A53 为全部图片模型逐项验收，CFG-A54 为 API 图片处理与素材库分离验收。后续每项调整记录改动内容、对应编号及源码提交。
