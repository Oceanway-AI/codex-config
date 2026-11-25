# Oceanway 原生图片模块

产品需求和完整 MCP 设计统一见 [Oceanway Config 需求与功能说明](../../../../../docs/PROJECT_REQUIREMENTS.md#image-mcp)。本页提供代码入口与本地验证命令。

## 集成入口

| 接口 | 用途 |
| --- | --- |
| `configure` / `configure_at` | 保存默认或自定义 API 地址、Key 与图片模型偏好 |
| `read_provider_summary` | 返回地址、Key 配置状态和模型偏好 |
| `read_api_key` | Rust 后端读取凭据，供 Agent 配置写入 |
| `ImageJobs::{start,status,cancel,retry,capabilities,result_path}` | 图片批次与结果管理 |
| `mcp_server_spec` | 生成安装后的 stdio 启动配置 |
| `mcp_stdio::check` | 独立子进程握手与五个工具发现 |
| `run_stdio_if_requested` | 在图形应用及单实例初始化前处理 `--image-mcp-stdio` |

模块使用 `OCEANWAY_CONFIG_HOME` 指定的应用数据目录。安装后的应用本体提供 MCP 入口，用户运行环境无需 Python 或 Node。依赖见 `test-harness/Cargo.toml`。

## 本地验证

```shell
cargo test --manifest-path apps/desktop/src-tauri/src/oceanway_images/test-harness/Cargo.toml
```

测试使用临时目录和本地模拟 API。打包后检查实际可执行文件：

```shell
python apps/desktop/src-tauri/src/oceanway_images/scripts/mcp_bundle_smoke.py --executable <built-application-executable>
```

该脚本在中文、空格路径及独立数据目录执行 MCP 握手、工具发现和退出检查。Python 用于构建检查。真实 API 和 Agent 对话结果统一记录在 [功能核验](../../../../../docs/functional-verification.md)。
