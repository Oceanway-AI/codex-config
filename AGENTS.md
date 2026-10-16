# 配置助手仓库约束

[需求与范围](https://docs.oceanway.site/docs/03_需求与范围/codex-config) · [系统技术](https://docs.oceanway.site/docs/06_技术与工程/02_系统实现/codex-config) · [团队与AI流程](https://docs.oceanway.site/docs/01_总览与治理/03_仓库与治理/BDOC-03-006_团队与AI协作研发流程) · [验证分工](https://docs.oceanway.site/docs/06_技术与工程/03_公共工程规范/DEV-X-04-002_轻量验证与PR检查分工)

本仓为OceanWay自己的Rust/Tauri配置助手，不是Codex官方产品规范。先读README、BUILD、当前任务与相关源码；协作遵循CONTRIBUTING，五人团队按责任分工，AI在已授权范围持续执行。

## 真实文件与秘密边界

- 配置读写在本地Rust后端；不新增远端密钥托管。完整Key不回传给前端、不写日志/支持报告，不读取或公开用户真实配置作为演示。
- `codex_home()`优先读既有CODEX_HOME，否则用户主目录下`.codex`。测试优先用接受显式目录的内部函数及自有临时夹具；不重设HOME或CODEX_HOME来覆盖用户环境，不操作真实config/auth/历史文件。
- 默认UI的一键配置路径实际为写入→回读→重启；诊断不是该写入动作。保留既有用户操作含义，不因文档补齐擅自增加、删去或拆分确认流程。
- 历史迁移必须独立扫描/用户选择/备份，按manifest恢复本工具修改的项；不改聊天正文，不承诺加密历史可续用。
- 首次config/auth快照恢复与选择性历史恢复是两种机制。不得把完整快照恢复描述为自动合并所有后续人工配置；失败/部分恢复需如实说明。
- 只读诊断可能用已存Key请求模型目录，并可读取版本/进程；复制报告写剪贴板。测试这些行为只用假凭据与本地受控服务，不触达真实账户。

## 构建与更新

npm脚本、Cargo manifest、Tauri配置与发布脚本是准确命令/机器契约源，不能从README猜不存在的配置或承诺全部Codex版本兼容。账户信息当前reserved，未知不填零。

更新公钥与endpoint可公开，签名私钥和密码不得读出、提交或自动上传。updater签名与macOS Developer ID/公证不同，当前ad-hoc包不能写成已公证。不运行真实重启、迁移、恢复、安装更新、签名或发版，除非任务明确授权。

## 本地检查与交接

说明文档核对命令/链接/diff；JS逻辑选相关Node测试，Rust逻辑选相关隔离测试，PR CI集中关键回归与target检查。不要每次全Tauri构建、所有系统打包或真实账号验证。安全界面优先`npm run test:ui`隔离fixture，真实`npm run dev`会进入本机应用边界，不作为默认安全测试。

不要求任务都另开Issue、独立AI审查或每次提交重新用户验收SHA。确认用户结果与最终接收、测试与发布分别记录；交接版本、路径、实际结果、未验项和后续Owner。
