# 开发、隔离验证与桌面制品

本仓使用Rust/Tauri 2与HTML/CSS/JavaScript。准确命令来自根package.json、src-tauri/Cargo.toml、Tauri配置和构建脚本；不使用Python打包。需求与详细机制见文档站系统技术，机器契约继续留源码。

## 环境与安全入口

当前CI使用Node.js 22、npm、Rust stable及相应平台target。原生Tauri编译需要目标平台工具链，不能以JS测试通过推定三个发行目标都能构建。

```bash
npm ci
npm run test:ui
```

UI fixture固定监听回环4186，先确认端口空闲；假凭据/假调用不读写真实配置或重启Codex。`npm run dev`是实际Tauri应用，会读取当前用户配置和进程状态，需要明确的隔离测试范围；不把它当fixture。

## 按改动选择检查

| 范围 | 可选本地命令 | 边界 |
|---|---|---|
| 普通说明文档 | 核对命令/链接与`git diff --check` | 不跑全量桌面构建 |
| 自动配置/日志/URL校验JS | `node --test tests/auto-configure.test.js`等相关文件 | `npm test`组合现有JS测试，不安装/启动桌面应用 |
| Rust配置/恢复 | `cargo test --manifest-path src-tauri/Cargo.toml corrupt_config_is_never_replaced`等具名测试 | 会编译原生crate；测试用显式临时目录/假凭据，不运行真实命令 |
| target编译 | `cargo check --locked --manifest-path src-tauri/Cargo.toml` | 选择实际目标工具链，不代表打包/签名通过 |
| 完整桌面制品 | `npm run build` | Tauri bundle，按明确平台和制品任务执行，不每次本地跑 |

当前JS回归位于tests，Rust回归在main.rs的tests模块与regression_tests.rs。只读诊断、配置写入、恢复、历史迁移和重启分别验证，不能用配置完成替代模型生成验收。

## 配置与恢复实现

`codex_home()`先读已有CODEX_HOME，再使用HOME/USERPROFILE下`.codex`。测试使用内部显式目录函数，不重设这些系统变量操作用户真实环境。

首次快照目录`oceanway-ai-backup`保存原config/auth和存在性元数据；重复配置不覆盖初始快照。恢复时按快照恢复/移除原文件，随后撤销已记录历史迁移；无快照时清理本工具provider、API Key和图片备用环境。完整文件快照可能回退首次配置后人工新增的设置，不能承诺自动无损合并。

历史迁移扫描sessions/archived_sessions中的JSONL首行session_meta与state_5.sqlite对应行，独立备份和manifest只关联本工具改变项。历史恢复尽量保护迁移后新会话；加密内容仅恢复可见性不保证续用。跨文件/数据库操作失败不能一概报成功。

秘密只由本地受控Rust后端读写；报告不输出完整Key/Token。Unix私密文件/目录权限与备份行为有专用回归，不能把Unix权限测试外推为Windows ACL或整个系统安全证明。

## CI与制品

build.yml保留现有三个Build检查名称。PR在原生macOS arm64/Windows执行受影响JS/Rust测试，macOS Intel矩阵只做Intel target编译以避免重复同主机测试；若runner实际host与目标不同，PR补目标编译，不假定macos-latest的CPU架构，手动运行才构建/上传bundle；普通文档只输出轻量结果。具体触发和scope以工作流为准，不新增固定PR章节、独立AI审查或文档ID门禁。

macOS本地脚本`./build.sh`生成arm64/Intel制品；Windows用`build.ps1`。这些是制品任务，不能在文档检查时执行。输出规则以脚本实际version/路径为准，不固定旧文件名给后续版本。

release.yml由v标签或手动运行创建签名updater资源和draft release。tauri.conf.json保存公开更新endpoint/公钥，tauri.updater.conf.json开启updater产物，tauri.adhoc.conf.json选择macOS ad-hoc应用签名。更新签名不是Apple Developer ID/公证；对外发行需要相应已确认范围。

TAURI_SIGNING_PRIVATE_KEY及密码由明确发布Owner管理，不读出、不提交、不自动设置远端Secret。下载、安装更新和应用重启均有真实副作用，本轮文档工作不执行。

[需求与范围](https://docs.oceanway.site/docs/03_需求与范围/codex-config) · [系统技术](https://docs.oceanway.site/docs/06_技术与工程/02_系统实现/codex-config) · [团队与AI流程](https://docs.oceanway.site/docs/01_总览与治理/03_仓库与治理/BDOC-03-006_团队与AI协作研发流程) · [验证分工](https://docs.oceanway.site/docs/06_技术与工程/03_公共工程规范/DEV-X-04-002_轻量验证与PR检查分工)
