# Oceanway Config 项目需求入口

代码工作分支：codex/oceanway-config-rebuild。

需求正文唯一维护在 [Oceanway-AI/oceanway-docs](https://github.com/Oceanway-AI/oceanway-docs)，文档工作分支同名。现行需求更新已推送至同名开发分支，包含六款应用、下载安装、文本多模型、图片 MCP 和历史保留；站点发布以文档仓合并部署为准。本仓保留入口、使用和运行说明，不复制维护另一套需求。

文档目录：content/docs/06_开发/07_内部工具/。

| 阅读顺序 | 文档 ID | 文件 |
| --- | --- | --- |
| 1. 产品需求 | DEV-X-07-001 / PRD-CODEXCONFIG-001 | DEV-X-07-001_配置助手核心需求PRD.mdx |
| 2. 功能与验收 | DEV-X-07-002 | DEV-X-07-002_配置恢复与诊断功能规格.mdx |
| 3. 开发任务与追踪 | DEV-X-07-003 / WP-CODEXCONFIG-001 | DEV-X-07-003_配置助手需求与兼容核对工作包.mdx |
| 4. 技术与维护 | DEV-X-07-004 | DEV-X-07-004_配置助手开发与维护.mdx |

补充入口：

- 文本与图片范围：content/docs/02_决策/ADR-0033_Oceanway文本与图片配置范围.mdx。
- 历史范围：content/docs/02_决策/ADR-0030_Oceanway历史保留与免迁移范围.mdx。
- 已确认方向：content/docs/02_决策/ADR-0029_Oceanway配置应用范围与改造方向.mdx。
- Codex / Claude Code 历史续聊调研：content/docs/07_过程记录/04_会议与复盘/PROC-04-006_OceanwayConfig历史续聊可行性核对20261102.mdx。

开发前读取相关 PRD、FS 和工作包；每项改动引用需求、任务和验收编号。文档中 Proposed 的技术选择不当成已验证功能，既有 README 的旧行为不反向覆盖新需求。

开发者已有文档仓 PR #39 与配置仓 PR #5 作为相关背景；正文沿用已有编号。后续整合时维护同一份文档，不保留重复需求源，也不把未合并草案误当成 main 当前内容。
