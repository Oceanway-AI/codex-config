# Oceanway 原生图片服务

复用 `Oceanway-AI/codex-config` 固定提交 `17107d20aca57115033c4f20889097921d62b2f0` 的图片队列、结果持久化和原生 MCP；由 CC Switch 原有 MCP 适配器负责安装到各 Agent。没有 Python/Node 运行依赖。

## 集成接口

所有接口返回 `Result<_, String>`，错误消息不回显 API Key 或服务端响应正文。

- `configure(home, api_key, default_model: Option<&str>, allowed_models: &[String]) -> ProviderSummary`：同步保存一份应用自有凭据和图片模型配置。`home` 必须是 Oceanway 应用数据目录；允许没有图片模型，此时生成操作返回明确提示。
- `read_provider_summary(home) -> ProviderSummary`：仅返回 endpoint、keyConfigured、defaultImageModel、allowedImageModels，不返回 Key。
- `read_api_key(home) -> String`：仅供 Rust 后端给 Agent 写配置，不可注册为 IPC command。
- `ImageJobs::with_home(home)`：初始化并读取本服务拥有的图片任务记录，不发网络请求，不启动生成。
- `ImageJobs::{start,status,cancel,retry,capabilities,result_path}`：普通同步 Rust API；Tauri 应在 blocking task 中包装。
- `mcp_server_spec(home) -> serde_json::Value`：返回当前安装程序的 stdio 配置，供 CC Switch 的 `McpService` 安装；配置无 Key。
- `mcp_stdio::check(executable, home)`：异步握手和四个工具发现，不生成图片。
- `run_stdio_if_requested() -> Option<Result<(), String>>`：在 Tauri/窗口/单实例逻辑之前调用；`Some` 表示已处理 `--image-mcp-stdio`，调用者应依据结果退出。

MCP 环境变量只有 `OCEANWAY_CONFIG_HOME`，指向同一应用数据目录；不使用 `CODEX_HOME`、Claude 配置路径或聊天数据库。标准输出仅用于 MCP 协议。

Linux AppImage 使用持久化的 AppImage 文件作为启动入口，避免记录退出后会消失的临时挂载路径；其他安装方式使用当前程序路径。

## 行为边界

- 生产图片请求固定为 `https://console.oceanway.tech/v1/images/generations` 或 `/images/edits`；生产配置不接受任意端点。
- `ImageTestRequest.model` 可省略/留空；提交时使用保存的图片模型，明确指定时必须在保存的可用图片模型列表中。
- 四个工具为 `generate_images`、`get_image_job`、`cancel_image_job`、`retry_image_job`；所有 Agent 共用同一服务，图片模型独立于对话模型。
- 每次请求 `n=1`、进程级并发最多 2；取消停止尚未开始的任务，已经开始的请求完成后保留结果；不自动重试。
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

2026-11-02 验证：以正式端点 `https://console.oceanway.tech/v1` 的固定配置构建，WSL Ubuntu / Rust 1.94.1 下 87 项单元测试及 1 项真实子进程 stdio 握手测试全部通过。请求仍全部使用本地 mock；没有使用客户 Key。此结果验证原生模块，不代替 Windows 安装包与真实服务联调。
