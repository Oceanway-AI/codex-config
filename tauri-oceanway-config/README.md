# codex-config

OceanWay AI Codex 配置工具是一个用于一键配置 Codex 第三方 provider 的小型桌面应用。当前主版本使用 Tauri：窗口界面由 HTML/CSS 提供，配置写入逻辑由 Rust 执行，不再打包 Python、Tkinter、PyQt 或 Qt 运行时。

## 功能

- 双击打开一个简单窗口
- API Key 输入框默认隐藏
- Base URL 默认 `https://ocean-way.top`
- 点击“一键配置”写入 Codex provider 配置
- 打开时读取当前 Codex 配置状态
- 测试 API Key 和 Base URL 连接
- 打开 `~/.codex` 配置目录
- API Key 支持显示/隐藏切换
- 点击“恢复默认值”清空 provider 配置
- 恢复默认值前会二次确认
- 写入或恢复前自动备份已有文件
- 支持 macOS 和 Windows 分别构建分发

## 写入内容

点击“一键配置”后会写入：

```text
~/.codex/config.toml
~/.codex/auth.json
```

`config.toml` 内容格式：

```toml
model_provider = "OceanWay"
model = "gpt-5.4"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.OceanWay]
name = "OceanWay"
base_url = "https://ocean-way.top"
wire_api = "responses"
requires_openai_auth = true
```

如果用户原本的 `config.toml` 已经有根级 `model = "..."`，工具会沿用该 model；否则使用 `gpt-5.4`。

`auth.json` 内容格式：

```json
{
  "OPENAI_API_KEY": "用户填写的 API Key"
}
```

## 恢复默认值

点击“恢复默认值”后，工具会先备份旧文件，然后恢复为无第三方 provider 的状态：

```text
~/.codex/config.toml  -> 清空
~/.codex/auth.json    -> {}
```

备份文件会生成在原文件同目录下，文件名带 `.bak.<时间戳>`。

## 构建依赖

需要安装：

- Node.js / npm
- Rust / Cargo，可通过 `https://rustup.rs` 安装

Tauri 不支持跨平台打包：macOS `.app` 需要在 macOS 构建，Windows 安装包需要在 Windows 构建。

## macOS 构建

```bash
chmod +x ./build.sh
./build.sh
```

常见输出：

```text
src-tauri/target/release/bundle/macos/codex-config.app
dist/codex-config-macOS.zip
```

正式对外分发时，macOS 版本建议进行 Developer ID 签名和 notarization。

## Windows 构建

在 Windows PowerShell 中运行：

```powershell
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

常见输出位于：

```text
src-tauri\target\release\bundle
```

具体文件类型取决于当前 Tauri/Windows 打包环境，通常会生成 `.exe` 或 `.msi` 安装包。

## 项目文件

```text
src/                      Tauri 前端界面
src-tauri/                Rust 后端与 Tauri 配置
build.sh                  macOS/Linux Tauri 构建脚本
build.ps1                 Windows Tauri 构建脚本
setup-codex-provider.py   旧 Python CLI/GUI 脚本，保留作兼容参考
BUILD.md                  构建说明
```
