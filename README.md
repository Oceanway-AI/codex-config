# OceanWay codex-config

[需求与范围](https://docs.oceanway.site/docs/03_需求与范围/codex-config) · [系统技术](https://docs.oceanway.site/docs/06_技术与工程/02_系统实现/codex-config) · [团队与AI流程](https://docs.oceanway.site/docs/01_总览与治理/03_仓库与治理/BDOC-03-006_团队与AI协作研发流程) · [验证分工](https://docs.oceanway.site/docs/06_技术与工程/03_公共工程规范/DEV-X-04-002_轻量验证与PR检查分工)

这是OceanWay维护的Rust/Tauri桌面配置助手，前端是HTML/CSS/JavaScript；不是官方Codex产品规范或模型兼容保证。源码只包含当前Rust/Tauri实现，旧Python/PyQt打包方案不适用。

## 实际功能与边界

- 本地Rust后端配置OceanWay provider，已检测到ChatGPT登录时采用保留登录的策略，否则采用现有API Key策略；具体字段与兼容判断以源码为准。
- 留空Key可复用已保存值，不向前端返回完整秘密。图片CLI备用配置写入Codex工具子进程环境，因此工具命令能读取该凭据；不是全系统环境变量。
- 当前“一键配置并重启”依次写入、回读并请求重启，失败立即停止、不自动重试，也不自动验证图片生成。配置成功、目录连接成功、模型调用成功分别判断。
- 首次配置保存config/auth原快照；可选历史迁移另有确认、备份与manifest。恢复配置快照和选择性恢复历史不能混为无损合并所有后续人工修改。
- 诊断不修改业务配置，但有有效凭据时可能请求模型目录并读取系统/进程；支持报告脱敏，复制报告会写剪贴板。
- 账户额度/模型权限尚为reserved，不展示模拟零余额。更新由Tauri updater验证签名；ad-hoc macOS包不代表Developer ID签名或公证完成。

默认Base URL当前为源码中的`https://ocean-way.top`，不由说明文档擅自切换；既有兼容字段不承诺未来所有客户端版本都支持。

## 使用和开发

面向用户的操作见[使用说明](docs/USER_GUIDE.md)。开发入口见[BUILD.md](BUILD.md)与[协作规则](CONTRIBUTING.md)。本地先做相关快检，PR CI集中关键检查；普通文档不运行全Tauri构建。

安全界面fixture：

```bash
npm ci
npm run test:ui
```

fixture只在`127.0.0.1:4186`监听，使用假状态，不调用Tauri、不发送Key、不重启应用。真实桌面开发命令`npm run dev`会进入本机应用读写边界，未经明确测试范围不要对真实账户点击配置、迁移、恢复或更新。

## 数据与发布

实际目录由`codex_home()`解析，可能是已有CODEX_HOME或用户`.codex`。涉及`config.toml`、`auth.json`、历史JSONL、`state_5.sqlite`和两类备份；不得把这些真实文件、日志或签名私钥提交到源码/文档。

macOS arm64/Intel和Windows是现有桌面发行目标，与服务器镜像linux/amd64规则分别处理。PR关键测试/target检查、手动制品构建、带签名的draft release是不同流程；发版不由修改README自动授权，准确命令和产物见BUILD。
