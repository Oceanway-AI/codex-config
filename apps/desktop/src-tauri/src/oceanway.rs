//! Oceanway product layer. Agent configuration is applied by CC Switch's native
//! provider adapters; history remains owned by the upstream session manager.
use crate::{AppState, AppType, Provider, ProviderService};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

pub const ORIGIN: &str = "https://console.oceanway.tech";
pub const API_BASE: &str = "https://console.oceanway.tech/v1";
const PROVIDER_ID: &str = "oceanway";
const APPS: &[(&str, &str)] = &[
    ("codex", "Codex"),
    ("claude", "Claude Code"),
    ("gemini", "Gemini CLI"),
    ("opencode", "OpenCode"),
    ("openclaw", "OpenClaw"),
];
static APPLY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSummary {
    configured: bool,
    key_preview: Option<String>,
    base_url: &'static str,
    configured_apps: Vec<String>,
    default_image_model: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    base_url: &'static str,
    models: Vec<Model>,
    checked_at: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedApp {
    app_id: String,
    name: String,
    installed: bool,
    desktop_installed: bool,
    config_exists: bool,
    configurable: bool,
    reason: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSelection {
    app_id: String,
    model: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyRequest {
    api_key: String,
    apps: Vec<AppSelection>,
    image_model: Option<String>,
    #[serde(default)]
    allowed_image_models: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppResult {
    app_id: String,
    ok: bool,
    message: String,
    image_configured: bool,
    image_message: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    results: Vec<AppResult>,
    restart_required: bool,
    backup_path: String,
    images_configured: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    launched: bool,
    restart_required: bool,
    message: String,
}

fn validate_key(value: &str) -> Result<&str, String> {
    let key = value.trim();
    if key.is_empty()
        || key.len() > 8192
        || key
            .chars()
            .any(|c| char::is_control(c) || char::is_whitespace(c))
    {
        return Err("请输入有效的 Oceanway API Key".into());
    }
    Ok(key)
}

pub fn validate_origin(value: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(value).map_err(|_| "无效的 API 地址")?;
    if url.scheme() != "https"
        || url.host_str() != Some("console.oceanway.tech")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path().trim_end_matches('/'), "" | "/v1")
    {
        return Err("Oceanway Config 只能接入 https://console.oceanway.tech".into());
    }
    Ok(())
}

/// Used by native switching and proxy routing as well as the new IPC layer.
/// Existing non-Oceanway rows remain in the database, but cannot be activated.
pub fn validate_provider(provider: &Provider) -> Result<(), String> {
    if provider.id != PROVIDER_ID || provider.category.as_deref() == Some("official") {
        return Err("请通过 Oceanway 配置页接入 API；原有供应商记录仍保留".into());
    }
    let mut endpoints = Vec::new();
    fn collect(value: &Value, endpoints: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    let key = key.to_ascii_lowercase().replace('_', "");
                    if key.contains("baseurl") || key == "endpoint" {
                        if let Some(s) = value.as_str() {
                            endpoints.push(s.to_owned());
                        }
                    }
                    collect(value, endpoints);
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect(item, endpoints);
                }
            }
            _ => {}
        }
    }
    collect(&provider.settings_config, &mut endpoints);
    if let Some(config) = provider
        .settings_config
        .get("config")
        .and_then(Value::as_str)
    {
        let parsed: toml::Value = toml::from_str(config).map_err(|_| "无效的客户端配置")?;
        collect(
            &serde_json::to_value(parsed).map_err(|_| "配置转换失败")?,
            &mut endpoints,
        );
    }
    if endpoints.is_empty() {
        return Err("Oceanway 供应商缺少 API 地址".into());
    }
    for endpoint in endpoints {
        validate_origin(&endpoint)?;
    }
    Ok(())
}

fn resolve_key(input: &str) -> Result<String, String> {
    if input.trim().is_empty() {
        crate::oceanway_images::read_api_key(&crate::config::get_app_config_dir())
    } else {
        validate_key(input).map(str::to_owned)
    }
}

async fn fetch_models(key: &str) -> Result<Vec<Model>, String> {
    // Refuse redirects so a compromised/misconfigured gateway cannot receive a
    // customer key at another origin. No caller-controlled endpoint exists.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法初始化 API 连接")?;
    let response = client
        .get(format!("{API_BASE}/models"))
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| "无法连接 Oceanway，请检查网络后重试")?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Oceanway Key 检查失败（HTTP {}）", status.as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > 4 * 1024 * 1024)
    {
        return Err("模型列表响应过大".into());
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|_| "Oceanway 返回的模型列表无效")?;
    parse_models(&payload)
}

fn parse_models(payload: &Value) -> Result<Vec<Model>, String> {
    let data = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or("模型列表缺少 data")?;
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for item in data {
        if let Some(id) = item.get("id").and_then(Value::as_str).filter(|id| {
            !id.trim().is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
        }) {
            if seen.insert(id.to_owned()) {
                models.push(Model { id: id.to_owned() });
            }
        }
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    if models.is_empty() {
        return Err("当前 Key 没有可用模型".into());
    }
    Ok(models)
}

#[tauri::command]
pub async fn oceanway_get_connection(
    state: tauri::State<'_, AppState>,
) -> Result<ConnectionSummary, String> {
    let home = crate::config::get_app_config_dir();
    let summary = if home.join("image-provider.json").exists() {
        Some(crate::oceanway_images::read_provider_summary(&home)?)
    } else {
        None
    };
    let mut configured_apps = Vec::new();
    for (id, _) in APPS {
        if state
            .db
            .get_provider_by_id(PROVIDER_ID, id)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            configured_apps.push((*id).to_owned());
        }
    }
    let configured = summary.as_ref().is_some_and(|s| s.key_configured);
    Ok(ConnectionSummary {
        configured,
        key_preview: configured.then(|| "已保存".into()),
        base_url: API_BASE,
        configured_apps,
        default_image_model: summary.and_then(|s| s.default_image_model),
    })
}

#[tauri::command]
pub async fn oceanway_check_key(api_key: String) -> Result<ModelCatalog, String> {
    let key = resolve_key(&api_key)?;
    let models = fetch_models(&key).await?;
    Ok(ModelCatalog {
        base_url: API_BASE,
        models,
        checked_at: chrono::Utc::now().to_rfc3339(),
    })
}

#[tauri::command]
pub async fn oceanway_detect_apps() -> Result<Vec<DetectedApp>, String> {
    let raw = crate::commands::get_tool_versions(
        Some(APPS.iter().map(|(id, _)| (*id).to_owned()).collect()),
        None,
    )
    .await?;
    let versions = serde_json::to_value(raw).map_err(|_| "无法读取应用状态")?;
    Ok(APPS
        .iter()
        .map(|(id, name)| {
            let item = versions
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["name"] == *id));
            let (installed, desktop_installed) = app_installation_state(id, item);
            let broken = item.is_some_and(|v| v["installed_but_broken"] == true);
            DetectedApp {
                app_id: (*id).into(),
                name: (*name).into(),
                installed,
                desktop_installed,
                configurable: true,
                config_exists: config_paths(id)
                    .map(|paths| paths.iter().any(|p| p.exists()))
                    .unwrap_or(false),
                reason: Some(
                    if broken {
                        "检测到应用，但运行环境需要修复"
                    } else if installed {
                        "可以配置"
                    } else {
                        "尚未检测到；可以先保存配置后安装"
                    }
                    .into(),
                ),
            }
        })
        .collect())
}

fn app_installation_state(id: &str, item: Option<&Value>) -> (bool, bool) {
    let desktop = id == "codex" && item.is_some_and(|v| v["desktop_installed"] == true);
    let cli = item.is_some_and(|v| v["version"].is_string() || v["installed_but_broken"] == true);
    (desktop || cli, desktop)
}

fn app_type(id: &str) -> Result<AppType, String> {
    match id {
        "codex" => Ok(AppType::Codex),
        "claude" => Ok(AppType::Claude),
        "gemini" => Ok(AppType::Gemini),
        "opencode" => Ok(AppType::OpenCode),
        "openclaw" => Ok(AppType::OpenClaw),
        _ => Err("此应用尚未完成 Oceanway 接入验证".into()),
    }
}

fn provider_for(selection: &AppSelection, key: &str) -> Result<Provider, String> {
    let model = &selection.model;
    let settings = match selection.app_id.as_str() {
        "codex" => {
            let mut doc = toml_edit::DocumentMut::new();
            doc["model"] = toml_edit::value(model.as_str());
            doc["model_provider"] = toml_edit::value("custom");
            doc["features"]["image_generation"] = toml_edit::value(false);
            doc["model_providers"]["custom"]["name"] = toml_edit::value("Oceanway");
            doc["model_providers"]["custom"]["base_url"] = toml_edit::value(API_BASE);
            doc["model_providers"]["custom"]["wire_api"] = toml_edit::value("responses");
            doc["model_providers"]["custom"]["requires_openai_auth"] = toml_edit::value(false);
            doc["model_providers"]["custom"]["experimental_bearer_token"] = toml_edit::value(key);
            json!({"auth":{"OPENAI_API_KEY":key}, "config":doc.to_string()})
        }
        "claude" => json!({"env":{"ANTHROPIC_BASE_URL":ORIGIN,"ANTHROPIC_AUTH_TOKEN":key,
            "ANTHROPIC_MODEL":model,"ANTHROPIC_DEFAULT_OPUS_MODEL":model,"ANTHROPIC_DEFAULT_SONNET_MODEL":model,"ANTHROPIC_DEFAULT_HAIKU_MODEL":model}}),
        "gemini" => {
            json!({"env":{"GEMINI_API_KEY":key,"GOOGLE_GEMINI_BASE_URL":ORIGIN,"GEMINI_MODEL":model}})
        }
        "opencode" => {
            json!({"npm":"@ai-sdk/openai-compatible","options":{"baseURL":API_BASE,"apiKey":key},"models":{model:{"name":model}}})
        }
        "openclaw" => {
            json!({"baseUrl":API_BASE,"apiKey":key,"api":"openai-responses","models":[{"id":model,"name":model}]})
        }
        _ => return Err("未支持的应用".into()),
    };
    let provider = Provider::with_id(
        PROVIDER_ID.into(),
        "Oceanway".into(),
        settings,
        Some(ORIGIN.into()),
    );
    validate_provider(&provider)?;
    Ok(provider)
}

fn config_paths(id: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = match id {
        "codex" => vec![
            crate::codex_config::get_codex_config_path(),
            crate::codex_config::get_codex_auth_path(),
        ],
        "claude" => vec![
            crate::config::get_claude_settings_path(),
            crate::config::get_claude_mcp_path(),
        ],
        "gemini" => vec![
            crate::gemini_config::get_gemini_env_path(),
            crate::gemini_config::get_gemini_dir().join("settings.json"),
        ],
        "opencode" => {
            vec![crate::opencode_config::get_opencode_config_path().map_err(|e| e.to_string())?]
        }
        "openclaw" => vec![crate::openclaw_config::get_openclaw_config_path()],
        _ => return Err("未支持的应用".into()),
    };
    paths.push(crate::prompt_files::prompt_file_path(&app_type(id)?).map_err(|e| e.to_string())?);
    Ok(paths)
}

const IMAGE_RULE_START: &str = "<!-- OCEANWAY:IMAGES:BEGIN -->";
const IMAGE_RULE_END: &str = "<!-- OCEANWAY:IMAGES:END -->";
fn merge_image_rule(original: &str) -> Result<String, String> {
    let rule = format!("{IMAGE_RULE_START}\nWhen the user requests image generation or image editing, use the oceanway_images MCP tools: generate_images, then get_image_job until completion. Omit the model to use the configured Oceanway image model. For multiple images, submit one generate_images call with count equal to the total requested and prompts for distinct images; the service handles concurrency. Return completed image files as they become available while polling the same job until completion. Do not use built-in imagegen or an image CLI as a fallback. Do not automatically retry failed paid requests.\n{IMAGE_RULE_END}");
    match (
        original.find(IMAGE_RULE_START),
        original.find(IMAGE_RULE_END),
    ) {
        (Some(start), Some(end)) if end >= start => Ok(format!(
            "{}{}{}",
            &original[..start],
            rule,
            &original[end + IMAGE_RULE_END.len()..]
        )),
        (None, None) => Ok(format!(
            "{original}{}\n{rule}\n",
            if original.is_empty() || original.ends_with('\n') {
                ""
            } else {
                "\n"
            }
        )),
        _ => Err("现有图片规则标记不完整；保留原提示词，请检查后重试".into()),
    }
}
fn install_image_rule(state: &AppState, app: &AppType) -> Result<(), String> {
    let path = crate::prompt_files::prompt_file_path(app).map_err(|e| e.to_string())?;
    let old = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let merged = merge_image_rule(&old)?;
    crate::config::write_text_file(&path, &merged).map_err(|e| e.to_string())?;
    // Reflect the live edit through the existing upstream prompt management,
    // retaining the active entry and all inactive prompt templates.
    let _ = crate::PromptService::get_prompts(state, app.clone()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Managed image routing remains active when a customer changes prompt templates.
pub(crate) fn prompt_with_image_rule(
    state: &AppState,
    app: &AppType,
    content: &str,
) -> Result<String, crate::error::AppError> {
    if !matches!(
        app,
        AppType::Codex | AppType::Claude | AppType::Gemini | AppType::OpenCode
    ) {
        return Ok(content.into());
    }
    let servers = state.db.get_all_mcp_servers()?;
    let managed = servers.get("oceanway_images").is_some_and(|server| {
        server.apps.is_enabled_for(app)
            && server.homepage.as_deref() == Some(ORIGIN)
            && server.tags.iter().any(|tag| tag == "oceanway-managed")
            && server.server["args"] == json!(["--image-mcp-stdio"])
    });
    if managed {
        merge_image_rule(content).map_err(crate::error::AppError::Message)
    } else {
        Ok(content.into())
    }
}

fn read_live_image_server(app: &AppType) -> Result<Option<Value>, String> {
    let value = match app {
        AppType::Codex => {
            let content = crate::codex_config::read_and_validate_codex_config_text()
                .map_err(|e| e.to_string())?;
            let parsed: toml::Value = toml::from_str(&content).map_err(|e| e.to_string())?;
            serde_json::to_value(parsed)
                .map_err(|e| e.to_string())?
                .pointer("/mcp_servers/oceanway_images")
                .cloned()
                .unwrap_or(Value::Null)
        }
        AppType::Claude => crate::claude_mcp::read_mcp_servers_map()
            .map_err(|e| e.to_string())?
            .remove("oceanway_images")
            .unwrap_or(Value::Null),
        AppType::Gemini => crate::gemini_mcp::read_mcp_servers_map()
            .map_err(|e| e.to_string())?
            .remove("oceanway_images")
            .unwrap_or(Value::Null),
        AppType::OpenCode => crate::opencode_config::get_mcp_servers()
            .map_err(|e| e.to_string())?
            .remove("oceanway_images")
            .unwrap_or(Value::Null),
        _ => return Err("此应用的图片 MCP 尚未适配".into()),
    };
    Ok((!value.is_null()).then_some(value))
}

fn expected_live_image_server(app: &AppType, spec: &Value) -> Value {
    if *app == AppType::OpenCode {
        let mut command = vec![spec["command"].clone()];
        command.extend(spec["args"].as_array().cloned().unwrap_or_default());
        return json!({"type":"local", "command":command,
            "environment":spec["env"], "enabled":true});
    }
    let mut expected = spec.clone();
    if *app == AppType::Codex {
        if let Some(object) = expected.as_object_mut() {
            object.remove("type");
        }
    }
    if *app == AppType::Gemini {
        if let Some(object) = expected.as_object_mut() {
            let timeout_ms = |object: &mut serde_json::Map<String, Value>,
                              seconds: &str,
                              millis: &str,
                              default: u64| {
                object
                    .remove(seconds)
                    .and_then(|value| value.as_f64().map(|n| (n * 1000.0) as u64))
                    .or_else(|| object.remove(millis).and_then(|value| value.as_u64()))
                    .unwrap_or(default)
            };
            let startup = timeout_ms(object, "startup_timeout_sec", "startup_timeout_ms", 10_000);
            let tool = timeout_ms(object, "tool_timeout_sec", "tool_timeout_ms", 60_000);
            object.insert("type".into(), json!("stdio"));
            object.insert("timeout".into(), json!(startup.max(tool)));
        }
    }
    expected
}

fn verify_live_image_server(app: &AppType, expected: &Value) -> Result<(), String> {
    if read_live_image_server(app)?.as_ref() != Some(&expected_live_image_server(app, expected)) {
        return Err("图片 MCP 未完整写入客户端配置，请重试".into());
    }
    Ok(())
}

fn ensure_image_server_ownership(
    previous: Option<&crate::McpServer>,
    apps: &crate::McpApps,
    spec: &Value,
) -> Result<(), String> {
    if let Some(previous) = previous {
        let owned = previous.homepage.as_deref() == Some(ORIGIN)
            && previous.tags.iter().any(|tag| tag == "oceanway-managed")
            && previous.server["args"] == json!(["--image-mcp-stdio"])
            && previous.server.pointer("/env/OCEANWAY_CONFIG_HOME")
                == spec.pointer("/env/OCEANWAY_CONFIG_HOME");
        if !owned {
            return Err(
                "已有同名 oceanway_images 工具，已保留原设置；请先在 MCP 管理中为原工具改名".into(),
            );
        }
    }
    for app in apps.enabled_apps() {
        let Some(live) = read_live_image_server(&app)? else {
            continue;
        };
        let is_current = live == expected_live_image_server(&app, spec);
        let is_previous =
            previous.is_some_and(|saved| live == expected_live_image_server(&app, &saved.server));
        if !is_current && !is_previous {
            return Err(format!(
                "{} 已有同名或被手动修改的 oceanway_images 工具，已保留原设置",
                app.as_str()
            ));
        }
    }
    Ok(())
}

fn check_image_runtime(spec: &Value, home: &Path) -> Result<(), String> {
    let executable = spec["command"]
        .as_str()
        .ok_or("图片工具缺少可执行文件路径")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "无法创建图片工具检查任务")?;
    let checked = runtime.block_on(crate::oceanway_images::mcp_stdio::check(
        Path::new(executable),
        home,
    ));
    runtime.shutdown_timeout(Duration::from_secs(5));
    let checked = checked.map_err(|error| format!("图片工具启动检查未通过：{error}"))?;
    if !checked.tools_available {
        return Err("图片工具未返回完整工具列表，请重新安装 Oceanway Config".into());
    }
    Ok(())
}

/// Immutable, verified preimages of configuration only. Session files are never
/// consumed by this writer; upstream history operations have their own backups.
fn backup_configs(root: &Path, selections: &[AppSelection]) -> Result<PathBuf, String> {
    let destination = root
        .join("backups")
        .join("oceanway-config")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
    let mut manifest = Vec::new();
    let mut paths = selections
        .iter()
        .map(|selection| {
            config_paths(&selection.app_id).map(|paths| (selection.app_id.as_str(), paths))
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.push(("oceanway", vec![root.join("image-provider.json")]));
    for (app_id, paths) in paths {
        for (index, source) in paths.into_iter().enumerate() {
            if !source.exists() {
                manifest.push(json!({"source":source,"existed":false}));
                continue;
            }
            let bytes = std::fs::read(&source).map_err(|e| format!("配置备份失败：{e}"))?;
            let target = destination.join(format!("{app_id}-{index}.backup"));
            crate::config::atomic_write_private(&target, &bytes).map_err(|e| e.to_string())?;
            if std::fs::read(&target).map_err(|e| e.to_string())? != bytes {
                return Err("配置备份校验失败，已停止配置".into());
            }
            manifest.push(json!({"source":source,"backup":target,"existed":true,"sha256":format!("{:x}",Sha256::digest(&bytes))}));
        }
    }
    crate::config::write_json_file_private(&destination.join("manifest.json"), &manifest)
        .map_err(|e| e.to_string())?;
    Ok(destination)
}

fn validate_selection(request: &ApplyRequest, models: &[Model]) -> Result<(), String> {
    if request.apps.is_empty() {
        return Err("请至少选择一个应用".into());
    }
    let allowed: HashSet<&str> = models.iter().map(|m| m.id.as_str()).collect();
    let mut apps = HashSet::new();
    for selection in &request.apps {
        app_type(&selection.app_id)?;
        if !apps.insert(&selection.app_id) {
            return Err("应用重复".into());
        }
        if !allowed.contains(selection.model.as_str()) {
            return Err(format!("当前 Key 无权使用模型 {}", selection.model));
        }
    }
    for model in request
        .allowed_image_models
        .iter()
        .chain(request.image_model.iter())
        .filter(|s| !s.is_empty())
    {
        if !allowed.contains(model.as_str()) {
            return Err(format!("当前 Key 无权使用图片模型 {model}"));
        }
    }
    Ok(())
}

fn retained_image_settings(
    selected: Option<&str>,
    allowed: &[String],
    previous: Option<&crate::oceanway_images::ProviderSummary>,
    available: &[Model],
) -> (Option<String>, Vec<String>, Option<String>) {
    if let Some(model) = selected {
        let mut models = allowed.to_vec();
        if !models.iter().any(|item| item == model) {
            models.push(model.to_owned());
        }
        return (Some(model.to_owned()), models, None);
    }
    let default_model = previous.and_then(|value| value.default_image_model.clone());
    let models = previous
        .map(|value| value.allowed_image_models.clone())
        .unwrap_or_default();
    let warning = default_model
        .as_ref()
        .filter(|model| !available.iter().any(|item| &item.id == *model))
        .map(|model| {
            format!("已保留原图片设置；当前 Key 无权使用图片模型 {model}，请重新选择图片模型")
        });
    (default_model, models, warning)
}

fn save_and_switch_provider(
    state: &AppState,
    app: &AppType,
    provider: Provider,
) -> Result<(), crate::AppError> {
    if *app == AppType::Codex {
        crate::mode::operation::settle(&state.db, "codex")?;
        crate::codex_history_migration::recover_oceanway_codex_history()?;
    }
    let apply = || {
        if let Some(existing) = state.db.get_provider_by_id(PROVIDER_ID, app.as_str())? {
            validate_provider(&existing).map_err(|_| {
                crate::AppError::Message("已有同名供应商，请先检查；不会覆盖其他服务的配置".into())
            })?;
            ProviderService::update(state, app.clone(), None, provider)?;
        } else {
            ProviderService::add_without_switching(state, app.clone(), provider)?;
        }
        ProviderService::switch(state, app.clone(), PROVIDER_ID).map(|_| ())
    };
    if *app == AppType::Codex {
        crate::codex_history_migration::with_oceanway_codex_history(|| {
            let result = apply();
            if result.is_err() {
                if let Err(recovery) = crate::mode::operation::settle(&state.db, "codex") {
                    return Err(crate::AppError::Message(format!(
                        "{}；原生配置恢复待重试：{recovery}",
                        result.unwrap_err()
                    )));
                }
            }
            result
        })
    } else {
        apply()
    }
}

#[tauri::command]
pub async fn oceanway_apply_config(
    state: tauri::State<'_, AppState>,
    request: ApplyRequest,
) -> Result<ApplyResult, String> {
    let _guard = APPLY_LOCK.lock().await;
    let key = resolve_key(&request.api_key)?;
    let models = fetch_models(&key).await?;
    validate_selection(&request, &models)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let home = crate::config::get_app_config_dir();
        let previous_mcp = state.db.get_all_mcp_servers().map_err(|e| e.to_string())?
            .get("oceanway_images").cloned();
        let mut backup_apps = request.apps.clone();
        if request.image_model.as_deref().is_some_and(|model| !model.is_empty()) {
            if let Some(previous) = &previous_mcp {
                for app in previous.apps.enabled_apps() {
                    if APPS.iter().any(|(id, _)| *id == app.as_str())
                        && !backup_apps.iter().any(|selection| selection.app_id == app.as_str()) {
                        backup_apps.push(AppSelection { app_id:app.as_str().into(), model:String::new() });
                    }
                }
            }
        }
        let backup = backup_configs(&home, &backup_apps)?;
        // Upstream supports preserving the original ChatGPT login alongside the
        // custom provider. Oceanway makes this invariant, not a user toggle.
        let mut settings = crate::settings::get_settings();
        settings.preserve_codex_official_auth_on_switch = true;
        crate::settings::update_settings(settings).map_err(|e| e.to_string())?;
        let image_model = request.image_model.as_deref().filter(|s| !s.is_empty());
        let previous_images = if home.join("image-provider.json").exists() {
            Some(crate::oceanway_images::read_provider_summary(&home)?)
        } else { None };
        let (effective_image_model, image_models, preserved_image_warning) = retained_image_settings(
            image_model, &request.allowed_image_models, previous_images.as_ref(), &models);
        let mut results = Vec::new();
        let mut mcp_apps = crate::McpApps::default();
        for selection in request.apps {
            let outcome = (|| -> Result<(), String> {
                let app = app_type(&selection.app_id)?;
                let provider = provider_for(&selection, &key)?;
                save_and_switch_provider(&state, &app, provider).map_err(|e| e.to_string())?;
                if app == AppType::OpenCode {
                    crate::opencode_config::set_default_model(&format!(
                        "{PROVIDER_ID}/{}",
                        selection.model
                    ))
                    .map_err(|e| e.to_string())?;
                }
                if app == AppType::OpenClaw {
                    crate::openclaw_config::set_default_model(
                        &crate::openclaw_config::OpenClawDefaultModel {
                            primary: format!("{PROVIDER_ID}/{}", selection.model),
                            fallbacks: vec![],
                            extra: Default::default(),
                        },
                    )
                    .map_err(|e| e.to_string())?;
                }
                mcp_apps.set_enabled_for(&app, image_model.is_some());
                Ok(())
            })();
            results.push(AppResult {
                app_id: selection.app_id,
                ok: outcome.is_ok(),
                image_configured: false,
                image_message: None,
                message: match outcome {
                    Ok(()) => "配置已保存，请重开应用加载新配置".into(),
                    Err(e) => crate::redact_known_secrets_strict(&e, &[key.clone()]),
                },
            });
        }
        let mut images_configured = false;
        let image_outcome = (|| -> Result<(), String> {
            if !results.iter().any(|r| r.ok) {
                return Ok(());
            }
            crate::oceanway_images::configure(&home, &key, effective_image_model.as_deref(), &image_models)?;
            if image_model.is_some() && !mcp_apps.is_empty() {
                let spec = crate::oceanway_images::mcp_server_spec(&home)?;
                if let Some(previous) = &previous_mcp {
                    for app in previous.apps.enabled_apps() {
                        mcp_apps.set_enabled_for(&app, true);
                    }
                }
                ensure_image_server_ownership(previous_mcp.as_ref(), &mcp_apps, &spec)?;
                let rule_apps = mcp_apps.enabled_apps().into_iter().filter(|app|
                    results.iter().any(|r|r.ok && r.app_id == app.as_str())).collect::<Vec<_>>();
                let sync_error=crate::McpService::upsert_server(
                    &state,
                    crate::McpServer {
                        id: "oceanway_images".into(),
                        name: "Oceanway 图片".into(),
                        server: spec.clone(),
                        apps: mcp_apps,
                        description: Some(
                            "直接通过 Oceanway 图片模型生成、查询及取消图片任务".into(),
                        ),
                        homepage: Some(ORIGIN.into()),
                        docs: None,
                        tags: vec!["image".into(), "oceanway-managed".into()],
                    },
                ).err().map(|e|e.to_string());
                let runtime_check = check_image_runtime(&spec, &home);
                for app in rule_apps {
                    let outcome=verify_live_image_server(&app,&spec)
                        .and_then(|_|runtime_check.clone())
                        .and_then(|_|install_image_rule(&state,&app));
                    if let Some(result)=results.iter_mut().find(|r|r.app_id==app.as_str()) {
                        result.image_configured=outcome.is_ok();
                        result.image_message=Some(match outcome {
                            Ok(())=>"图片 MCP 已写入，并通过本机启动和四项工具检查；首次调用若提示授权，只需授权 Oceanway 图片工具".into(),
                            Err(e)=>crate::redact_known_secrets_strict(&format!("{e}{}",sync_error.as_ref().map(|s|format!("：{s}")).unwrap_or_default()),&[key.clone()]),
                        });
                        if !result.image_configured {result.message.push_str("；图片配置未完成，请查看图片状态");}
                    }
                }
                images_configured=results.iter().filter(|r|r.ok).all(|r|r.image_configured);
            } else if image_model.is_none() && previous_images.is_some() {
                for result in results.iter_mut().filter(|result| result.ok) {
                    result.image_message=Some(preserved_image_warning.clone()
                        .unwrap_or_else(|| "本次未修改已有图片设置".into()));
                    if let Some(warning) = &preserved_image_warning {
                        result.message.push_str(&format!("；{warning}"));
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = image_outcome {
            images_configured = false;
            let safe = crate::redact_known_secrets_strict(&error, &[key.clone()]);
            for result in results.iter_mut().filter(|r| r.ok) {
                result.message = format!(
                    "文本配置已保存；图片配置未完成：{safe}。原配置备份：{}",
                    backup.display()
                );
                result.image_message=Some(safe.clone());
            }
        }
        if image_model.is_some() {
            for result in results
                .iter_mut()
                .filter(|r| r.ok && r.app_id == "openclaw")
            {
                result
                    .message
                    .push_str("。OpenClaw 的图片工具尚需单独适配，本次仅保存文本模型");
                result.image_message=Some("OpenClaw 的图片 MCP 尚未适配".into());
            }
        }
        let restart_required = results.iter().any(|r| r.ok);
        Ok(ApplyResult {
            results,
            restart_required,
            backup_path: backup.display().to_string(),
            images_configured,
        })
    })
    .await
    .map_err(|_| "配置任务异常，请查看备份和应用日志".to_owned())?
}

#[tauri::command]
pub async fn oceanway_start_image_job(
    jobs: tauri::State<'_, crate::oceanway_images::ImageJobs>,
    request: crate::oceanway_images::ImageTestRequest,
) -> Result<crate::oceanway_images::ImageJob, String> {
    let jobs = jobs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || jobs.start(request, None, false))
        .await
        .map_err(|_| "图片任务启动异常".to_owned())?
}
#[tauri::command]
pub async fn oceanway_get_image_job(
    jobs: tauri::State<'_, crate::oceanway_images::ImageJobs>,
    job_id: String,
) -> Result<crate::oceanway_images::ImageJob, String> {
    let jobs = jobs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || jobs.status(&job_id))
        .await
        .map_err(|_| "图片任务读取异常".to_owned())?
}
#[tauri::command]
pub async fn oceanway_cancel_image_job(
    jobs: tauri::State<'_, crate::oceanway_images::ImageJobs>,
    job_id: String,
) -> Result<crate::oceanway_images::ImageJob, String> {
    let jobs = jobs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || jobs.cancel(&job_id))
        .await
        .map_err(|_| "图片任务取消异常".to_owned())?
}
#[tauri::command]
pub async fn oceanway_retry_image_job(
    jobs: tauri::State<'_, crate::oceanway_images::ImageJobs>,
    job_id: String,
) -> Result<crate::oceanway_images::ImageJob, String> {
    let jobs = jobs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || jobs.retry(&job_id))
        .await
        .map_err(|_| "图片任务重试异常".to_owned())?
}
#[tauri::command]
pub async fn oceanway_open_image(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, crate::oceanway_images::ImageJobs>,
    job_id: String,
    index: usize,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = jobs.result_path(&job_id, index)?;
    app.opener()
        .open_path(path.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn oceanway_launch_app(app_id: String, restart: bool) -> Result<LaunchResult, String> {
    app_type(&app_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        if app_id == "codex" {
            if let Some(message) = crate::oceanway_desktop::launch_codex(restart)? {
                return Ok(LaunchResult {
                    launched: true,
                    restart_required: false,
                    message,
                });
            }
        }
        crate::commands::misc::launch_oceanway_terminal(&app_id)?;
        Ok(LaunchResult {
            launched: true,
            restart_required: false,
            message: "已打开新终端加载配置；已有终端会话继续保留".into(),
        })
    })
    .await
    .map_err(|_| "启动任务失败".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oceanway_codex_desktop_is_configurable_without_cli_and_cli_does_not_imply_desktop() {
        let desktop_only = json!({"name":"codex", "version":null, "desktop_installed":true});
        assert_eq!(
            app_installation_state("codex", Some(&desktop_only)),
            (true, true)
        );
        let cli_only = json!({"name":"codex", "version":"1.0.0", "desktop_installed":false});
        assert_eq!(
            app_installation_state("codex", Some(&cli_only)),
            (true, false)
        );
        assert_eq!(
            app_installation_state("claude", Some(&desktop_only)),
            (false, false)
        );
        assert_eq!(app_installation_state("codex", None), (false, false));
    }
    #[test]
    fn origin_rejects_other_hosts_and_url_tricks() {
        for url in [
            "http://console.oceanway.tech/v1",
            "https://console.oceanway.tech.evil.test/v1",
            "https://console.oceanway.tech@evil.test/v1",
            "https://console.oceanway.tech:8443/v1",
            "https://console.oceanway.tech/v1?token=x",
        ] {
            assert!(validate_origin(url).is_err(), "{url}");
        }
        assert!(validate_origin(API_BASE).is_ok());
    }
    #[test]
    fn built_providers_are_oceanway_only() {
        for (id, _) in APPS {
            let selection = AppSelection {
                app_id: (*id).into(),
                model: "fixture-model".into(),
            };
            let mut provider = provider_for(&selection, "fixture-key").unwrap();
            assert!(validate_provider(&provider).is_ok());
            provider.id = "imported-third-party".into();
            assert!(validate_provider(&provider).is_err());
        }
    }
    #[test]
    fn permissions_are_checked_before_writing() {
        let models =
            parse_models(&json!({"data":[{"id":"gpt-fixture"},{"id":"gpt-fixture"}]})).unwrap();
        assert_eq!(models.len(), 1);
        let request = ApplyRequest {
            api_key: "fixture".into(),
            apps: vec![AppSelection {
                app_id: "codex".into(),
                model: "not-authorized".into(),
            }],
            image_model: None,
            allowed_image_models: vec![],
        };
        assert!(validate_selection(&request, &models).is_err());
    }
    #[test]
    fn image_rule_preserves_original_instructions_and_is_idempotent() {
        let original = "# Existing user instructions\nDo not delete files.\n";
        let once = merge_image_rule(original).unwrap();
        assert!(once.starts_with(original));
        assert_eq!(merge_image_rule(&once).unwrap(), once);
        assert!(merge_image_rule(IMAGE_RULE_START).is_err());
    }

    #[test]
    fn skipping_image_configuration_retains_previous_model_and_reports_lost_permission() {
        let previous = crate::oceanway_images::ProviderSummary {
            endpoint: API_BASE.into(),
            key_configured: true,
            default_image_model: Some("image-fixture".into()),
            allowed_image_models: vec!["image-fixture".into()],
        };
        let (model, allowed, warning) = retained_image_settings(None, &[], Some(&previous), &[]);
        assert_eq!(model.as_deref(), Some("image-fixture"));
        assert_eq!(allowed, previous.allowed_image_models);
        assert!(warning.unwrap().contains("当前 Key 无权使用"));
        let available = vec![Model {
            id: "image-fixture".into(),
        }];
        assert!(
            retained_image_settings(None, &[], Some(&previous), &available)
                .2
                .is_none()
        );
        let (model, allowed, warning) =
            retained_image_settings(Some("new-image"), &[], Some(&previous), &[]);
        assert_eq!(model.as_deref(), Some("new-image"));
        assert_eq!(allowed, vec!["new-image".to_owned()]);
        assert!(warning.is_none());
    }

    struct FixtureHome {
        previous: Option<std::ffi::OsString>,
        directory: tempfile::TempDir,
    }
    impl FixtureHome {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", directory.path());
            crate::settings::reload_settings().unwrap();
            Self {
                previous,
                directory,
            }
        }
    }
    impl Drop for FixtureHome {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn codex_override_rule_and_backup_preserve_original_files_fixture() {
        let fixture = FixtureHome::new();
        let codex = fixture.directory.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let standard = "Keep standard instructions byte-for-byte.\n";
        let original = "# Existing override\nNever remove the customer's files.\n";
        std::fs::write(codex.join("AGENTS.md"), standard).unwrap();
        std::fs::write(codex.join("AGENTS.override.md"), original).unwrap();
        let backup = backup_configs(
            &crate::config::get_app_config_dir(),
            &[AppSelection {
                app_id: "codex".into(),
                model: "fixture".into(),
            }],
        )
        .unwrap();
        let state = AppState::new(std::sync::Arc::new(crate::Database::memory().unwrap()));
        install_image_rule(&state, &AppType::Codex).unwrap();
        let effective = std::fs::read_to_string(codex.join("AGENTS.override.md")).unwrap();
        assert!(effective.starts_with(original));
        assert!(effective.contains(IMAGE_RULE_START));
        assert_eq!(
            std::fs::read_to_string(codex.join("AGENTS.md")).unwrap(),
            standard
        );
        let manifest: Value = crate::config::read_json_file(&backup.join("manifest.json")).unwrap();
        let entry = manifest
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| {
                entry["source"]
                    .as_str()
                    .is_some_and(|source| source.ends_with("AGENTS.override.md"))
            })
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(entry["backup"].as_str().unwrap()).unwrap(),
            original
        );
    }

    #[test]
    #[serial_test::serial]
    fn image_registration_preserves_other_tools_and_refuses_same_name_conflicts_fixture() {
        let _fixture = FixtureHome::new();
        let codex = crate::codex_config::get_codex_config_dir();
        std::fs::create_dir_all(&codex).unwrap();
        std::fs::write(
            codex.join("config.toml"),
            "[mcp_servers.customer_tool]\ncommand = \"keep-me\"\n",
        )
        .unwrap();
        let claude = crate::config::get_claude_settings_path();
        std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
        crate::claude_mcp::set_mcp_servers_map(&std::collections::HashMap::from([(
            "customer_tool".into(),
            json!({"command":"keep-me"}),
        )]))
        .unwrap();
        crate::gemini_mcp::set_mcp_servers_map(&std::collections::HashMap::from([(
            "customer_tool".into(),
            json!({"command":"keep-me"}),
        )]))
        .unwrap();
        crate::opencode_config::set_mcp_server(
            "customer_tool",
            json!({"type":"local", "command":["keep-me"], "enabled":true}),
        )
        .unwrap();
        let state = AppState::new(std::sync::Arc::new(crate::Database::memory().unwrap()));
        let apps = crate::McpApps {
            codex: true,
            claude: true,
            gemini: true,
            opencode: true,
            ..Default::default()
        };
        let spec = json!({"command":"fixture-oceanway", "args":["--image-mcp-stdio"],
            "env":{"OCEANWAY_CONFIG_HOME":crate::config::get_app_config_dir()},
            "startup_timeout_sec":30, "tool_timeout_sec":60});
        ensure_image_server_ownership(None, &apps, &spec).unwrap();
        let server = crate::McpServer {
            id: "oceanway_images".into(),
            name: "Oceanway 图片".into(),
            server: spec.clone(),
            apps: apps.clone(),
            description: None,
            homepage: Some(ORIGIN.into()),
            docs: None,
            tags: vec!["oceanway-managed".into()],
        };
        crate::McpService::upsert_server(&state, server.clone()).unwrap();
        for app in [
            AppType::Codex,
            AppType::Claude,
            AppType::Gemini,
            AppType::OpenCode,
        ] {
            verify_live_image_server(&app, &spec)
                .unwrap_or_else(|error| panic!("{}: {error}", app.as_str()));
            let prompt_path = crate::prompt_files::prompt_file_path(&app).unwrap();
            let original = format!(
                "# Original {} rules\nKeep customer instructions.\n",
                app.as_str()
            );
            crate::config::write_text_file(&prompt_path, &original).unwrap();
            install_image_rule(&state, &app).unwrap();
            let once = std::fs::read_to_string(&prompt_path).unwrap();
            assert!(once.starts_with(&original));
            assert!(once.contains("oceanway_images MCP tools: generate_images"));
            assert!(once.contains("count equal to the total requested"));
            install_image_rule(&state, &app).unwrap();
            assert_eq!(std::fs::read_to_string(prompt_path).unwrap(), once);
        }
        ensure_image_server_ownership(Some(&server), &apps, &spec).unwrap();
        assert!(std::fs::read_to_string(codex.join("config.toml"))
            .unwrap()
            .contains("customer_tool"));
        let mut claude_servers = crate::claude_mcp::read_mcp_servers_map().unwrap();
        assert_eq!(claude_servers["customer_tool"]["command"], "keep-me");
        assert_eq!(
            crate::gemini_mcp::read_mcp_servers_map().unwrap()["customer_tool"]["command"],
            "keep-me"
        );
        assert_eq!(
            crate::opencode_config::get_mcp_servers().unwrap()["customer_tool"]["command"],
            json!(["keep-me"])
        );
        claude_servers.insert(
            "oceanway_images".into(),
            json!({"command":"customer-owned"}),
        );
        crate::claude_mcp::set_mcp_servers_map(&claude_servers).unwrap();
        let before = std::fs::read(crate::config::get_claude_mcp_path()).unwrap();
        assert!(ensure_image_server_ownership(Some(&server), &apps, &spec).is_err());
        assert_eq!(
            std::fs::read(crate::config::get_claude_mcp_path()).unwrap(),
            before
        );
        let mut unrelated = server;
        unrelated.tags.clear();
        assert!(
            ensure_image_server_ownership(Some(&unrelated), &crate::McpApps::default(), &spec)
                .is_err()
        );
    }

    #[test]
    #[serial_test::serial]
    fn managed_image_rule_survives_prompt_changes_and_mcp_resync_fixture() {
        let _fixture = FixtureHome::new();
        let state = AppState::new(std::sync::Arc::new(crate::Database::memory().unwrap()));
        let spec = json!({"command":"fixture-oceanway", "args":["--image-mcp-stdio"],
            "env":{"OCEANWAY_CONFIG_HOME":crate::config::get_app_config_dir()},
            "startup_timeout_sec":30, "tool_timeout_sec":60});
        let server = crate::McpServer {
            id: "oceanway_images".into(),
            name: "Oceanway 图片".into(),
            server: spec,
            apps: crate::McpApps {
                codex: true,
                claude: true,
                gemini: true,
                opencode: true,
                ..Default::default()
            },
            description: None,
            homepage: Some(ORIGIN.into()),
            docs: None,
            tags: vec!["oceanway-managed".into()],
        };
        state.db.save_mcp_server(&server).unwrap();
        for app in [
            AppType::Codex,
            AppType::Claude,
            AppType::Gemini,
            AppType::OpenCode,
        ] {
            let path = crate::prompt_files::prompt_file_path(&app).unwrap();
            let mut prompt = crate::prompt::Prompt {
                id: "customer-template".into(),
                name: "Customer template".into(),
                content: "# Customer template\nUse concise replies.\n".into(),
                description: None,
                enabled: false,
                created_at: None,
                updated_at: None,
            };
            crate::PromptService::upsert_prompt(&state, app.clone(), &prompt.id, prompt.clone())
                .unwrap();
            crate::PromptService::enable_prompt(&state, app.clone(), &prompt.id).unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.starts_with(&prompt.content));
            assert_eq!(content.matches(IMAGE_RULE_START).count(), 1);
            assert!(crate::PromptService::delete_prompt(&state, app.clone(), &prompt.id).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);

            prompt.enabled = true;
            prompt.content = "# Edited customer template\n".into();
            crate::PromptService::upsert_prompt(&state, app.clone(), &prompt.id, prompt.clone())
                .unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.starts_with(&prompt.content));
            assert_eq!(content.matches(IMAGE_RULE_START).count(), 1);

            state.db.save_prompt(app.as_str(), &prompt).unwrap();
            crate::PromptService::sync_to_live(&state, app.clone()).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
            crate::McpService::upsert_server(&state, server.clone()).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);

            prompt.enabled = false;
            crate::PromptService::upsert_prompt(&state, app.clone(), &prompt.id, prompt.clone())
                .unwrap();
            let rule_only = std::fs::read_to_string(&path).unwrap();
            assert!(rule_only.contains(IMAGE_RULE_START));
            assert!(!rule_only.contains("Edited customer template"));
            crate::PromptService::sync_to_live(&state, app.clone()).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), rule_only);
            crate::PromptService::delete_prompt(&state, app.clone(), &prompt.id).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), rule_only);

            state
                .db
                .update_mcp_server_app_enabled("oceanway_images", &app, false)
                .unwrap();
            prompt.enabled = true;
            crate::PromptService::upsert_prompt(&state, app.clone(), &prompt.id, prompt.clone())
                .unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), prompt.content);
        }
    }

    #[test]
    #[serial_test::serial]
    fn codex_claude_configuration_preserves_history_and_old_login_fixture() {
        let fixture = FixtureHome::new();
        let root = fixture.directory.path();
        let codex = root.join(".codex");
        let claude = root.join(".claude");
        std::fs::create_dir_all(codex.join("sessions")).unwrap();
        std::fs::create_dir_all(claude.join("projects/fixture")).unwrap();
        let history=b"{\"type\":\"fixture\",\"message\":\"original conversation and attachment reference\"}\n";
        let codex_history = codex.join("sessions/fixture.jsonl");
        let claude_history = claude.join("projects/fixture/session.jsonl");
        let codex_original = format!(
            "{}\n{}",
            json!({"type":"session_meta","payload":{
            "id":"fixture-session","model_provider":"openai","cwd":"/fixture/project"}}),
            String::from_utf8_lossy(history)
        );
        std::fs::write(&codex_history, &codex_original).unwrap();
        std::fs::write(&claude_history, history).unwrap();
        std::fs::write(
            codex.join("config.toml"),
            "# Customer setting\n[projects.fixture]\ntrust_level = \"trusted\"\n[features]\nimage_generation = true\nshell_tool = true\n",
        )
        .unwrap();
        let original_auth = json!({"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{
            "id_token":crate::codex_config::test_codex_id_token("fixture-user"),
            "access_token":"fixture-access","refresh_token":"fixture-refresh","account_id":"fixture-account"}});
        crate::config::write_json_file_private(&codex.join("auth.json"), &original_auth).unwrap();
        crate::config::write_json_file(
            &claude.join("settings.json"),
            &json!({"permissions":{"allow":["Read"]},"env":{"CUSTOM_KEEP":"yes"}}),
        )
        .unwrap();
        let selections = vec![
            AppSelection {
                app_id: "codex".into(),
                model: "gpt-fixture".into(),
            },
            AppSelection {
                app_id: "claude".into(),
                model: "claude-fixture".into(),
            },
        ];
        let backup = backup_configs(&crate::config::get_app_config_dir(), &selections).unwrap();
        let state = AppState::new(std::sync::Arc::new(crate::Database::memory().unwrap()));
        for selection in &selections {
            let app = app_type(&selection.app_id).unwrap();
            save_and_switch_provider(
                &state,
                &app,
                provider_for(selection, "fixture-key").unwrap(),
            )
            .unwrap();
        }
        let codex_updated = std::fs::read_to_string(&codex_history).unwrap();
        assert_eq!(
            codex_updated.split_once('\n').unwrap().1.as_bytes(),
            history
        );
        let meta: Value = serde_json::from_str(codex_updated.lines().next().unwrap()).unwrap();
        assert_eq!(meta["payload"]["id"], "fixture-session");
        assert_eq!(meta["payload"]["model_provider"], "custom");
        assert_eq!(std::fs::read(&claude_history).unwrap(), history);
        let auth: Value = crate::config::read_json_file(&codex.join("auth.json")).unwrap();
        assert_eq!(auth["tokens"], original_auth["tokens"]);
        let config = std::fs::read_to_string(codex.join("config.toml")).unwrap();
        assert!(config.contains("trust_level"));
        assert!(config.contains(API_BASE));
        let parsed: toml::Value = toml::from_str(&config).unwrap();
        assert_eq!(
            parsed["features"]["image_generation"].as_bool(),
            Some(false)
        );
        assert_eq!(parsed["features"]["shell_tool"].as_bool(), Some(true));
        let settings: Value = crate::config::read_json_file(&claude.join("settings.json")).unwrap();
        assert_eq!(settings["env"]["CUSTOM_KEEP"], "yes");
        assert_eq!(settings["permissions"]["allow"], json!(["Read"]));
        assert!(backup.join("manifest.json").is_file());
        assert!(!root.join(".cc-switch").exists());
    }

    #[test]
    #[serial_test::serial]
    fn oceanway_history_preflight_failure_does_not_update_active_provider_or_config() {
        let fixture = FixtureHome::new();
        let codex = fixture.directory.path().join(".codex");
        let state = AppState::new(std::sync::Arc::new(crate::Database::memory().unwrap()));
        let selection = AppSelection {
            app_id: "codex".into(),
            model: "gpt-original".into(),
        };
        save_and_switch_provider(
            &state,
            &AppType::Codex,
            provider_for(&selection, "fixture-key").unwrap(),
        )
        .unwrap();
        let before = std::fs::read(codex.join("config.toml")).unwrap();
        let provider_before = state
            .db
            .get_provider_by_id(PROVIDER_ID, "codex")
            .unwrap()
            .unwrap();
        std::fs::write(codex.join("state_5.sqlite"), b"fixture unreadable DB").unwrap();
        let changed = AppSelection {
            app_id: "codex".into(),
            model: "gpt-changed".into(),
        };
        assert!(save_and_switch_provider(
            &state,
            &AppType::Codex,
            provider_for(&changed, "fixture-key").unwrap()
        )
        .is_err());
        assert_eq!(std::fs::read(codex.join("config.toml")).unwrap(), before);
        assert_eq!(
            state
                .db
                .get_provider_by_id(PROVIDER_ID, "codex")
                .unwrap()
                .unwrap()
                .settings_config,
            provider_before.settings_config
        );
    }

    #[test]
    #[serial_test::serial]
    fn invalid_client_config_is_not_replaced_fixture() {
        let fixture = FixtureHome::new();
        let codex = fixture.directory.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let malformed = "model = [ incomplete";
        std::fs::write(codex.join("config.toml"), malformed).unwrap();
        let state = AppState::new(std::sync::Arc::new(crate::Database::memory().unwrap()));
        let selection = AppSelection {
            app_id: "codex".into(),
            model: "gpt-fixture".into(),
        };
        let configured = ProviderService::add_without_switching(
            &state,
            AppType::Codex,
            provider_for(&selection, "fixture-key").unwrap(),
        )
        .and_then(|_| ProviderService::switch(&state, AppType::Codex, PROVIDER_ID));
        assert!(configured.is_err());
        assert_eq!(
            std::fs::read_to_string(codex.join("config.toml")).unwrap(),
            malformed
        );
    }
}
