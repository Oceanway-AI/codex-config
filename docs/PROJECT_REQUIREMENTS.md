# Oceanway Config 需求与功能说明

版本：v1.0 · 更新：2026-11-04 · 负责人：zao

项目：`Oceanway-AI/codex-config` · 分支：`codex/oceanway-config-rebuild`

本文件统一维护产品需求、功能行为、图片 MCP 设计和验收要求。后续功能调整直接更新对应章节，在文末记录版本与改动。

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

用户点击“打开应用”后加载新配置。Codex 等桌面应用需要重启时先提示保存工作，再正常退出并打开；Claude Code 使用新终端。配置恢复沿用 CC Switch，保留用户后续新增的设置和文件。

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

**本机 MCP 的批次任务与 HTTP 同步/异步是两层机制。** `generate_images` 返回本地任务 ID，用于多图进度、取消和查询；HTTP 是否携带 `async:true` 取决于所选 API 的公开协议。同步 HTTP 请求也可以在 MCP 后台并行执行。

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

用保存的 Key 请求 `{baseUrl}/models`，API 请求使用 `Authorization: Bearer <apiKey>`。图片能力依据目录中的端点元数据和 Console 的公开模型能力识别，包含 OpenAI Images、Gemini、Grok 和站点提供的图片模型别名。返回真实完整清单，模型 ID 原样传递。

Console 能力说明来自 [图片文档](https://docs.oceanway.tech/docs/images)、[任务文档](https://docs.oceanway.tech/docs/tasks)及公开模型能力。New API 以其模型目录、端点元数据和对外响应为准。目录获取与生成统一使用用户保存的站点地址。

地址示例：`https://example.com/customer/v1` 的 Images 地址为 `/customer/v1/images/generations`，Gemini 地址为 `/customer/v1beta/models/{model}:generateContent`；保留 `/customer` 前缀。图片数量、参考图数量、MIME 类型、尺寸和分辨率按所选模型支持的参数校验。

### 3.5 文生图与参考图输入

| 协议 | 文生图 | 本地参考图 | 链接参考图 |
| --- | --- | --- | --- |
| OpenAI Images | `POST {baseUrl}/images/generations`，JSON | `POST {baseUrl}/images/edits`，multipart 的 `image` 或 `image[]` 文件字段 | 采用服务公布的 URL 输入；支持 Console 扩展时使用 `reference_images` |
| Gemini | `POST {geminiBase}/models/{model}:generateContent` | `contents[].parts[].inlineData` 携带 MIME 和原文件 Base64 | 支持时使用 `fileData.fileUri` |
| Grok / xAI | 使用该服务公布的 Images 生成接口 | 原生 xAI 编辑使用 `image: {type: "image_url", url: "data:image/...;base64,..."}`；网关公布标准 Images 编辑时按该协议发送 | 使用该服务公布的 `image.url` 或 `reference_images` |

MCP 读取附件原文件并保留参考顺序。原始文件用于 API 输入，缩略图仅用于结果展示。本地文件和链接混用时，适配器完整转换全部输入；当前接口无法表达时返回具体缺少的输入方式，让用户调整参考格式。

OpenAI 文生图核心请求体：

```json
{"model":"gpt-image-2","prompt":"一艘橙色帆船，白色背景","n":1,"size":"1024x1024"}
```

本地编辑使用同一模型、描述、单张数量及尺寸，并附 multipart 文件原字节。Gemini 核心输入如下，其中 Base64 为原图字节：

```json
{
  "contents": [{"role":"user","parts":[
    {"text":"保留原船形，把帆改成紫色"},
    {"inlineData":{"mimeType":"image/png","data":"<原图Base64>"}}
  ]}],
  "generationConfig":{"candidateCount":1,"responseModalities":["TEXT","IMAGE"]}
}
```

Gemini 的尺寸按模型能力转换为 `generationConfig.imageConfig.aspectRatio` 和 `imageSize`。Grok 采用当前服务公布的编辑字段和参考数量。MCP 始终使用用户选定模型处理实际参考内容，生成结果另存，原附件保留。

### 3.6 同步与异步结果

**同步返回**：OpenAI Images 读取 `data[].b64_json` 或 `data[].url`；Gemini 读取 `candidates[].content.parts[].inlineData`，兼容 `inline_data`。解码或下载原图，校验可读取后保存。响应已经包含图片时优先接收图片，`X-Task-ID` 作为请求跟踪信息记录。

**异步返回**：接口明确支持异步时才发送相应参数。返回任务回执后保存服务地址、原任务 ID、查询协议和请求编号；随后查询这个原任务。Console Images 使用 `GET {baseUrl}/images/{id}`，完成后读取 `/images/{id}/outputs/{index}/content`；Gemini 采用对应 `/v1beta/images/{id}` 路径。New API 使用它实际开放的查询协议；仅有跟踪头时按实际响应内容处理。

生成 POST 与结果查询分别记录。查询和断线恢复复用原任务；接口返回的错误保留 HTTP 状态、错误码和有用原因，API Key 脱敏。查询地址缺失或结果未知时向 Agent 返回当前状态与原任务信息。参数或配置错误通过 MCP `isError:true` 与文字说明返回；批次执行错误记入对应 `items[].error`，让已成功图片继续回传。

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
| CFG-A15～16 | 配置后逐应用打开；桌面加载新配置，Claude Code 新终端可用，首次引导有清晰步骤 |
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

图片全模型验收先完成 Console，再通过 New API 转接重测；自然语言端到端验收至少覆盖 Codex 与 Claude Code，其余四款应用逐项核对 MCP 注册、调用和图片展示。平台接口更新后重测受影响项，实际接入通过后再构建安装包。

## 5. 实现入口与维护

| 内容 | 当前代码位置 |
| --- | --- |
| CC Switch 桌面主体 | `apps/desktop/` |
| Oceanway 配置与 Agent 接入 | `apps/desktop/src-tauri/src/oceanway.rs` |
| 图片模型、凭据及端点 | `apps/desktop/src-tauri/src/oceanway_images/provider.rs` |
| MCP 五个工具和消息 | `apps/desktop/src-tauri/src/oceanway_images/mcp_stdio.rs` |
| 图片协议转换 | `apps/desktop/src-tauri/src/oceanway_images/image_protocol.rs` |
| 本地批次、文件与恢复 | `apps/desktop/src-tauri/src/oceanway_images/image_api.rs`、`image_engine_store.rs` |
| 远端异步任务 | `apps/desktop/src-tauri/src/oceanway_images/async_images.rs` |

构建命令见 [开发交接](DEVELOPMENT_HANDOFF.md)，实际结果见 [功能核验](functional-verification.md)，客户操作见 [使用说明](USER_GUIDE.md)。新增 MCP 实现按本文件第 3、4 章交付，接入配置程序时保持五个工具及任务字段兼容。

<a id="changes"></a>

## 6. 变更记录

| 版本 | 日期 | 变动 |
| --- | --- | --- |
| v1.0 | 2026-11-04 | 将需求、功能、MCP 设计和验收合并至 codex-config 本文件；整理六款应用、模型多选、并行下载、自定义地址、历史保留及 API 直接生图流程 |

沿用 CFG-R01～10 和现有 CFG-A 编号；CFG-A53 为全部图片模型逐项验收，CFG-A54 为 API 图片处理与素材库分离验收。后续每项调整记录改动内容、对应编号及源码提交。
