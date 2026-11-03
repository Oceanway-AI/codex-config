# Oceanway Config 开发交接

日期：2026-11-03。工作包：WP-CODEXCONFIG-001。代码与文档工作分支均为 `codex/oceanway-config-rebuild`。

用户确认由技术人员接手视频 MCP 的 HTTP 413 排查、修复及安装包构建。本次提交使用 `[skip ci]`，保留现有构建工作流，现有 Release 安装包仍为上一版。

## 本次交接内容

- 对话模型可多选并设默认，写入 Agent 原生配置；用户进入 Agent 后可切换其他所选兼容模型。
- 图片 MCP 实时查询当前 Key 的全部图片模型，默认模型与旧勾选清单不限制发现范围。
- 复用 CC Switch 供应商切换和历史管理；修复切回已保存官方/其他供应商的拦截，Codex 成功配置后开启统一历史。
- 补齐 Claude Desktop MCP 持久化、DeepSeek 默认模型与工具发现、Hermes 协议选择，以及 Windows 已安装应用检测。
- 六款应用直接展示，安装状态归属对应应用，官网入口统一为 `https://oceanway.tech`，移除应用内用量统计。

完整结果见 [功能核验](functional-verification.md)；现行 PRD、FS、工作包见 [项目需求入口](PROJECT_REQUIREMENTS.md) 及 [文档仓同名分支](https://github.com/Oceanway-AI/oceanway-docs/tree/codex/oceanway-config-rebuild)。

## 视频 HTTP 413

服务地址为 `https://console.oceanway.tech/v1`，协议依据 [Oceanway 文档](https://docs.oceanway.tech/docs/)。图片、视频与文本统一使用用户在配置工具中保存的 Console Key。

| 检查 | 本轮观察 |
| --- | --- |
| 模型发现 | 同一个 Key 返回 36 个文本、8 个图片、4 个视频模型 |
| Agent 调用 | 独立 Codex 自然语言调用 `get_video_models`、`generate_video`、`get_video_job` |
| 实际生成参数 | `seedance-2.0-mini`，4 秒、480p、16:9，纯文字白色帆船海面场景，无参考文件 |
| 直接 API 复核 | 158 字节最小请求也返回 HTTP 413，耗时约 1.02 秒 |
| 错误 | 外层 `fail_to_fetch_task`，内层 `video_request_too_large`，消息“视频请求体大小超过限制” |
| 结果 | 未取得可用远端任务编号或视频文件 |

服务端日志检索用请求 ID：

```text
202611030438137715554248268d9d6Tnfl5TZO
202611030441470591902738268d9d6vispZPnL
```

直接 API 的脱敏诊断：

```json
{
  "requestBytes": 158,
  "status": 413,
  "requestId": "202611030441470591902738268d9d6vispZPnL",
  "outerCode": "fail_to_fetch_task",
  "innerCode": "video_request_too_large",
  "message": "视频请求体大小超过限制"
}
```

按请求 ID 跟踪 Console 入口、请求体校验与媒体上游转发，核对实际接收的字节数、解析结果、模型参数和限额。小请求复现说明应核对平台处理链，但具体故障点仍需服务日志确认。

代码入口：

- [video_api.rs](../apps/desktop/src-tauri/src/oceanway_images/video_api.rs)：能力校验、请求体构造、`POST /videos`、`GET /videos/{task}`、视频下载与任务保存。
- [mcp_stdio.rs](../apps/desktop/src-tauri/src/oceanway_images/mcp_stdio.rs)：视频模型查询、提交和轮询工具。
- [provider.rs](../apps/desktop/src-tauri/src/oceanway_images/provider.rs)：Console Key、网关与模型目录。
- [MCP 包内检查](../apps/desktop/src-tauri/src/oceanway_images/scripts/mcp_bundle_smoke.py)：检查打包后的工具入口。

修复后以独立 HOME 重测：自然语言发起视频 → 获得任务编号 → 查询同一任务 → 下载并播放结果 → Agent 重启后继续读取原任务。查询与恢复继续使用原任务，不自动重复提交生成。图片、视频、音频 HTTPS 参考保留类型和顺序；本地素材上传入口按已确认安排由平台技术后续补齐。

## 已有验证与待完成项

- Oceanway 原生 140 项、媒体模块 103 项、stdio 5 项、模型前端 25 项通过；TypeScript、renderer build 和 Linux 完整应用本体编译通过。
- 独立 Codex 通过应用自身 MCP 一次提交两张图片，取得两张 1024×1024 PNG。
- Codex 6 次供应商切换后，旧会话默认列表可见且正文不变；Claude Code 重配后旧会话可续聊；Kimi、DeepSeek 在旧会话中完成模型切换及 8 个图片模型查询。
- Claude Desktop 与 Hermes 桌面完整媒体调用仍需验收。Hermes CLI 本轮在测试目录补充 Python MCP 依赖后查询成功，发行安装环境仍需核对该依赖。
- Windows/Mac 最新安装包、干净电脑完整流程及国内网络完整下载仍需完成。Windows 本轮媒体测试通过 WSL 桥接已编译的 Linux 应用，正式打包后需验证随包原生 MCP。
- 官方账号登录和退出按用户确认不追加专项测试；供应商切换后的历史保留继续作为验收项。

本轮独立测试环境已清理，用户现用 Codex、Claude 的配置和登录文件未改动。测试凭据、运行目录、截图与会话原始数据不随源码提交。

## 构建与交付

修复并提交后，在 GitHub Actions 手动运行 **Build Oceanway Config**，选择 `codex/oceanway-config-rebuild` 分支，也可使用：

```shell
gh workflow run build.yml --repo Oceanway-AI/codex-config --ref codex/oceanway-config-rebuild
```

[build.yml](../.github/workflows/build.yml) 已覆盖 Windows x64、macOS Apple Silicon 与 macOS Intel，执行前端/原生测试、安装包构建及包内 MCP 检查。该分支全部构建通过后，工作流会更新 `oceanway-preview` Release；普通后续推送也会触发此工作流。下载入口在 README。

构建产物核对：Windows 安装器、两种 Mac ZIP、对应提交 SHA 和 SHA256SUMS。使用产物在干净系统验收“下载安装 Agent → 检测 → 填 Key → 多模型配置 → 实际文本/图片/视频 → 重启 → 旧会话仍可见”，将结果更新至功能核验和文档仓 PROC-02-028。
