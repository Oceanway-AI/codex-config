# Oceanway Config 功能核验

日期：2026-11-02。对应 WP-CODEXCONFIG-001；源码基于 CC Switch。本文登记当前用户可见功能、验证结果与待验收项。

## 可见功能

| 入口     | 用户能做什么                                                                                | 当前核验                                                                                                        |
| -------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| MCP      | 添加、编辑、删除、从应用导入；按应用启用/停用，批量同步、重试失败项                         | 已核对 `UnifiedMcpPanel` 与 MCP API 链路，纳入全量前端回归；四端注册、Gemini 适配回读及默认生图规则通过原生测试 |
| Skills   | 发现与安装技能、从应用或 ZIP 导入；管理安装目标、卸载、备份恢复和来源仓库                   | 已核对 `SkillsPage`、`UnifiedSkillsPanel`，纳入全量前端回归                                                     |
| 提示词   | 新增、编辑、启用、删除、导入；复制内容或复制到其他应用                                      | 全量前端回归通过；已启用 Oceanway 图片 MCP 时，编辑、切换、停用、删除提示词及恢复配置保留默认生图规则           |
| 用量统计 | 按应用、日期筛选；查看请求、Token、费用估算、趋势与模型统计，调整刷新及显示方式             | 已核对 `UsagePage`、`UsageDashboard`；费用来自本地用量和定价，不代表账户实时账单                                |
| 设置     | 语言、外观、终端、应用目录、网络代理、配置导出、备份与日志                                  | 已核对 `SettingsPage`，纳入全量前端回归；测试区分 Oceanway 当前入口和上游多供应商模式                           |
| 应用     | 查看安装/版本状态、官方安装入口、升级、卸载、重新检测与侧栏显示；配置 Oceanway 后打开 Agent | 原生接入范围为 Codex、Claude Code、Gemini CLI、OpenCode、OpenClaw；其他应用显示接入状态，安装管理仍保留         |
| 会话     | 应用筛选、项目/时间分组、搜索、消息阅读、正文查找、复制/导出、恢复命令、确认删除            | 6 个前端文件 96 项通过；原生读取与历史兼容测试见下文                                                            |

图片 MCP 自动注册支持 Codex、Claude Code、Gemini CLI、OpenCode。OpenClaw 当前支持对话接入，图片功能尚未适配；安装管理与 Oceanway 接入的支持范围分别显示。

## 会话与历史

会话页读取 Agent 本机保存的历史，不需要把记录上传到 Oceanway。链路为 `sessionsApi.list` → `list_sessions` → `session_manager::scan_sessions`；阅读为 `get_session_messages` / `stream_session_messages` → `load_transcript`，图片和工具长输出按需加载。

当前解析来源为 Claude Code、Codex、OpenCode、OpenClaw、Gemini CLI、Hermes、Grok Build、Pi、Oh My Pi、MiniMax Code、Kimi Code、DeepSeek Harness、Antigravity CLI；是否有记录取决于本机对应目录。Claude Desktop 入口复用 Claude Code 记录。Codex 同时扫描 `sessions` 和 `archived_sessions`，支持 `.jsonl` 与 `.jsonl.zst`，不会按旧 API 或 Oceanway 供应商过滤。Hermes 数据库列表最多读取最近 500 条。

列表搜索覆盖标题、摘要、ID 和目录；进入阅读页后可查找正文、查看图片和工具步骤、复制单轮或整段 Markdown、导出文件。Windows/Linux 提供恢复命令复制，macOS 支持打开终端运行恢复命令。当前没有会话深链接或“新分支”按钮。

扫描、阅读与复制是读取操作。导出只写用户选择的文件；删除必须确认，直接删除对应 Agent 本地记录。Oceanway 配置另有历史兼容步骤：先核验备份，再规范 Codex 供应商元数据，使旧记录继续可见；会话 ID、正文、附件及归档状态保留。该步骤位于 `codex_history_migration.rs`，不由会话页触发。

本轮修复：

1. 隐藏侧栏应用后，已有历史仍出现在会话页的应用筛选及“全部应用”中。
2. 列表读取请求失败显示错误与重试，不再显示“没有记录”或提示刷新成功；已有列表仍可查看。
3. 流式阅读缺少结束消息或消息未收全时显示读取失败，不再把部分内容当成完整会话。

## 已执行验证

在 `apps/desktop` 执行：

```powershell
pnpm exec vitest run tests/components/SessionManagerPage.test.tsx tests/components/SessionReader.test.tsx tests/components/sessionUtils.test.ts tests/lib/sessionsApi.test.ts tests/lib/sessionsQuery.test.tsx tests/types/sessionFixtures.test.ts
pnpm typecheck
pnpm exec prettier --check src/components/sessions/SessionManagerPage.tsx src/lib/query/sessions.ts tests/components/SessionManagerPage.test.tsx tests/lib/sessionsQuery.test.tsx
```

会话相关测试 96/96 通过（21、22、6、3、19、25），typecheck 和格式检查通过；`git diff --check` 通过。测试使用模拟 IPC 和隔离 fixture。结果保存在本地 `.artifacts/session-audit/tests.json` 与 `tests.log`。

本轮全量前端回归为 275 个文件、3536/3536 通过；typecheck、renderer build 通过。命令为 `pnpm exec vitest run --maxWorkers=4 --minWorkers=1`，结果保存在 `.artifacts/functional-audit/frontend-final.json`。末尾未适配应用提示页新增 2 个用例，定向 22/22 通过，记录为 `.artifacts/functional-audit/unsupported-app-pages.log`；合计 3538 个用例在这两批回归中通过。

原生测试代码已核对：

| 测试                                                                                                           | 覆盖                                                               |
| -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `scan_sessions_in_roots_includes_active_and_archived_files`                                                    | Codex 活动和归档记录都出现；扫描和阅读后原文件字节不变             |
| `load_messages_reads_compressed_rollout_identically`                                                           | 压缩记录与普通记录解析一致，读取后压缩文件字节不变                 |
| `scanning_and_reading_preserve_session_and_attachments`                                                        | Claude 扫描、阅读后会话 ID、正文和附件保持不变                     |
| `codex_claude_configuration_preserves_history_and_old_login_fixture`                                           | 配置 Oceanway 后 Codex/Claude 历史及已有登录信息保留               |
| `oceanway_history_unifies_all_sources_with_content_and_backups_intact`                                         | 官方/旧 API/归档历史的正文、附件引用、ID、数据库归档状态及备份核验 |
| `oceanway_history_*failure*`、`oceanway_history_partial_publication_and_startup_recovery_follow_live_provider` | 历史预检查、配置失败回滚、追加消息和启动恢复                       |

前三项本轮增强或新增，验证命令调用的扫描和消息解析函数，均直接使用临时目录；Codex 扫描测试不再读取默认用户目录的标题索引。Tauri 命令封装已核对，完整 GUI 到原生 IPC 的读取仍需安装包验收。以下命令在隔离 WSL 源码副本的 `src-tauri` 目录执行，`CC_SWITCH_TEST_HOME` 指向临时目录。Oceanway 原生 112/112、Codex 会话 34/34、Claude 会话 19/19、提示词服务 6/6、Codex 配置投影 9/9，共 180/180 通过。

```sh
cargo test --locked --lib oceanway -- --test-threads=1
cargo test --locked --lib session_manager::providers::codex::tests -- --test-threads=1
cargo test --locked --lib session_manager::providers::claude::tests -- --test-threads=1
cargo test --locked --lib services::prompt::tests -- --test-threads=1
cargo test --locked --lib live::project::codex::tests -- --test-threads=1
```

图片链路本轮修复了 Gemini 对 MCP timeout/type 规范化后的回读误判，保留提示词操作后的默认图片规则，并通过原子配置写入 Codex 的 `features.image_generation=false`，保留其他功能设置。隔离 Codex 0.166.0 的 `features list` 已确认该开关可用。

当前生产地址源码在隔离 WSL 中执行 `cargo build --locked --features tauri/custom-protocol --bin oceanway-config` 成功。最新 Linux 二进制在 bwrap 隔离环境通过七项 stdio smoke：初始化、四工具发现、EOF 退出、中文空格 home、不同工作目录、无运行时 PATH 和应用数据不变；未调用工具生图，图片 POST 为 0。结果与源码哈希见 `.artifacts/mcp-audit-20261102/final-tests.json`。该步骤未启动 GUI。

独立 Codex 对图片测试服务的自然语言生图回归已完成：64.78 秒，一次提交两张图片，最大并发为 2，结果分两条消息返回；未调用其他工具，当前 Codex 配置哈希保持不变。记录为 `.artifacts/gui-e2e/evidence/mcp-regression-20261102.json`、`mcp-regression-20261102-gateway.json`、`mcp-regression-20261102-isolation.json`，图片在同目录的 `mcp-regression-20261102/1.png` 与 `2.png`。该测试使用隔离 QA 端点，不代表主服务图片权限已开放。

## 空环境安装与完整 Codex 流程（2026-11-02）

本轮从没有 Node/npm 和 Agent 的临时 HOME 开始，在 WSL 的 bubblewrap 隔离空间运行真实 Tauri GUI，通过页面按钮执行安装、连接和配置。Windows 挂载与原 Linux 用户目录不可见。

| 验证 | 结果 |
| --- | --- |
| Apps 页实际安装 | Codex 0.166.0、Claude Code 2.1.316、Gemini CLI 0.66.0、OpenCode 1.19.5、OpenClaw 2026.10.9、Grok Build 1.0.70、Pi 1.4.0、MiniMax Code 0.7.4、Kimi Code 2.4.1、DeepSeek Harness 0.2.1-rc.5、Oh My Pi 18.15.0、Hermes 0.22.5，均安装后识别版本 |
| 首页直接安装 | 第二个空 HOME 点击“安装 Codex”，56.95 秒安装完成，显示安装中状态并自动识别；不依赖 Node，未使用 API Key |
| 安装修复 | Codex 改用官方独立安装器；需要 Node 的 Agent 自动安装并校验官方运行环境；Hermes 使用官方非交互选项；失败时显示错误并允许重试 |
| 连接与默认模型 | 无效 Key 返回 401；真实测试目录共 47 个模型；Codex 默认 gpt-6.1-sol、Claude Code 默认 claude-sonnet-5-5，生图默认 gpt-image-2 |
| 文本对话 | GUI 保存的配置交给独立 Codex，三轮通过，用时 4.26、5.77、4.18 秒 |
| 自然语言生图 | 两轮均自动调用 Oceanway MCP，每轮一次提交两张，最大并发 2，共 4 张 1024×1024 PNG；用时 42.59、57.87 秒，已完成图片陆续返回，其他工具调用为 0 |
| MCP 自动注册 | 配置指向应用自身 `--image-mcp-stdio`，携带应用目录，写入默认图片指令并关闭 Codex 内置 image_generation；测试未人工修改 GUI 生成的配置 |
| 旧历史保留 | 官方、旧 API、归档三种测试会话的 ID、正文、归档位置保留，原文件备份一致；独立 Codex 默认/归档列表可见，重开 GUI 后仍可查阅原文 |
| 主机配置 | 当前 Windows Codex 与 Claude 的 5 个配置/登录文件 SHA256 全部保持不变 |

图片测试使用用户指定的临时图片 Key；QA-only 本地转发将两组测试服务接到同一流程。正式主服务 Key 的图片权限仍需服务开通后复验。安装版本检测通过不代表所有 Agent 的聊天和图片兼容性都已验收；本轮完整对话链路只测 Codex。Claude Desktop 保留官网下载入口。

前端安装相关 86 项、App 集成 26 项、原生安装命令 166 项及运行环境 2 项通过；Windows PowerShell 运行环境复用与退出码检查通过。测试证据、截图和图片 SHA256 在本地 `.artifacts/isolated-e2e-204abf6/evidence/`，不包含 Key。测试环境、Agent 安装和凭据在验收后删除，清理结果随证据保存。

安装包的固定下载入口在分支 README 顶部，完整 CI 通过后更新 `oceanway-preview` 测试版；发布页标明对应源码提交。

## 待验收

- 204abf6 的构建 41401338172 已产出 Windows x64 和 macOS arm64 安装包，Windows 原生 MCP 七项检查通过；macOS Intel 前端测试超时。c0e61ae 的构建 41438362540 暴露 macOS 脚本执行和 Windows/WSL 命令选择问题，已修正并提交后续 CI。完整 Windows GUI、实际运行安装程序和另一台干净电脑接入仍需验证。
- MCP/Skills 的外部服务和安装来源、提示词写入所有 Agent、系统安装/卸载、macOS 终端恢复、费用与账户实际账单未逐客户端完成真实 GUI 验收。
- 本轮未访问真实用户会话正文，未改动当前 Codex 配置或进程；客户真实旧账号历史可见性与旧会话继续聊天尚未作为全客户端通过项。
- 上游扫描按可解析文件返回列表；部分目录或单文件读取失败会记日志并跳过，尚未逐项在页面展示诊断。辅助子 Agent 记录按上游规则省略。


## 2026-11-03 平台文档与主服务复验

图片/视频按 https://docs.oceanway.tech/docs/ 及当前模型公开能力接入。图片分别使用 OpenAI Images 和 Gemini 协议；任务 ID 持久化、按原任务查询/下载、Key 变化校验和 Retry-After 退避纳入原生回归。

- 同一个主 Key 读取 49 个模型：36 文本、9 图片、4 视频。
- 修正版独立 Windows Codex 正常发现并调用默认 Oceanway MCP；一次 count=2 图片批次实际返回 HTTP 400，无图片，未自动重提；现用 Codex/Claude 配置 hash 不变。
- 最小异步图片请求返回 `account asset storage is full`。平台请求 ID：`202611021706141114096698268d9d6AlMabJng`。需平台处理账户存储额度后重新验收真实生成。
- 最小视频请求返回 HTTP 413；请求 ID：`202611021630451831257558268d9d6s6px3Lfr`。源码中账户存储预留失败也会被包装成请求体过大，实际根因需日志确认。
- Windows Codex、Claude Desktop 默认安装的修复脚本在独立 GitHub Windows 环境均成功；线上 EXE 尚未重打包切换，下载服务器 SSH 当前连接超时。
- API Key 本地素材上传由技术人员后续处理；本期视频参考传真实 HTTPS 图片/视频/音频链接。

本轮原生媒体回归：[41488357344](https://github.com/Oceanway-AI/codex-config/actions/runs/41488357344)。完整应用安装包构建与主服务实际生成单独验收，不以工具发现或本地 fixture 成功替代。详细记录维护在文档仓 PROC-02-027 第 6 节。
