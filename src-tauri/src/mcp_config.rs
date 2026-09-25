use super::*;
use serde_json::Value as Json;

pub const SERVER: &str = "oceanway_images";
pub const RECEIPT: &str = "oceanway-image-mcp.json";
const OWNER: &str = "oceanway-config/image-mcp/v1";

#[derive(Serialize, Deserialize)]
struct Receipt {
    owner: String,
    version: String,
    command: PathBuf,
    sha256: String,
    entry: Json,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub configured: bool,
    pub rules_configured: bool,
    pub runtime_verified: bool,
    pub tools_available: bool,
    pub message: String,
}

fn entry_value(doc: &DocumentMut) -> Result<Option<Json>, String> {
    let Some(servers) = doc.get("mcp_servers") else { return Ok(None); };
    let table = servers.as_table_like().ok_or("mcp_servers 不是有效配置表，未覆盖。")?;
    let Some(item) = table.get(SERVER) else { return Ok(None); };
    let mut wrapper = DocumentMut::new();
    wrapper["entry"] = item.clone();
    let value: Json = toml_edit::de::from_str(&wrapper.to_string())
        .map_err(|_| "图片 MCP 配置不是有效的 TOML 表。")?;
    Ok(value.get("entry").cloned())
}

fn receipt(home: &Path) -> Result<Option<Receipt>, String> {
    managed_files::text(&home.join(RECEIPT))?.map(|text| serde_json::from_str(&text)
        .map_err(|_| "图片 MCP 所有权记录损坏，未覆盖现有配置。".into())).transpose()
}

fn verify_receipt(home: &Path, receipt: &Receipt, verify_hash: bool) -> Result<(), String> {
    let expected_root = fs::canonicalize(home).map_err(|_| "无法定位 Codex 配置目录。")?
        .join("oceanway-runtime");
    if receipt.owner != OWNER || receipt.sha256.len() != 64
        || !receipt.sha256.bytes().all(|c| c.is_ascii_hexdigit())
        || receipt.command.parent() != Some(expected_root.join(&receipt.sha256).as_path())
        || receipt.command.file_name().and_then(|name| name.to_str()) != Some(runtime_name())
    {
        return Err("图片 MCP 所有权记录不属于本工具。".into());
    }
    if verify_hash {
        let actual = fs::canonicalize(&receipt.command).map_err(|_| "图片 MCP 后台程序缺失，请重新配置。")?;
        if actual != receipt.command || managed_files::file_hash(&actual)? != receipt.sha256 {
            return Err("图片 MCP 后台程序校验失败，请重新配置。".into());
        }
    }
    Ok(())
}

fn runtime_name() -> &'static str {
    if cfg!(windows) { "oceanway-image-mcp.exe" } else { "oceanway-image-mcp" }
}

pub fn preflight(home: &Path, config: &str) -> Result<(), String> {
    let doc = config.parse::<DocumentMut>().map_err(|_| "config.toml 格式损坏，未写入 MCP。")?;
    if let Some(current) = entry_value(&doc)? {
        let saved = receipt(home)?.ok_or("发现其他来源的同名 oceanway_images MCP，未覆盖。")?;
        verify_receipt(home, &saved, false)?;
        if current != saved.entry {
            return Err("oceanway_images 已被其他程序或用户修改，未覆盖。".into());
        }
    }
    Ok(())
}

fn deploy(home: &Path) -> Result<(PathBuf, String), String> {
    let source = env::current_exe().map_err(|_| "无法定位内置图片 MCP 程序。")?;
    let hash = managed_files::file_hash(&source)?;
    let root = fs::canonicalize(home).map_err(|_| "无法定位 Codex 配置目录。")?;
    let runtime_root = root.join("oceanway-runtime");
    let directory = runtime_root.join(&hash);
    for path in [&runtime_root, &directory] {
        if path.exists() {
            if fs::canonicalize(path).ok().as_ref() != Some(path) {
                return Err("图片 MCP 后台程序目录不能是链接。".into());
            }
        } else {
            fs::create_dir(path).map_err(|_| "无法建立图片 MCP 后台程序目录。")?;
        }
    }
    let target = directory.join(runtime_name());
    if !target.exists() {
        copy_private_new(&source, &target).map_err(|_| "无法部署图片 MCP 后台程序。")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o700))
                .map_err(|_| "无法设置图片 MCP 后台执行权限。")?;
        }
    }
    if fs::canonicalize(&target).ok().as_ref() != Some(&target)
        || managed_files::file_hash(&target)? != hash
    {
        return Err("内置图片 MCP 后台程序复制校验失败。".into());
    }
    Ok((target, hash))
}

pub fn install(home: &Path, config: &str) -> Result<String, String> {
    preflight(home, config)?;
    let (command, sha256) = deploy(home)?;
    let mut doc = config.parse::<DocumentMut>().map_err(|_| "config.toml 格式损坏。")?;
    let mut table = Table::new();
    table["command"] = value(command.to_string_lossy().to_string());
    let mut args = toml_edit::Array::new();
    args.push("--image-mcp-stdio");
    table["args"] = value(args);
    let mut environment = Table::new();
    environment["CODEX_HOME"] = value(fs::canonicalize(home).map_err(|_| "无法定位 Codex 目录。")?
        .to_string_lossy().to_string());
    table["env"] = Item::Table(environment);
    table["enabled"] = value(true);
    table["startup_timeout_sec"] = value(30);
    table["tool_timeout_sec"] = value(60);
    let servers = ensure_table(doc.as_table_mut(), "mcp_servers")?;
    servers[SERVER] = Item::Table(table);
    let saved = Receipt { owner: OWNER.into(), version: env!("CARGO_PKG_VERSION").into(),
        command, sha256, entry: entry_value(&doc)?.ok_or("无法生成 MCP 配置。")? };
    let bytes = serde_json::to_vec_pretty(&saved).map_err(|_| "无法生成 MCP 所有权记录。")?;
    write_private_atomic(&home.join(RECEIPT), &bytes).map_err(|_| "无法保存 MCP 所有权记录。")?;
    Ok(doc.to_string())
}

pub fn status(home: &Path, config: &str) -> McpStatus {
    let checked = (|| -> Result<(), String> {
        preflight(home, config)?;
        let saved = receipt(home)?.ok_or("图片 MCP 尚未安装。")?;
        verify_receipt(home, &saved, true)?;
        let doc = config.parse::<DocumentMut>().map_err(|_| "config.toml 格式损坏。")?;
        if entry_value(&doc)?.as_ref() != Some(&saved.entry) { return Err("图片 MCP 尚未注册。".into()); }
        Ok(())
    })();
    let rules_configured = agent_rules::configured(home);
    McpStatus { configured: checked.is_ok() && rules_configured, rules_configured,
        runtime_verified: checked.is_ok(), tools_available: false,
        message: checked.err().unwrap_or_else(|| "MCP 与短规则已保存；工具握手及真实生图分开验证。".into()) }
}

pub fn command(home: &Path) -> Result<PathBuf, String> {
    let config = read_config_for_write(&home.join("config.toml"))?;
    preflight(home, &config)?;
    let saved = receipt(home)?.ok_or("图片 MCP 尚未配置。")?;
    verify_receipt(home, &saved, true)?;
    let doc = config.parse::<DocumentMut>().map_err(|_| "配置损坏。")?;
    if entry_value(&doc)?.as_ref() != Some(&saved.entry) { return Err("图片 MCP 尚未注册。".into()); }
    Ok(saved.command)
}

pub fn remove(home: &Path, config: &str) -> Result<String, String> {
    preflight(home, config)?;
    let mut doc = config.parse::<DocumentMut>().map_err(|_| "配置损坏，未撤销 MCP。")?;
    if let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut) {
        servers.remove(SERVER);
        if servers.is_empty() { doc.remove("mcp_servers"); }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn home() -> PathBuf {
        let root = env::temp_dir().join(format!("ow-mcp-{}", Local::now().timestamp_nanos_opt().unwrap()));
        fs::create_dir_all(&root).unwrap();
        fs::canonicalize(root).unwrap()
    }
    #[test]
    fn external_mcp_is_not_overwritten() {
        let home = home();
        assert!(install(&home, "[mcp_servers.oceanway_images]\ncommand='user-tool'\n").is_err());
    }
    #[test]
    fn registration_is_idempotent_and_other_servers_survive() {
        let home = home();
        let initial = "[mcp_servers.keep]\ncommand='keep-tool'\n";
        let first = install(&home, initial).unwrap();
        let second = install(&home, &first).unwrap();
        assert_eq!(first, second);
        assert!(remove(&home, &second).unwrap().contains("keep-tool"));
        assert!(!remove(&home, &second).unwrap().contains("oceanway_images"));
        assert!(preflight(&home, &second.replace("tool_timeout_sec = 60", "tool_timeout_sec = 61")).is_err());
    }
}
