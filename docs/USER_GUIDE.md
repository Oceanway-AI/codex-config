# codex-config 使用说明

本文档适用于 OceanWay AI 用户，用来完成 Codex 的一键配置。

`codex-config` 是一个小工具。您只需要打开软件，填写 API Key，然后点击“一键配置”即可。配置完成后，请重启 Codex。

## 一、下载软件

请根据您的电脑系统下载对应版本：

- macOS 用户：下载 `codex-config-macOS.zip`
- Windows 用户：下载 `codex-config-Windows` 里的 `.exe` 文件

下载后，请先解压文件。

## 二、macOS 用户如何打开

### 正常打开方式

1. 双击 `codex-config-macOS.zip` 解压。
2. 得到 `codex-config.app`。
3. 将 `codex-config.app` 拖到“应用程序”文件夹。
4. 双击打开。

### 如果提示“已损坏，无法打开”

如果 macOS 提示：

```text
“codex-config”已损坏，无法打开。你应该将它移到废纸篓。
```

这通常不是软件真的损坏，而是 macOS 对未上架 App Store 的软件做了安全限制。

请按下面步骤处理：

1. 打开“终端”应用。
2. 复制下面这行命令。
3. 粘贴到终端里。
4. 按回车。

```bash
xattr -dr com.apple.quarantine /Applications/codex-config.app
```

然后重新双击 `codex-config.app` 打开。

如果您的软件没有放在“应用程序”文件夹，而是在“下载”文件夹，请使用下面这行命令：

```bash
xattr -dr com.apple.quarantine ~/Downloads/codex-config.app
```

## 三、Windows 用户如何打开

1. 下载 Windows 版本。
2. 双击 `.exe` 文件。
3. 如果 Windows 弹出安全提示，请选择“更多信息”，然后点击“仍要运行”。
4. 软件打开后即可开始配置。

## 四、如何完成配置

打开软件后，您会看到两个输入框：

- `API Key`
- `Base URL`

一般情况下，`Base URL` 已经自动填写好了，不需要修改。

您只需要：

1. 在 `API Key` 输入框中粘贴您的 API Key。
2. 确认 `Base URL` 是：

```text
https://ocean-way.top
```

3. 点击“一键配置”。
4. 看到成功提示后，关闭软件。
5. 重启 Codex。

重启 Codex 后，即可使用 OceanWay AI provider。

如果您已经在 Codex 里登录过 ChatGPT，本工具会尽量保留这个登录状态，并把 OceanWay API Key 写到 provider 配置中。这样更有机会继续使用 Codex Mobile、插件、自动化、额度查询等依赖登录态的功能。

如果您一开始没有登录 ChatGPT，本工具不会伪造登录状态，会使用普通 API Key 方式配置 OceanWay AI。这种方式也可以使用 OceanWay AI，但部分依赖 ChatGPT 登录态的功能可能不可用。

## 五、可选：迁移历史可见性

如果您之前使用 OpenAI 账号登录 Codex，切换到 OceanWay AI 后发现旧历史记录不显示，可以点击“迁移历史”。

这个功能不是自动执行的，并且只在当前 provider 是 OceanWay AI 时可用。点击后，软件会先扫描本机 Codex 历史记录，并显示将要迁移的数量。确认后才会开始处理。

迁移历史只会修改本机历史记录里的 provider 元数据，让旧会话在 OceanWay AI 下显示；不会修改对话正文。

迁移前，软件会自动创建备份：

```text
~/.codex/oceanway-history-migration-backup/
```

需要注意：如果某些历史记录包含加密内容，迁移后可能能在列表里看到，但不一定能继续对话或压缩上下文。软件会在确认前提示这个风险。

如果之后点击“恢复默认”，软件会同时撤销由本工具记录过的历史迁移。使用 OceanWay AI 期间新建的会话不会被强行同步到 OpenAI Official。

## 六、如何测试连接

如果您不确定 API Key 是否可用，可以点击“测试连接”。

如果测试成功，说明当前 API Key 和 Base URL 可以连接。

如果测试失败，请检查：

- API Key 是否复制完整。
- API Key 前后是否多了空格。
- 网络是否正常。
- Base URL 是否为 `https://ocean-way.top`。

## 七、如何恢复默认设置

如果您不想继续使用 OceanWay AI 配置，可以点击“恢复默认”。

恢复后，软件会尽量把 Codex 配置恢复到您第一次使用本工具之前的状态。

如果您之前使用过“迁移历史”，恢复默认也会撤销由本工具记录过的历史迁移。

适用情况：

- 您之前已经配置过其他 Codex provider。
- 您只是临时试用 OceanWay AI。
- 您想撤销本工具写入的配置。

点击“恢复默认”后，请重启 Codex。

## 八、常见问题

### 1. 我需要修改 Base URL 吗？

通常不需要。

默认 Base URL 是：

```text
https://ocean-way.top
```

除非 OceanWay AI 工作人员明确要求您修改，否则请保持默认。

### 2. 配置完成后为什么还要重启 Codex？

Codex 启动时会读取配置文件。配置完成后，重启 Codex 可以让新配置生效。

### 3. API Key 输入后为什么看不到明文？

这是正常的。为了避免旁人看到您的 API Key，输入框默认会隐藏内容。

如果需要检查，可以点击输入框右侧的眼睛图标显示或隐藏。

### 4. 恢复默认会删除我的其他 Codex 配置吗？

正常情况下不会。

本工具会在第一次配置前保存一份原始配置。点击“恢复默认”时，会优先恢复这份原始配置。

### 5. 我不小心填错了 API Key 怎么办？

重新打开软件，填写正确的 API Key，再点击“一键配置”即可。

### 6. 软件打不开怎么办？

请先确认您下载的是对应系统的版本：

- macOS 使用 `codex-config-macOS.zip`
- Windows 使用 `.exe`

如果仍然无法打开，请联系 OceanWay AI 工作人员，并说明：

- 您的电脑系统是 macOS 还是 Windows。
- 出现了什么提示。
- 您操作到了哪一步。

## 九、给用户的简短操作版

如果您只想快速完成配置，请按这几步操作：

1. 下载并打开 `codex-config`。
2. 粘贴 API Key。
3. 确认 Base URL 是 `https://ocean-way.top`。
4. 点击“一键配置”。
5. 重启 Codex。
