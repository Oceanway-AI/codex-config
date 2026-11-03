# Oceanway 原生图片服务

复用 `Oceanway-AI/codex-config` 固定提交 `17107d20aca57115033c4f20889097921d62b2f0` 的图片队列、结果持久化和原生 MCP；由 CC Switch 原有 MCP 适配器负责安装到各 Agent。没有 Python/Node 运行依赖。

## 集成接口

所有接口返回 `Result<_, String>`，错误消息不回显 API Key 或服务端响应正文。

- `configure(home, api_key, default_model: Option<&str>, allowed_models: &[String]) -> ProviderSummary`：同步保存一份应用自有凭据和图片模型配置。`home` 必须是 Oceanway 应用数据目录；允许没有图片模型，此时生成操作返回明确提示。
- `configure_at(home, base_url, api_key, default_model, allowed_models)`：保存用户选定的服务地址；默认使用 Console，旧记录缺少地址字段时也使用 Console。
- `read_provider_summary(home) -> ProviderSummary`：仅返回 endpoint、keyConfigured、defaultImageModel、allowedImageModels，不返回 Key。
- `read_api_key(home) -> String`：仅供 Rust 后端给 Agent 写配置，不可注册为 IPC command。
- `ImageJobs::with_home(home)`：初始化并读取本服务拥有的图片任务记录，不发网络请求，不启动生成。
- `ImageJobs::{start,status,cancel,retry,capabilities,result_path}`：普通同步 Rust API；Tauri 应在 blocking task 中包装。
- `mcp_server_spec(home) -> serde_json::Value`：返回当前安装程序的 stdio 配置，供 CC Switch 的 `McpService` 安装；配置无 Key。
- `mcp_stdio::check(executable, home)`：异步握手和五个工具发现；子进程自动设置 `OCEANWAY_MCP_CHECK_ONLY=1`，只验证目录，不恢复、改写或取消已有图片任务，并拒绝全部 `tools/call`。
- `run_stdio_if_requested() -> Option<Result<(), String>>`：在 Tauri/窗口/单实例逻辑之前调用；`Some` 表示已处理 `--image-mcp-stdio`，调用者应依据结果退出。

MCP 环境变量只有 `OCEANWAY_CONFIG_HOME`，指向同一应用数据目录；不使用 `CODEX_HOME`、Claude 配置路径或聊天数据库。标准输出仅用于 MCP 协议。

Linux AppImage 使用持久化的 AppImage 文件作为启动入口，避免记录退出后会消失的临时挂载路径；其他安装方式使用当前程序路径。

## 行为边界

- 默认地址为 `https://console.oceanway.tech/v1`，按平台公开能力调用 OpenAI/Gemini 图片接口。用户可自定义 HTTP/HTTPS 服务地址；自定义服务使用该地址下的 `/images/generations` 与 `/images/edits`，模型查询使用 `/models`。
- `ImageTestRequest.model` 可省略/留空；提交时使用保存的图片模型，指定其他模型时查询当前服务的可用图片清单。
- 五个工具为 `get_image_models`、`generate_images`、`get_image_job`、`cancel_image_job`、`retry_image_job`；所有 Agent 共用同一服务，图片模型独立于对话模型。
- 每次请求 `n=1`、进程级并发最多 2；取消停止尚未开始的任务，已经开始的请求完成后保留结果；不自动重试。
- 多张图片应由 Agent 一次提交 `count` 和可选的逐张 `prompts`；空闲 worker 自动取下一张，已完成图片可先读取和展示，不必等待最慢的一张。工具说明明确要求整批提交，避免 Agent 逐张调用后才等待下一张。
- `get_image_job` 首次遇到本次 MCP 会话尚未返回的已完成图片时立即返回；已经回传过的图片不会造成重复快速轮询，无新结果时继续等待进度变化或超时。
- 重启只恢复拥有的图片任务索引，未完成任务需要明确重试，不重复计费生成。
- 原始聊天记录、Agent 认证文件和历史索引不会被本模块读取、移动、删除或改写。
- 图片输出保留原文件、hash、尺寸和进度；MCP 返回可显示的图片预览与保存路径。

## 依赖和验证

准确依赖见 `test-harness/Cargo.toml`。最低 Rust 1.89（使用标准文件锁）；不依赖 Tauri、SQLite、TOML 或旧程序的全局 helper。

独立验证：

```sh
cargo test --manifest-path apps/desktop/src-tauri/src/oceanway_images/test-harness/Cargo.toml
```

测试只使用临时目录和 loopback mock；fixture 端点字段及配置写入辅助函数只在 `cfg(test)` 中编译，生产程序不存在该开关。保留原实现的恢复、锁、取消竞争、显式重试、Key 更新、编辑请求、下载验证测试，并增加应用凭据隔离、可选图片模型与端点限制检查。

`allowed_models` 必须来自后端当前 API Key 的模型发现结果。模型被 `/models` 列出只证明可见，不代表图片生成已被实测；正式生成需要用户/Agent 发起明确请求。

打包后应对实际应用可执行文件执行零计费检查：

```sh
python apps/desktop/src-tauri/src/oceanway_images/scripts/mcp_bundle_smoke.py --executable <built-application-executable>
```

脚本在中文空格路径内建立临时应用数据、合法格式的假 Key 和空图片模型，并从不同工作目录、没有开发工具的 `PATH` 启动实际程序，检查 initialize、四个工具、标准输出和 EOF 退出。只进行工具发现，不调用任何生成工具。它检查真实产物是否包含 MCP 入口，不代替 Agent 自然语言触发验收。Python 仅用于 CI 检查，用户运行应用不需要 Python。

2026-11-02 验证：以正式端点 `https://console.oceanway.tech/v1` 的固定配置构建，WSL Ubuntu / Rust 1.94.1 下 90 项单元测试及 4 项真实子进程 stdio 测试全部通过。新增测试将引擎放入独立子进程、mock API 留在父进程，证明 4 张任务同时最多发送 2 个请求，并在第一张仍阻塞时返回其他图片的真实文件、hash 和预览。实际 rmcp client 通过 `tools/call` 生成和查询的测试证明：某张在 poll 前完成时，即使设置等待 20 秒，也会在 1 秒断言范围内返回图片；已回传后会继续等待下一张，不重复预览。自检子进程对有效待恢复任务进行工具发现并拒绝工具调用后，manifest/index 字节保持不变，没有新增锁文件；普通启动阳性对照确认该记录确实会被正常恢复。另外检查已复制到中文空格路径的可执行文件及缺失可执行文件报错。零计费脚本也在 Linux native harness 产物上通过。请求仍全部使用本地 mock；没有使用客户 Key。此结果验证原生模块，不代替 Windows/macOS 安装包与真实服务联调。
