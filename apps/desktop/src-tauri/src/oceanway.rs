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
const WEBSITE: &str = "https://oceanway.tech";
const PROVIDER_ID: &str = "oceanway";
const APPS: &[(&str, &str)] = &[
    ("codex", "Codex"), ("claude-desktop", "Claude Desktop"), ("claude", "Claude Code"),
    ("dsh", "DeepSeek Harness"), ("hermes", "Hermes"), ("kimi", "Kimi Code Desktop"),
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
    allowed_image_models: Vec<String>,
    app_models: serde_json::Map<String, Value>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(default, rename = "supportedProtocols", skip_serializing_if = "Option::is_none")]
    pub supported_protocols: Option<Vec<String>>,
    #[serde(default, rename = "modelType", skip_serializing_if = "Option::is_none")]
    pub model_type: Option<String>,
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
    install_available: bool,
    reason: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSelection {
    app_id: String,
    model: String,
    #[serde(default)]
    models: Vec<String>,
}

impl AppSelection {
    fn selected_models(&self) -> Vec<String> {
        let mut selected = vec![self.model.clone()];
        for model in &self.models {
            if !selected.contains(model) { selected.push(model.clone()); }
        }
        selected
    }
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

/// Validate configuration created or edited by the Oceanway setup flow.
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

/// Existing connections may be restored using CC Switch's native adapters.
/// Keep the Oceanway-owned entry pinned to its gateway when switching as well.
pub fn validate_provider_switch(provider: &Provider) -> Result<(), String> {
    if provider.id == PROVIDER_ID {
        validate_provider(provider)?;
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
    let mut models = parse_models(&payload)?;
    let offering: Value = client
        .get(format!("{ORIGIN}/api/platform/v1/public/offering"))
        .send().await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| "暂时无法读取模型兼容信息，请重试")?
        .json().await.map_err(|_| "模型兼容信息无效，请重试")?;
    attach_model_protocols(&mut models, &offering)?;
    Ok(models)
}

fn attach_model_protocols(models: &mut [Model], offering: &Value) -> Result<(), String> {
    let rows = offering.pointer("/data/models").and_then(Value::as_array)
        .ok_or("模型兼容信息缺少 models")?;
    for model in models {
        // The authenticated catalog determines entitlement. Public metadata only
        // describes how those already-authorized models can be invoked.
        model.supported_protocols = Some(rows.iter()
            .find(|row| row["model_name"].as_str() == Some(&model.id))
            .and_then(|row| row["supported_protocols"].as_array())
            .map(|values| values.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default());
        model.model_type = rows.iter().find(|row| row["model_name"].as_str() == Some(&model.id))
            .and_then(|row| row["model_type"].as_str()).map(str::to_owned);
    }
    Ok(())
}

fn conversation_protocol(app: &str, default_model: &str) -> &'static str {
    match app {
        "codex" => "openai-responses",
        "claude" | "claude-desktop" => "anthropic-messages",
        "hermes" => {
            let family = default_model.rsplit('/').next().unwrap_or(default_model).to_ascii_lowercase();
            if family.starts_with("gpt-") || family.starts_with("o1") || family.starts_with("o3") || family.starts_with("o4") {
                "openai-responses"
            } else if family.starts_with("claude-") { "anthropic-messages" }
            else { "openai-chat" }
        },
        _ => "openai-chat",
    }
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
                models.push(Model { id: id.to_owned(), supported_protocols: None, model_type: None });
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
    let mut app_models = serde_json::Map::new();
    for (id, _) in APPS {
        if let Some(provider) = state
            .db
            .get_provider_by_id(PROVIDER_ID, id)
            .map_err(|e| e.to_string())?
        {
            configured_apps.push((*id).to_owned());
            if let Some(selection) = provider_model_selection(id, &provider) {
                app_models.insert((*id).into(), selection);
            }
        }
    }
    let configured = summary.as_ref().is_some_and(|s| s.key_configured);
    Ok(ConnectionSummary {
        configured,
        key_preview: configured.then(|| "已保存".into()),
        base_url: API_BASE,
        configured_apps,
        default_image_model: summary.as_ref().and_then(|s| s.default_image_model.clone()),
        allowed_image_models: summary.map(|s| s.allowed_image_models).unwrap_or_default(),
        app_models,
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
    let raw = crate::commands::get_tool_versions(Some(vec!["claude".into()]), None).await?;
    let versions = serde_json::to_value(raw).map_err(|_| "无法读取应用状态")?;
    let probes = APPS.iter().map(|(id,name)| {
        let id=(*id).to_owned(); let name=(*name).to_owned(); let versions=versions.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let probe = if id=="claude" {
                Ok(versions.as_array().is_some_and(|rows|rows.iter().any(|row|row["name"]=="claude" && row["version"].is_string())))
            } else if id=="codex" { Ok(crate::codex_desktop_detection::is_installed()) }
            else {crate::agent_desktops::find(&id).map(|path|path.is_some())};
            let installed=probe.as_ref().copied().unwrap_or(false);
            let install_available=!(id=="hermes" && cfg!(all(target_os="macos",target_arch="x86_64")));
            DetectedApp {install_available,app_id:id.clone(),name,installed,desktop_installed:installed && id!="claude",
                configurable:true,config_exists:config_paths(&id).map(|paths|paths.iter().any(|p|p.is_file())).unwrap_or(false),
                reason:Some(if !install_available {"Hermes 暂未提供 Intel Mac 完整离线包".into()}else{match probe {Ok(true)=>"可以配置".into(),Ok(false)=>"尚未安装，请点击下载安装".into(),Err(e)=>e}})}
        })
    });
    futures::future::join_all(probes).await.into_iter().map(|result|result.map_err(|e|e.to_string())).collect()
}

#[cfg(test)]
fn app_installation_state(id: &str, item: Option<&Value>) -> (bool, bool) {
    let desktop = id != "claude" && item.is_some_and(|v| v["desktop_installed"] == true);
    let cli = id == "claude" && item.is_some_and(|v| v["version"].is_string());
    (desktop || cli, desktop)
}
fn app_type(id: &str) -> Result<AppType, String> {
    match id {
        "codex"=>Ok(AppType::Codex), "claude"=>Ok(AppType::Claude),
        "claude-desktop"=>Ok(AppType::ClaudeDesktop), "dsh"=>Ok(AppType::Dsh),
        "hermes"=>Ok(AppType::Hermes), "kimi"=>Ok(AppType::Kimi),
        _=>Err("未支持的应用".into()),
    }
}

fn provider_for(selection: &AppSelection, key: &str) -> Result<Provider, String> {
    let model = &selection.model;
    let selected = selection.selected_models();
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
            json!({"auth":{"OPENAI_API_KEY":key}, "config":doc.to_string(),
                "modelCatalog":{"models":selected.iter().map(|id|json!({"model":id,"displayName":id})).collect::<Vec<_>>()}})
        }
        "claude" | "claude-desktop" => json!({"env":{"ANTHROPIC_BASE_URL":ORIGIN,"ANTHROPIC_AUTH_TOKEN":key,
            "ANTHROPIC_MODEL":model,"ANTHROPIC_DEFAULT_OPUS_MODEL":model,"ANTHROPIC_DEFAULT_SONNET_MODEL":model,"ANTHROPIC_DEFAULT_HAIKU_MODEL":model},
            "modelPicker":{"replaceBuiltInOptions":true,"options":selected.iter().map(|id|json!({"model":id,"label":id,"description":"Oceanway"})).collect::<Vec<_>>()}}),
        "dsh" => json!({"name":"Oceanway","api":"openai-completions","defaultModel":model,"options":{"baseURL":API_BASE,"apiKey":key},
            "models":selected.iter().map(|id|(id.clone(),json!({"name":id}))).collect::<serde_json::Map<_,_>>()}),
        "hermes" => {
            let (mode, base) = match conversation_protocol("hermes", model) {
                "openai-responses" => ("codex_responses", API_BASE),
                "anthropic-messages" => ("anthropic_messages", ORIGIN),
                _ => ("chat_completions", API_BASE),
            };
            json!({"base_url":base,"api_key":key,"api_mode":mode,"models":selected.iter().map(|id|json!({"id":id,"name":id})).collect::<Vec<_>>()})
        },
        "kimi" => json!({"provider":{"type":"openai","base_url":API_BASE,"api_key":key},
            "models":selected.iter().enumerate().map(|(index,id)|
                (if index==0 {PROVIDER_ID.into()} else {format!("{PROVIDER_ID}/{id}")},
                json!({"provider":PROVIDER_ID,"model":id,"max_context_size":128000}))).collect::<serde_json::Map<_,_>>()}),
        _ => return Err("未支持的应用".into()),
    };
    let mut provider = Provider::with_id(
        PROVIDER_ID.into(),
        "Oceanway".into(),
        settings,
        Some(WEBSITE.into()),
    );
    if selection.app_id=="claude-desktop" {
        if selected.iter().any(|id| !crate::claude_desktop_config::is_claude_safe_model_id(id)) {return Err("Claude Desktop 请选择 Claude 系列模型".into());}
        let routes=selected.iter().map(|id|(id.clone(),json!({"model":id}))).collect::<serde_json::Map<_,_>>();
        provider.meta=Some(serde_json::from_value(json!({"claudeDesktopModelRoutes":routes})).map_err(|e|e.to_string())?);
    }
    validate_provider(&provider)?;
    Ok(provider)
}

fn provider_model_selection(app: &str, provider: &Provider) -> Option<Value> {
    let settings = &provider.settings_config;
    let models: Vec<String> = match app {
        "codex" => settings.pointer("/modelCatalog/models")?.as_array()?.iter()
            .filter_map(|row| row["model"].as_str().map(str::to_owned)).collect(),
        "claude" | "claude-desktop" => settings.pointer("/modelPicker/options")?.as_array()?.iter()
            .filter_map(|row| row["model"].as_str().map(str::to_owned)).collect(),
        "dsh" => settings["models"].as_object()?.keys().cloned().collect(),
        "hermes" => settings["models"].as_array()?.iter().filter_map(|row|row["id"].as_str().map(str::to_owned)).collect(),
        "kimi" => settings["models"].as_object()?.values().filter_map(|row|row["model"].as_str().map(str::to_owned)).collect(),
        _ => return None,
    };
    let model = if app == "codex" {
        toml::from_str::<toml::Value>(settings["config"].as_str()?).ok()?["model"].as_str()?.to_owned()
    } else if app.starts_with("claude") { settings.pointer("/env/ANTHROPIC_MODEL")?.as_str()?.to_owned() }
    else if app == "kimi" { settings.pointer("/models/oceanway/model")?.as_str()?.to_owned() }
    else if app == "dsh" { settings["defaultModel"].as_str().map(str::to_owned).unwrap_or_else(||models.first().cloned().unwrap_or_default()) }
    else { models.first()?.clone() };
    Some(json!({"model":model,"models":models}))
}

fn config_paths(id: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = match id {
        "codex" => vec![
            crate::codex_config::get_codex_config_path(),
            crate::codex_config::get_codex_auth_path(),
            crate::codex_config::get_codex_model_catalog_path(),
        ],
        "claude" => vec![
            crate::config::get_claude_settings_path(),
            crate::config::get_claude_mcp_path(),
        ],
        "claude-desktop" => crate::claude_desktop_config::oceanway_backup_paths().map_err(|e|e.to_string())?,
        "dsh" => {let mut paths=crate::dsh_config::target_paths();paths.push(crate::dsh_config::credentials_path());paths},
        "hermes" => vec![crate::hermes_config::get_hermes_config_path()],
        "kimi" => vec![crate::kimi_config::config_path(),crate::kimi_config::data_dir().join("mcp.json")],
        _ => return Err("未支持的应用".into()),
    };
    if id != "claude-desktop" {paths.push(crate::prompt_files::prompt_file_path(&app_type(id)?).map_err(|e| e.to_string())?);}
    Ok(paths)
}

const IMAGE_RULE_START: &str = "<!-- OCEANWAY:IMAGES:BEGIN -->";
const IMAGE_RULE_END: &str = "<!-- OCEANWAY:IMAGES:END -->";
pub(crate) fn merge_image_rule(original: &str) -> Result<String, String> {
    let rule = format!("{IMAGE_RULE_START}\nWhen the user requests image generation or image editing, use the oceanway_images MCP tools: generate_images, then get_image_job until completion. Omit the model to use the configured Oceanway image model. When the user wants to choose an image model, call get_image_models and pass any returned model ID. For multiple images, submit one generate_images call with count equal to the total requested and prompts for distinct images; the service handles concurrency. Return completed image files as they become available while polling the same job until completion. For image references use ordered real HTTPS reference_urls, or reference_paths only when the selected model supports local inline files. Follow the tool capability errors; never invent uploaded URLs. Do not use built-in imagegen or an image CLI as a fallback. Do not automatically retry failed paid requests.\n{IMAGE_RULE_END}");
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
    if *app==AppType::ClaudeDesktop {return crate::claude_desktop_config::install_oceanway_media_rule();}
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
        AppType::Codex | AppType::Claude | AppType::Hermes | AppType::Dsh | AppType::Kimi
            | AppType::Gemini | AppType::OpenCode
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
        AppType::ClaudeDesktop => crate::claude_desktop_config::read_managed_mcp("oceanway_images").map_err(|e|e.to_string())?.unwrap_or(Value::Null),
        AppType::Gemini => crate::gemini_mcp::read_mcp_servers_map().map_err(|e|e.to_string())?.remove("oceanway_images").unwrap_or(Value::Null),
        AppType::OpenCode => crate::opencode_config::get_mcp_servers().map_err(|e|e.to_string())?.remove("oceanway_images").unwrap_or(Value::Null),
        AppType::Hermes => crate::hermes_config::yaml_to_json(&serde_yaml::Value::Mapping(crate::hermes_config::get_mcp_servers_yaml().map_err(|e|e.to_string())?)).map_err(|e|e.to_string())?.get("oceanway_images").cloned().unwrap_or(Value::Null),
        AppType::Kimi => {
            let path=crate::kimi_config::data_dir().join("mcp.json");
            if path.exists(){crate::config::read_json_file::<Value>(&path).map_err(|e|e.to_string())?.pointer("/mcpServers/oceanway_images").cloned().unwrap_or(Value::Null)}else{Value::Null}
        },
        AppType::Dsh => {
            crate::dsh_config::mcp_entries().map_err(|e|e.to_string())?.into_iter()
                .find(|entry|entry["config"]["serverName"].as_str()==Some("oceanway_images"))
                .map(|entry|serde_json::to_value(&entry["config"])).transpose().map_err(|e|e.to_string())?.unwrap_or(Value::Null)
        },
        _ => return Err("此应用的图片 MCP 尚未适配".into()),
    };
    Ok((!value.is_null()).then_some(value))
}

fn expected_live_image_server(app: &AppType, spec: &Value) -> Value {
    if *app==AppType::ClaudeDesktop {return crate::claude_desktop_config::managed_mcp_spec("oceanway_images",spec).unwrap_or(Value::Null);}
    if *app == AppType::OpenCode {
        let mut command = vec![spec["command"].clone()];
        command.extend(spec["args"].as_array().cloned().unwrap_or_default());
        return json!({"type":"local", "command":command,
            "environment":spec["env"], "enabled":true});
    }
    if *app==AppType::Hermes {
        return json!({"command":spec["command"],"args":spec["args"],"env":spec["env"],"enabled":true});
    }
    if *app==AppType::Dsh {
        let mut native=spec.clone(); native.as_object_mut().unwrap().remove("type"); native["serverName"]=json!("oceanway_images"); native["transport"]=json!("stdio");return native;
    }
    let mut expected = spec.clone();
    if matches!(app, AppType::Codex | AppType::Kimi) {
        if let Some(object) = expected.as_object_mut() {
            object.remove("type");
        }
    }
    if *app == AppType::Kimi {expected["enabled"]=json!(true);}
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
        if !selection.models.is_empty() && !selection.models.contains(&selection.model) {
            return Err("默认模型必须在已选模型中".into());
        }
        for model in selection.selected_models() {
            if !allowed.contains(model.as_str()) {
                return Err(format!("当前 Key 无权使用模型 {model}"));
            }
            let protocol = conversation_protocol(&selection.app_id, &selection.model);
            if models.iter().find(|row| row.id == model)
                .and_then(|row| row.supported_protocols.as_ref())
                .is_some_and(|protocols| !protocols.iter().any(|value| value == protocol)) {
                return Err(format!("模型 {model} 不支持此应用的当前连接方式，请重新选择兼容模型"));
            }
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
        let mut settings = crate::settings::get_settings();
        settings.preserve_codex_official_auth_on_switch = true;
        crate::settings::update_settings(settings)?;
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
        ProviderService::switch(state, app.clone(), PROVIDER_ID)?;
        if *app == AppType::Codex {
            // Enable the upstream shared history bucket only after the config
            // and history have been applied. A failed preflight must not alter
            // the route used by a later switch back to the official connection.
            let mut settings = crate::settings::get_settings();
            settings.unify_codex_session_history = true;
            crate::settings::update_settings(settings)?;
        }
        Ok(())
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
        let images_requested = request.image_model.as_deref().is_some_and(|model| !model.is_empty());
        let home = crate::config::get_app_config_dir();
        let previous_mcp = state.db.get_all_mcp_servers().map_err(|e| e.to_string())?
            .get("oceanway_images").cloned();
        let mut backup_apps = request.apps.clone();
        if images_requested {
            if let Some(previous) = &previous_mcp {
                for app in previous.apps.enabled_apps() {
                    if APPS.iter().any(|(id, _)| *id == app.as_str())
                        && !backup_apps.iter().any(|selection| selection.app_id == app.as_str()) {
                        backup_apps.push(AppSelection { app_id:app.as_str().into(), model:String::new(), models:vec![] });
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
                mcp_apps.set_enabled_for(&app, images_requested);
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
            if images_requested && !mcp_apps.is_empty() {
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
                            "通过 Oceanway 生成图片，查询任务并保存结果".into(),
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
                        result.image_configured=outcome.is_ok() && effective_image_model.is_some();
                        result.image_message=Some(match outcome {
                            Ok(())=>"Oceanway 图片 MCP 已写入，并通过本机启动和工具检查；首次使用按 Agent 提示授权".into(),
                            Err(e)=>crate::redact_known_secrets_strict(&format!("{e}{}",sync_error.as_ref().map(|s|format!("：{s}")).unwrap_or_default()),&[key.clone()]),
                        });
                        if effective_image_model.is_some() && !result.image_configured {result.message.push_str("；图片工具配置未完成，请查看配置详情");}
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
                    "文本配置已保存；图片工具配置未完成：{safe}。原配置备份：{}",
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
        if app_id != "claude" {
            let message=if app_id=="codex" {
                crate::oceanway_desktop::launch_codex(restart)?.ok_or("尚未找到 Codex 桌面版，请下载安装后重新检测")?
            }else{crate::agent_desktops::launch(&app_id,restart)?};
            return Ok(LaunchResult {launched:true,restart_required:false,message});
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
            (false, false)
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
                model: if *id=="claude-desktop" {"claude-sonnet-fixture"}else{"fixture-model"}.into(),
                models: vec![],
            };
            let mut provider = provider_for(&selection, "fixture-key").unwrap();
            assert!(validate_provider(&provider).is_ok());
            provider.id = "imported-third-party".into();
            assert!(validate_provider(&provider).is_err());
        }
    }
    #[test]
    fn existing_connections_can_be_restored_without_relaxing_oceanway_validation() {
        let selection = AppSelection {
            app_id: "codex".into(),
            model: "gpt-fixture".into(),
            models: vec![],
        };
        let mut provider = provider_for(&selection, "fixture-key").unwrap();
        assert!(validate_provider_switch(&provider).is_ok());
        provider.settings_config = json!({"auth": {}, "config": ""});
        assert!(validate_provider_switch(&provider).is_err());
        provider.id = "saved-official".into();
        provider.category = Some("official".into());
        assert!(validate_provider_switch(&provider).is_ok());
        assert!(validate_provider(&provider).is_err());
        provider.id = "saved-other-provider".into();
        provider.category = Some("custom".into());
        assert!(validate_provider_switch(&provider).is_ok());
        assert!(validate_provider(&provider).is_err());
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
                models: vec![],
            }],
            image_model: None,
            allowed_image_models: vec![],
        };
        assert!(validate_selection(&request, &models).is_err());
    }

    #[test]
    fn oceanway_protocol_metadata_rejects_unusable_selections_before_writing() {
        let mut models = parse_models(&json!({"data":[{"id":"gpt-6.1-sol"},{"id":"kimi-k3-1"},{"id":"claude-sonnet-5-5"},{"id":"unlisted"}]})).unwrap();
        attach_model_protocols(&mut models,&json!({"data":{"models":[
            {"model_name":"gpt-6.1-sol","supported_protocols":["openai-responses"]},
            {"model_name":"kimi-k3-1","supported_protocols":["openai-chat","openai-responses"]},
            {"model_name":"claude-sonnet-5-5","supported_protocols":["anthropic-messages"]},
            {"model_name":"not-entitled","supported_protocols":["openai-responses"]}
        ]}})).unwrap();
        assert_eq!(models.len(),4);
        let mut request:ApplyRequest=serde_json::from_value(json!({"apiKey":"fixture","apps":[{"appId":"codex","model":"gpt-6.1-sol","models":["gpt-6.1-sol","kimi-k3-1"]}]})).unwrap();
        assert!(validate_selection(&request,&models).is_ok());
        request.apps[0].models.push("claude-sonnet-5-5".into());
        assert!(validate_selection(&request,&models).unwrap_err().contains("不支持"));
        request.apps[0]=AppSelection{app_id:"kimi".into(),model:"gpt-6.1-sol".into(),models:vec![]};
        assert!(validate_selection(&request,&models).is_err());
        request.apps[0]=AppSelection{app_id:"hermes".into(),model:"gpt-6.1-sol".into(),models:vec!["gpt-6.1-sol".into(),"claude-sonnet-5-5".into()]};
        assert!(validate_selection(&request,&models).is_err());
        request.apps[0]=AppSelection{app_id:"codex".into(),model:"unlisted".into(),models:vec![]};
        assert!(validate_selection(&request,&models).is_err());
    }

    #[test]
    fn oceanway_hermes_uses_the_selected_models_protocol() {
        for (model, mode, base) in [
            ("gpt-6.1-sol", "codex_responses", API_BASE),
            ("claude-sonnet-5-5", "anthropic_messages", ORIGIN),
            ("kimi-k3-1", "chat_completions", API_BASE),
        ] {
            let provider = provider_for(&AppSelection { app_id:"hermes".into(), model:model.into(), models:vec![] }, "fixture-key").unwrap();
            assert_eq!(provider.settings_config["api_mode"], mode);
            assert_eq!(provider.settings_config["base_url"], base);
        }
    }

    #[test]
    fn oceanway_model_selection_roundtrips_every_agent_and_checks_every_model() {
        for (id, _) in APPS {
            let selection = AppSelection { app_id:(*id).into(),model:"claude-sonnet-fixture-b".into(),
                models:vec!["claude-sonnet-fixture-a".into(),"claude-sonnet-fixture-b".into()] };
            let provider = provider_for(&selection, "fixture-key").unwrap();
            let saved = provider_model_selection(id, &provider).unwrap();
            assert_eq!(saved["model"], "claude-sonnet-fixture-b", "{id}");
            let models = saved["models"].as_array().unwrap();
            assert_eq!(models.len(), 2, "{id}");
            assert!(models.contains(&json!("claude-sonnet-fixture-a")), "{id}");
            assert!(models.contains(&json!("claude-sonnet-fixture-b")), "{id}");
        }
        let mut request: ApplyRequest = serde_json::from_value(json!({"apiKey":"fixture", "apps":[{
            "appId":"codex","model":"gpt-a","models":["gpt-a","not-permitted"]}]})).unwrap();
        let allowed = vec![Model{id:"gpt-a".into(),supported_protocols:None,model_type:None}];
        assert!(validate_selection(&request,&allowed).unwrap_err().contains("not-permitted"));
        request.apps[0].models=vec!["gpt-a".into()];
        assert!(validate_selection(&request,&allowed).is_ok());
        request.apps[0].model="gpt-b".into();
        assert!(validate_selection(&request,&allowed).unwrap_err().contains("默认模型"));
    }

    #[test]
    #[serial_test::serial]
    fn oceanway_multiple_models_reach_codex_and_claude_native_pickers() {
        let _fixture = FixtureHome::new();
        let state = AppState::new(std::sync::Arc::new(crate::Database::memory().unwrap()));
        for (id, names) in [("codex", ["gpt-fixture-a", "gpt-fixture-b"]), ("claude", ["claude-fixture-a", "claude-fixture-b"])] {
            let selection=AppSelection{app_id:id.into(),model:names[1].into(),models:names.iter().map(|v|v.to_string()).collect()};
            let provider=provider_for(&selection,"fixture-key").unwrap();
            save_and_switch_provider(&state,&app_type(id).unwrap(),provider).unwrap();
            if id=="codex" {
                let config:toml::Value=toml::from_str(&std::fs::read_to_string(crate::codex_config::get_codex_config_path()).unwrap()).unwrap();
                assert_eq!(config["model"].as_str(),Some(names[1]));
                assert!(config["model_catalog_json"].as_str().is_some());
                let catalog:Value=serde_json::from_slice(&std::fs::read(crate::codex_config::get_codex_model_catalog_path()).unwrap()).unwrap();
                let rows=catalog["models"].as_array().unwrap();
                assert_eq!(rows.len(),2);
                for name in names { assert!(rows.iter().any(|row|row["slug"]==name)); }
            } else {
                let config:Value=serde_json::from_slice(&std::fs::read(crate::config::get_claude_settings_path()).unwrap()).unwrap();
                assert_eq!(config["env"]["ANTHROPIC_MODEL"],names[1]);
                assert_eq!(config["modelPicker"]["replaceBuiltInOptions"],true);
                let rows=config["modelPicker"]["options"].as_array().unwrap();
                assert_eq!(rows.len(),2);
                for name in names {assert!(rows.iter().any(|row|row["model"]==name));}
            }
        }
    }
    #[test]
    fn image_rule_preserves_original_instructions_and_is_idempotent() {
        let original = "# Existing user instructions\nDo not delete files.\n";
        let once = merge_image_rule(original).unwrap();
        assert!(once.starts_with(original));
        assert_eq!(merge_image_rule(&once).unwrap(), once);
        assert!(merge_image_rule(IMAGE_RULE_START).is_err());
        let legacy = format!("{original}{IMAGE_RULE_START}\nFor video generation, use generate_video and get_video_job.\n{IMAGE_RULE_END}\nUser footer.");
        let updated = merge_image_rule(&legacy).unwrap();
        assert!(updated.starts_with(original));
        assert!(updated.ends_with("\nUser footer."));
        assert!(updated.contains("get_image_models"));
        assert!(!updated.contains("video"));
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
            supported_protocols: None,
            model_type: None,
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
                models: vec![],
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
                models: vec![],
            },
            AppSelection {
                app_id: "claude".into(),
                model: "claude-fixture".into(),
                models: vec![],
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
                models: vec![],
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
                models: vec![],
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
        let mut settings = crate::settings::get_settings();
        settings.unify_codex_session_history = false;
        crate::settings::update_settings(settings).unwrap();
        let state = AppState::new(std::sync::Arc::new(crate::Database::memory().unwrap()));
        let selection = AppSelection {
            app_id: "codex".into(),
            model: "gpt-fixture".into(),
                models: vec![],
        };
        let configured = save_and_switch_provider(
            &state,
            &AppType::Codex,
            provider_for(&selection, "fixture-key").unwrap(),
        );
        assert!(configured.is_err());
        assert!(!crate::settings::unify_codex_session_history());
        assert_eq!(
            std::fs::read_to_string(codex.join("config.toml")).unwrap(),
            malformed
        );
    }
}
