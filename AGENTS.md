<!-- oceanway:governance:start -->
<!-- OceanWay project: codex-config -->
## OceanWay 项目协作规范

开工与恢复先读[文档治理规范](<../operations/oceanway-docs/content/docs/01_组织与品牌/03_仓库与治理/BDOC-03-002_文档归属修订与交付状态规范.mdx>)、[本项目文档入口](<../operations/oceanway-docs/content/docs/03_项目/codex-config/index.mdx>)和 [oceanway-devops 统一技能](<../operations/oceanway-docs/governance/skills/oceanway-devops/SKILL.md>)。记录本次任务类型、已读步骤、PRD／FS／WP 与采用版本、实际证据和下一动作。链接以当前工程根目录为基准，路径不可读时定位本环境的 Docs checkout 并运行接入诊断。

1. 新需求先检索项目功能索引、`state/registry.json`、原名称和替代／修订关系，记录复用、修订、替代或新建结论。已有功能沿原 PRD／FS 的稳定 ID 维护。
2. PRD 写目标与范围，FS 写规则与验收，WP 写本轮任务与完成条件，过程记录写具名事实。正文使用项目总览、功能、交付、工程与运行、历史五类目录，共享合同引用唯一维护入口。
3. 追加、修改、删除同步原规范版本、验收映射和修订记录，核对消费者、数据、在途任务及旧结果的重验。已明确并获授权的任务持续推进，待决项关联受影响任务和所需决定。
4. 测试按 [BDOC-03-005](<../operations/oceanway-docs/content/docs/01_组织与品牌/03_仓库与治理/BDOC-03-005_AI辅助测试与质量验证规范.mdx>) 维护大纲、用例 ID／版本、覆盖、实际批次、缺陷和重验。AI 用例预期以已确认 FS 为依据，结果引用实际工具证据，未执行项如实登记。
5. 资源、构建、CI/CD、部署与运行按 [BDOC-03-006](<../operations/oceanway-docs/content/docs/01_组织与品牌/03_仓库与治理/BDOC-03-006_资源构建部署与运行规范.mdx>) 核对制品摘要、配置与数据版本、现场状态、并发、中断接续、迁移恢复及交接。操作采用当前任务授权和本工程环境规则。
6. 开源改造及上游更新按 [BDOC-03-007](<../operations/oceanway-docs/content/docs/01_组织与品牌/03_仓库与治理/BDOC-03-007_开源项目接入与持续维护规范.mdx>) 保留来源与使用依据、原版证据、目标差异和维护安排。共同交付按 [BDOC-03-003](<../operations/oceanway-docs/content/docs/01_组织与品牌/03_仓库与治理/BDOC-03-003_跨项目交付结构与流转规范.mdx>) 关联总包、工程子范围及实际版本组合。
7. 获准执行基线使用 Accepted，过程事实使用 Recorded。完成阶段及时更新 WP 的 `progress`、已完成项、`status_reviewed`、`status_evidence` 与结论范围，核对完成工作遗留的 Proposed。关联 WP 按阶段运行 start／close，满足本轮完成定义后登记 done。无 WP 的独立评估按具名记录交接。
8. Docs 集中维护需求、功能与架构规范，代码仓维护源码说明、运行手册、脚本和脱敏原始证据。交付列明源码、规范、契约、用例、制品、环境、结果与剩余项。批次进展写入关联记录，AGENTS.md 维护长期工程规则。
9. Docs 修改在 main 保留已有工作，依次运行 `pnpm docs:redirects`、`pnpm docs:test`、`pnpm docs:registry`、`pnpm docs:check`，核对后提交并立即推送 `origin main`。代码仓提交、PR 和发布采用本工程现行规则。
10. 新建或接手项目时核对本文件、工程职责、真实命令与运行位置。成员接入及更新按 [BDOC-03-004](<../operations/oceanway-docs/content/docs/01_组织与品牌/03_仓库与治理/BDOC-03-004_团队技能接入与文档实操手册.mdx>) 执行 agents 检查、doctor 和实际会话演练。流程调整或发现缺口时同批修订共享技能、模板与检查，记录实际生效的仓库提交和范围。

模板及填写规则见 [BDOC-04-001](<../operations/oceanway-docs/content/docs/01_组织与品牌/04_文档模板/01_通用与决策/BDOC-04-001_通用元数据模板.mdx>)。本节随共享技能维护，工程专属规则写在后续章节，并核对适用的子目录指令。
<!-- oceanway:governance:end -->

# Codex Config 桌面配置工具

`main` 维护 Rust/Tauri 配置工具。`src/` 为桌面界面，`src-tauri/` 为配置写入、凭据处理、诊断、备份和恢复。变更提供方、登录、历史迁移与恢复时，在独立临时配置目录验证。保留用户其他提供方、登录状态和首次备份。测试夹具与支持报告只使用脱敏数据。

根 `package.json`、`src-tauri/Cargo.toml` 及 `.github/workflows/build.yml` 记录实际工具链。在仓库根运行 `npm ci`、`npm test`，在 `src-tauri/` 运行 `cargo test`。本机交互开发使用 `npm run dev`，桌面包使用 `npm run build` 并具备目标平台的 Tauri/Rust 工具链。用户操作与恢复记录见 [使用说明](docs/USER_GUIDE.md)。

向 `main` 提交 PR，核对 macOS ARM、macOS Intel 和 Windows 三项构建，以及当前分支的评审要求。`.github/workflows/release.yml` 在推送 `v*` 标签或手工触发时构建签名更新包并创建草稿 Release。发布操作按本轮授权执行，交付时记录版本、制品和实际发布状态。重建开发分支的工程命令见该分支根 `AGENTS.md`。
