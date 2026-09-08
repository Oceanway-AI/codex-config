#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Local;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{LogicalSize, Manager, Size, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use toml_edit::{value, DocumentMut, Item, Table, Value as TomlValue};

const PROVIDER_ID: &str = "OceanWay";
const DEFAULT_BASE_URL: &str = "https://ocean-way.top";
const MODEL_FALLBACK: &str = "gpt-5.4";
const CODEX_AUTH_KEY: &str = "OPENAI_API_KEY";
const OPENAI_BASE_URL_ENV_KEY: &str = "OPENAI_BASE_URL";
const IMAGE_EXTENSION_ACTOR_AUTHORIZATION: &str = "local-image-extension";
const BACKUP_DIR_NAME: &str = "oceanway-ai-backup";
const HISTORY_MIGRATION_BACKUP_DIR_NAME: &str = "oceanway-history-migration-backup";
const CODEX_STATE_DB_NAME: &str = "state_5.sqlite";
const MINIMUM_IMAGE_EXTENSION_VERSION: &str = "0.143.0";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationResult {
    config_path: String,
    auth_path: String,
    config_backup_path: Option<String>,
    auth_backup_path: Option<String>,
    auth_strategy: String,
    imagegen_cli_configured: bool,
    history_migration_restore: Option<HistoryMigrationRestoreResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImagegenCliConfigResult {
    config_path: String,
    base_url: String,
    configured: bool,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMigrationRestoreResult {
    restored_backups: usize,
    restored_session_files: usize,
    sqlite_rows_restored: usize,
    warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMigrationStatus {
    target_provider: String,
    migration_supported: bool,
    needs_migration: bool,
    rollout_files_to_update: usize,
    sqlite_rows_to_update: usize,
    encrypted_content_files: usize,
    provider_counts: Vec<ProviderCount>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMigrationResult {
    target_provider: String,
    changed_session_files: usize,
    sqlite_rows_updated: usize,
    skipped_files: usize,
    encrypted_content_files: usize,
    backup_path: Option<String>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderCount {
    provider: String,
    files: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMigrationManifest {
    version: u8,
    target_provider: String,
    created_at: String,
    files: Vec<HistoryMigrationManifestFile>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMigrationManifestFile {
    path: String,
    thread_id: Option<String>,
    original_provider: Option<String>,
    migrated_provider: String,
    original_first_line: String,
}

#[derive(Clone)]
struct HistoryMigrationChange {
    path: PathBuf,
    thread_id: Option<String>,
    original_provider: Option<String>,
    original_first_line: String,
    next_first_line: String,
    encrypted_content: bool,
}

#[derive(Default)]
struct CollectedHistoryMigration {
    changes: Vec<HistoryMigrationChange>,
    provider_counts: HashMap<String, usize>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigStatus {
    configured: bool,
    provider_id: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    has_api_key: bool,
    auth_strategy: String,
    chatgpt_login_detected: bool,
    chatgpt_account_label: Option<String>,
    imagegen_cli_configured: bool,
    config_path: String,
    auth_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionTestResult {
    ok: bool,
    message: String,
    endpoint: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemInfo {
    operating_system: String,
    operating_system_version: String,
    architecture: String,
    codex_cli_version: Option<String>,
    codex_desktop_version: Option<String>,
    codex_host: Option<String>,
    codex_running: bool,
    app_version: String,
    backup_created_at: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticCheck {
    id: String,
    label: String,
    status: String,
    detail: String,
    repairable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticReport {
    generated_at: String,
    overall_status: String,
    passed: usize,
    warnings: usize,
    errors: usize,
    checks: Vec<DiagnosticCheck>,
    system: SystemInfo,
    redacted_report: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelPermission {
    model: String,
    status: String,
    detail: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountAccessStatus {
    integration_available: bool,
    status: String,
    message: String,
    balance: Option<f64>,
    balance_unit: Option<String>,
    plan_name: Option<String>,
    last_checked_at: Option<String>,
    model_permissions: Vec<ModelPermission>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RestartCodexResult {
    restarted: bool,
    was_running: bool,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCheckResult {
    available: bool,
    current_version: String,
    latest_version: Option<String>,
    notes: Option<String>,
    published_at: Option<String>,
}

struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Deserialize, Serialize)]
struct RestoreSnapshotMeta {
    config_existed: bool,
    auth_existed: bool,
    created_at: String,
}

#[derive(Default)]
struct CliOptions {
    dry_run: bool,
    provider_id: String,
    base_url: String,
    model: Option<String>,
    api_key: Option<String>,
}

#[tauri::command]
fn get_config_status() -> Result<ConfigStatus, String> {
    let codex_home = codex_home()?;
    let config_path = codex_home.join("config.toml");
    let auth_path = codex_home.join("auth.json");
    let config = fs::read_to_string(&config_path).unwrap_or_default();
    let provider_id = read_root_string(&config, "model_provider");
    let base_url = read_provider_base_url(&config, PROVIDER_ID);
    let model = read_current_model_from_content(&config);
    let provider_token = read_provider_bearer_token(&config, PROVIDER_ID);
    let auth_api_key = read_auth_api_key(&auth_path);
    let has_api_key = provider_token
        .as_ref()
        .or(auth_api_key.as_ref())
        .is_some_and(|value| !value.trim().is_empty());
    let chatgpt_login_detected = read_auth_has_chatgpt_login(&auth_path);
    let chatgpt_account_label = read_auth_chatgpt_account_label(&auth_path);
    let auth_strategy = if provider_token.is_some() {
        ProviderAuthStrategy::ChatGptBearerToken
    } else {
        ProviderAuthStrategy::ApiKey
    };
    let oceanway_active = provider_id.as_deref() == Some(PROVIDER_ID);
    let imagegen_cli_configured = oceanway_active
        && has_matching_imagegen_cli_environment(
            &config,
            provider_token.as_deref().or(auth_api_key.as_deref()),
            base_url.as_deref(),
        );
    let configured = oceanway_active
        && base_url
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());

    Ok(ConfigStatus {
        configured,
        provider_id,
        base_url,
        model,
        has_api_key,
        auth_strategy: auth_strategy.as_str().to_string(),
        chatgpt_login_detected,
        chatgpt_account_label,
        imagegen_cli_configured,
        config_path: display_path(&config_path),
        auth_path: display_path(&auth_path),
    })
}

#[tauri::command]
async fn test_connection(
    api_key: String,
    base_url: String,
) -> Result<ConnectionTestResult, String> {
    tauri::async_runtime::spawn_blocking(move || test_connection_command(api_key, base_url))
        .await
        .map_err(|_| "连接测试任务异常".to_string())?
}

fn test_connection_command(
    api_key: String,
    base_url: String,
) -> Result<ConnectionTestResult, String> {
    let api_key = if api_key.trim().is_empty() {
        let codex_home = codex_home()?;
        resolve_api_key_in_home(&codex_home, "")?
    } else {
        api_key.trim().to_string()
    };
    let base_url = base_url.trim();

    let base_url = if base_url.is_empty() {
        DEFAULT_BASE_URL
    } else {
        base_url
    };
    Ok(test_connection_internal(&api_key, base_url))
}

fn test_connection_internal(api_key: &str, base_url: &str) -> ConnectionTestResult {
    if let Err(message) = validate_base_url(base_url) {
        return ConnectionTestResult {
            ok: false,
            message,
            endpoint: "无效地址".into(),
        };
    }
    let endpoints = model_endpoints(base_url);
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return ConnectionTestResult {
                ok: false,
                message: format!("无法创建测试客户端：{err}"),
                endpoint: base_url.to_string(),
            };
        }
    };

    let mut last_error = String::new();
    for endpoint in endpoints {
        match client.get(&endpoint).bearer_auth(api_key).send() {
            Ok(response) if response.status().is_success() => {
                return ConnectionTestResult {
                    ok: true,
                    message: "连接成功，API Key 和 Base URL 可用。".to_string(),
                    endpoint,
                };
            }
            Ok(response) => {
                let status = response.status();
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return ConnectionTestResult {
                        ok: false,
                        message: format!("连接到服务，但 API Key 无效或无权限。HTTP {status}"),
                        endpoint,
                    };
                }
                last_error = format!("HTTP {status}");
            }
            Err(err) => {
                last_error = err.to_string();
            }
        }
    }

    ConnectionTestResult {
        ok: false,
        message: format!("连接失败：{last_error}"),
        endpoint: base_url.to_string(),
    }
}

#[tauri::command]
async fn get_system_info() -> Result<SystemInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let codex_home = codex_home()?;
        Ok(collect_system_info(&codex_home))
    })
    .await
    .map_err(|_| "系统信息读取任务异常".to_string())?
}

#[tauri::command]
fn get_account_access_status() -> AccountAccessStatus {
    AccountAccessStatus {
        integration_available: false,
        status: "reserved".to_string(),
        message: "额度与模型权限接口已预留，接入 OceanWay 账户接口后即可展示实时数据。".to_string(),
        balance: None,
        balance_unit: None,
        plan_name: None,
        last_checked_at: None,
        model_permissions: vec![
            ModelPermission {
                model: "gpt-5.4".to_string(),
                status: "pending".to_string(),
                detail: "等待账户权限接口".to_string(),
            },
            ModelPermission {
                model: "gpt-image-2".to_string(),
                status: "pending".to_string(),
                detail: "等待账户权限接口".to_string(),
            },
        ],
    }
}

#[tauri::command]
async fn run_diagnostics() -> Result<DiagnosticReport, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let codex_home = codex_home()?;
        run_diagnostics_in_home(&codex_home)
    })
    .await
    .map_err(|_| "诊断任务异常".to_string())?
}

#[tauri::command]
async fn copy_support_report() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let codex_home = codex_home()?;
        let report = run_diagnostics_in_home(&codex_home)?;
        copy_text_to_clipboard(&report.redacted_report)?;
        Ok("脱敏诊断报告已复制到剪贴板。".to_string())
    })
    .await
    .map_err(|_| "诊断报告复制任务异常".to_string())?
}

#[tauri::command]
async fn repair_configuration() -> Result<OperationResult, String> {
    tauri::async_runtime::spawn_blocking(repair_configuration_internal)
        .await
        .map_err(|_| "配置修复任务异常".to_string())?
}

fn repair_configuration_internal() -> Result<OperationResult, String> {
    let codex_home = codex_home()?;
    let config_path = codex_home.join("config.toml");
    let config = fs::read_to_string(&config_path).unwrap_or_default();
    let base_url = read_provider_base_url(&config, PROVIDER_ID)
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    configure_provider_internal(String::new(), base_url)
}

#[tauri::command]
async fn restart_codex() -> Result<RestartCodexResult, String> {
    tauri::async_runtime::spawn_blocking(restart_codex_desktop)
        .await
        .map_err(|_| "重启执行任务异常".to_string())?
}

#[tauri::command]
async fn check_for_updates(
    app: tauri::AppHandle,
    pending_update: State<'_, PendingUpdate>,
) -> Result<UpdateCheckResult, String> {
    let current_version = app.package_info().version.to_string();
    let update = app
        .updater()
        .map_err(|err| format!("无法初始化更新服务：{err}"))?
        .check()
        .await
        .map_err(|err| match err {
            tauri_plugin_updater::Error::ReleaseNotFound => {
                "更新清单不可用或格式无效，请联系维护者检查发布通道。".to_string()
            }
            _ => format!("检查更新失败：{err}"),
        })?;

    let result = if let Some(update) = update.as_ref() {
        UpdateCheckResult {
            available: true,
            current_version,
            latest_version: Some(update.version.clone()),
            notes: update.body.clone(),
            published_at: update.date.map(|date| date.to_string()),
        }
    } else {
        UpdateCheckResult {
            available: false,
            current_version,
            latest_version: None,
            notes: None,
            published_at: None,
        }
    };

    *pending_update
        .0
        .lock()
        .map_err(|_| "无法保存待安装更新状态。".to_string())? = update;
    Ok(result)
}

#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    pending_update: State<'_, PendingUpdate>,
) -> Result<(), String> {
    let update = pending_update
        .0
        .lock()
        .map_err(|_| "无法读取待安装更新状态。".to_string())?
        .take()
        .ok_or_else(|| "没有待安装的更新，请先检查更新。".to_string())?;

    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|err| format!("下载或安装更新失败：{err}"))?;
    app.restart();
}

#[tauri::command]
fn open_config_dir() -> Result<(), String> {
    let codex_home = codex_home()?;
    fs::create_dir_all(&codex_home).map_err(|err| format!("无法创建 Codex 目录：{err}"))?;
    open_path(&codex_home)
}

#[tauri::command]
async fn get_history_migration_status() -> Result<HistoryMigrationStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let codex_home = codex_home()?;
        history_migration_status_in_home(&codex_home)
    })
    .await
    .map_err(|_| "历史记录扫描任务异常".to_string())?
}

#[tauri::command]
async fn migrate_history_visibility() -> Result<HistoryMigrationResult, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let codex_home = codex_home()?;
        migrate_history_visibility_in_home(&codex_home)
    })
    .await
    .map_err(|_| "历史迁移任务异常".to_string())?
}

#[tauri::command]
async fn configure_provider(api_key: String, base_url: String) -> Result<OperationResult, String> {
    tauri::async_runtime::spawn_blocking(move || configure_provider_internal(api_key, base_url))
        .await
        .map_err(|_| "配置执行任务异常".to_string())?
}

#[tauri::command]
async fn configure_imagegen_cli() -> Result<ImagegenCliConfigResult, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let codex_home = codex_home()?;
        configure_imagegen_cli_in_home(&codex_home)
    })
    .await
    .map_err(|_| "图片配置同步任务异常".to_string())?
}

fn configure_imagegen_cli_in_home(codex_home: &Path) -> Result<ImagegenCliConfigResult, String> {
    let config_path = codex_home.join("config.toml");
    let auth_path = codex_home.join("auth.json");
    let config = fs::read_to_string(&config_path).unwrap_or_default();
    if read_root_string(&config, "model_provider").as_deref() != Some(PROVIDER_ID) {
        return Err("请先使用“一键配置”将当前 provider 切换到 OceanWay。".to_string());
    }
    let base_url = read_provider_base_url(&config, PROVIDER_ID)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "请先使用“一键配置”完成 OceanWay provider 配置。".to_string())?;
    let api_key = read_provider_bearer_token(&config, PROVIDER_ID)
        .or_else(|| read_auth_api_key(&auth_path))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "未找到已保存的 OceanWay API Key，请重新输入并执行“一键配置”。".to_string()
        })?;

    write_imagegen_cli_environment(&config_path, &api_key, &base_url)?;

    Ok(ImagegenCliConfigResult {
        config_path: display_path(&config_path),
        base_url,
        configured: true,
    })
}

fn configure_provider_internal(
    api_key: String,
    base_url: String,
) -> Result<OperationResult, String> {
    let base_url = base_url.trim();

    let base_url = if base_url.is_empty() {
        DEFAULT_BASE_URL
    } else {
        base_url
    };

    validate_base_url(base_url)?;

    let codex_home = codex_home()?;
    let api_key = resolve_api_key_in_home(&codex_home, &api_key)?;
    let config_path = codex_home.join("config.toml");
    let auth_path = codex_home.join("auth.json");
    fs::create_dir_all(&codex_home).map_err(|err| format!("无法创建 Codex 目录：{err}"))?;
    ensure_restore_snapshot(&codex_home, &config_path, &auth_path)?;
    let auth_strategy = choose_provider_auth_strategy(&auth_path);

    let old_auth = if auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|err| format!("无法读取旧 auth.json：{err}"))?)
    } else {
        None
    };

    let auth_backup_path = write_auth_json(&auth_path, &api_key, auth_strategy)?;
    let model = read_current_model(&config_path).unwrap_or_else(|| MODEL_FALLBACK.to_string());
    let provider_token = if auth_strategy == ProviderAuthStrategy::ChatGptBearerToken {
        Some(api_key.as_str())
    } else {
        None
    };
    let config_result = write_config_toml(
        &config_path,
        PROVIDER_ID,
        base_url,
        &model,
        provider_token,
        auth_strategy,
        Some(&api_key),
    );

    let config_backup_path = match config_result {
        Ok(path) => path,
        Err(err) => {
            rollback_auth(&auth_path, old_auth);
            return Err(err);
        }
    };

    Ok(OperationResult {
        config_path: display_path(&config_path),
        auth_path: display_path(&auth_path),
        config_backup_path: config_backup_path.as_ref().map(|path| display_path(path)),
        auth_backup_path: auth_backup_path.as_ref().map(|path| display_path(path)),
        auth_strategy: auth_strategy.as_str().to_string(),
        imagegen_cli_configured: true,
        history_migration_restore: None,
    })
}

fn resolve_api_key_in_home(codex_home: &Path, candidate: &str) -> Result<String, String> {
    let candidate = candidate.trim();
    if !candidate.is_empty() {
        return Ok(candidate.to_string());
    }

    let config_path = codex_home.join("config.toml");
    let auth_path = codex_home.join("auth.json");
    let config = fs::read_to_string(config_path).unwrap_or_default();
    read_provider_bearer_token(&config, PROVIDER_ID)
        .or_else(|| read_auth_api_key(&auth_path))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "尚未保存 API Key，请先输入 OceanWay API Key。".to_string())
}

#[tauri::command]
async fn restore_defaults() -> Result<OperationResult, String> {
    tauri::async_runtime::spawn_blocking(restore_defaults_internal)
        .await
        .map_err(|_| "配置恢复任务异常".to_string())?
}

fn restore_defaults_internal() -> Result<OperationResult, String> {
    let codex_home = codex_home()?;
    let config_path = codex_home.join("config.toml");
    let auth_path = codex_home.join("auth.json");
    fs::create_dir_all(&codex_home).map_err(|err| format!("无法创建 Codex 目录：{err}"))?;

    let config_backup_path = backup_file(&config_path)?;
    let auth_backup_path = backup_file(&auth_path)?;

    if restore_from_snapshot(&codex_home, &config_path, &auth_path)? {
        if config_path.exists() {
            set_private_permissions(&config_path)?;
        }
        if auth_path.exists() {
            set_private_permissions(&auth_path)?;
        }
    } else {
        remove_provider_from_config(&config_path, PROVIDER_ID)?;
        remove_imagegen_cli_environment_from_file(&config_path)?;
        remove_api_key_from_auth(&auth_path)?;
        set_private_permissions(&config_path)?;
        set_private_permissions(&auth_path)?;
    }
    let history_migration_restore = restore_history_migrations_lossy(&codex_home);

    Ok(OperationResult {
        config_path: display_path(&config_path),
        auth_path: display_path(&auth_path),
        config_backup_path: config_backup_path.as_ref().map(|path| display_path(path)),
        auth_backup_path: auth_backup_path.as_ref().map(|path| display_path(path)),
        auth_strategy: "restore".to_string(),
        imagegen_cli_configured: false,
        history_migration_restore: Some(history_migration_restore),
    })
}

#[tauri::command]
fn exit_app(app_handle: tauri::AppHandle) {
    app_handle.exit(0);
}

#[tauri::command]
fn resize_window_to_content(app_handle: tauri::AppHandle, height: f64) -> Result<(), String> {
    let window = app_handle
        .get_webview_window("main")
        .ok_or_else(|| "无法找到主窗口".to_string())?;
    let scale_factor = window
        .scale_factor()
        .map_err(|err| format!("无法读取窗口缩放比例：{err}"))?;
    let size = window
        .inner_size()
        .map_err(|err| format!("无法读取窗口尺寸：{err}"))?;
    let width = (size.width as f64 / scale_factor).clamp(720.0, 980.0);
    let height = height.clamp(500.0, 760.0);
    window
        .set_size(Size::Logical(LogicalSize { width, height }))
        .map_err(|err| format!("无法调整窗口尺寸：{err}"))
}

fn write_config_toml(
    config_path: &Path,
    provider_id: &str,
    base_url: &str,
    model: &str,
    bearer_token: Option<&str>,
    auth_strategy: ProviderAuthStrategy,
    imagegen_api_key: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    validate_base_url(base_url)?;
    let codex_home = config_path
        .parent()
        .ok_or_else(|| format!("无法定位 Codex 目录：{}", config_path.display()))?;
    let auth_path = codex_home.join("auth.json");
    ensure_restore_snapshot(codex_home, config_path, &auth_path)?;

    let backup_path = backup_file(config_path)?;
    let original = read_config_for_write(config_path)?;
    let mut rendered = merge_config(
        &original,
        provider_id,
        base_url,
        model,
        bearer_token,
        auth_strategy,
    );
    if let Some(api_key) = imagegen_api_key.filter(|value| !value.trim().is_empty()) {
        rendered = merge_imagegen_cli_environment(&rendered, api_key, base_url)?;
    }

    write_private_atomic(config_path, rendered.as_bytes())
        .map_err(|err| format!("无法写入 config.toml：{err}"))?;
    Ok(backup_path)
}

fn write_imagegen_cli_environment(
    config_path: &Path,
    api_key: &str,
    base_url: &str,
) -> Result<Option<PathBuf>, String> {
    validate_base_url(base_url)?;
    let codex_home = config_path
        .parent()
        .ok_or_else(|| format!("无法定位 Codex 目录：{}", config_path.display()))?;
    let auth_path = codex_home.join("auth.json");
    ensure_restore_snapshot(codex_home, config_path, &auth_path)?;

    let backup_path = backup_file(config_path)?;
    let original = read_config_for_write(config_path)?;
    let rendered = merge_imagegen_cli_environment(&original, api_key, base_url)?;
    write_private_atomic(config_path, rendered.as_bytes())
        .map_err(|err| format!("无法写入 config.toml：{err}"))?;
    Ok(backup_path)
}

fn write_auth_json(
    auth_path: &Path,
    api_key: &str,
    strategy: ProviderAuthStrategy,
) -> Result<Option<PathBuf>, String> {
    let content = match fs::read_to_string(auth_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(_) => return Err("无法读取 auth.json，已停止写入，请检查权限。".into()),
    };
    let rendered = render_auth_json_content(&content, api_key, strategy)?;
    let backup_path = backup_file(auth_path)?;

    write_private_atomic(auth_path, rendered.as_bytes())
        .map_err(|err| format!("无法写入 auth.json：{err}"))?;
    Ok(backup_path)
}

fn read_config_for_write(path: &Path) -> Result<String, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => return Err("无法读取 config.toml，已停止写入，请检查编码与权限。".into()),
    };
    content
        .parse::<DocumentMut>()
        .map_err(|_| "config.toml 格式损坏，已停止写入。".to_string())?;
    Ok(content)
}

fn render_auth_json_content(
    content: &str,
    api_key: &str,
    strategy: ProviderAuthStrategy,
) -> Result<String, String> {
    let mut value = serde_json::from_str::<serde_json::Value>(content)
        .map_err(|_| "auth.json 格式损坏，已停止写入，请先修复或恢复认证文件。".to_string())?;
    if !value.is_object() {
        return Err("auth.json 必须是 JSON 对象，已停止写入。".into());
    }

    if let Some(object) = value.as_object_mut() {
        match strategy {
            ProviderAuthStrategy::ApiKey => {
                object.insert(CODEX_AUTH_KEY.to_string(), json!(api_key));
            }
            ProviderAuthStrategy::ChatGptBearerToken => {
                object.insert("auth_mode".to_string(), json!("chatgpt"));
                object.insert(CODEX_AUTH_KEY.to_string(), Value::Null);
            }
        }
    } else {
        let mut object = serde_json::Map::new();
        match strategy {
            ProviderAuthStrategy::ApiKey => {
                object.insert(CODEX_AUTH_KEY.to_string(), json!(api_key));
            }
            ProviderAuthStrategy::ChatGptBearerToken => {
                object.insert("auth_mode".to_string(), json!("chatgpt"));
                object.insert(CODEX_AUTH_KEY.to_string(), Value::Null);
            }
        }
        value = Value::Object(object);
    }

    let rendered = serde_json::to_string_pretty(&value)
        .map_err(|err| format!("无法生成 auth.json：{err}"))?
        + "\n";
    Ok(rendered)
}

fn remove_provider_from_config(config_path: &Path, provider_id: &str) -> Result<(), String> {
    let original = fs::read_to_string(config_path).unwrap_or_default();
    let rendered = remove_provider_config(&original, provider_id);
    fs::write(config_path, rendered).map_err(|err| format!("无法写入 config.toml：{err}"))
}

fn remove_api_key_from_auth(auth_path: &Path) -> Result<(), String> {
    let content = fs::read_to_string(auth_path).unwrap_or_else(|_| "{}".to_string());
    let mut value =
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| json!({}));

    if let Some(object) = value.as_object_mut() {
        object.remove(CODEX_AUTH_KEY);
    } else {
        value = json!({});
    }

    let rendered = serde_json::to_string_pretty(&value)
        .map_err(|err| format!("无法生成 auth.json：{err}"))?
        + "\n";
    fs::write(auth_path, rendered).map_err(|err| format!("无法写入 auth.json：{err}"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProviderAuthStrategy {
    ApiKey,
    ChatGptBearerToken,
}

impl ProviderAuthStrategy {
    fn as_str(self) -> &'static str {
        match self {
            ProviderAuthStrategy::ApiKey => "apiKey",
            ProviderAuthStrategy::ChatGptBearerToken => "chatgptBearerToken",
        }
    }
}

fn read_auth_api_key(auth_path: &Path) -> Option<String> {
    let content = fs::read_to_string(auth_path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    value
        .get(CODEX_AUTH_KEY)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn ensure_restore_snapshot(
    codex_home: &Path,
    config_path: &Path,
    auth_path: &Path,
) -> Result<(), String> {
    let snapshot_dir = codex_home.join(BACKUP_DIR_NAME);
    let meta_path = snapshot_dir.join("meta.json");
    if meta_path.exists() {
        secure_snapshot_permissions(&snapshot_dir)?;
        return Ok(());
    }

    fs::create_dir_all(&snapshot_dir)
        .map_err(|err| format!("无法创建 OceanWay 备份目录：{err}"))?;
    secure_snapshot_permissions(&snapshot_dir)?;

    let meta = RestoreSnapshotMeta {
        config_existed: config_path.exists(),
        auth_existed: auth_path.exists(),
        created_at: Local::now().to_rfc3339(),
    };

    if meta.config_existed {
        copy_private_new(config_path, &snapshot_dir.join("config.toml"))
            .map_err(|err| format!("无法保存 config.toml 初始快照：{err}"))?;
    }
    if meta.auth_existed {
        copy_private_new(auth_path, &snapshot_dir.join("auth.json"))
            .map_err(|err| format!("无法保存 auth.json 初始快照：{err}"))?;
    }

    let rendered = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("无法生成 OceanWay 备份元数据：{err}"))?
        + "\n";
    fs::write(meta_path, rendered).map_err(|err| format!("无法写入 OceanWay 备份元数据：{err}"))
}

fn restore_from_snapshot(
    codex_home: &Path,
    config_path: &Path,
    auth_path: &Path,
) -> Result<bool, String> {
    let snapshot_dir = codex_home.join(BACKUP_DIR_NAME);
    let meta_path = snapshot_dir.join("meta.json");
    if !meta_path.exists() {
        return Ok(false);
    }

    let meta_content = fs::read_to_string(&meta_path)
        .map_err(|err| format!("无法读取 OceanWay 备份元数据：{err}"))?;
    let meta = serde_json::from_str::<RestoreSnapshotMeta>(&meta_content)
        .map_err(|err| format!("OceanWay 备份元数据无效：{err}"))?;

    if meta.config_existed {
        fs::copy(snapshot_dir.join("config.toml"), config_path)
            .map_err(|err| format!("无法恢复 config.toml 初始快照：{err}"))?;
    } else if config_path.exists() {
        fs::remove_file(config_path).map_err(|err| format!("无法删除新建的 config.toml：{err}"))?;
    }

    if meta.auth_existed {
        fs::copy(snapshot_dir.join("auth.json"), auth_path)
            .map_err(|err| format!("无法恢复 auth.json 初始快照：{err}"))?;
    } else if auth_path.exists() {
        fs::remove_file(auth_path).map_err(|err| format!("无法删除新建的 auth.json：{err}"))?;
    }

    fs::remove_dir_all(snapshot_dir).map_err(|err| format!("无法删除 OceanWay 备份目录：{err}"))?;
    Ok(true)
}

fn history_migration_status_in_home(codex_home: &Path) -> Result<HistoryMigrationStatus, String> {
    let target_provider = read_session_provider_from_config(&codex_home.join("config.toml"));
    let collected = collect_history_migration(codex_home, &target_provider)?;
    let migration_supported = target_provider == PROVIDER_ID;
    let sqlite_rows_to_update =
        count_history_migration_sqlite_rows(codex_home, &target_provider, &collected.changes)?;
    let mut provider_counts = collected
        .provider_counts
        .into_iter()
        .map(|(provider, files)| ProviderCount { provider, files })
        .collect::<Vec<_>>();
    provider_counts.sort_by(|left, right| left.provider.cmp(&right.provider));
    let encrypted_content_files = collected
        .changes
        .iter()
        .filter(|change| change.encrypted_content)
        .count();
    let rollout_files_to_update = collected.changes.len();

    Ok(HistoryMigrationStatus {
        target_provider,
        migration_supported,
        needs_migration: migration_supported
            && (rollout_files_to_update > 0 || sqlite_rows_to_update > 0),
        rollout_files_to_update: if migration_supported {
            rollout_files_to_update
        } else {
            0
        },
        sqlite_rows_to_update: if migration_supported {
            sqlite_rows_to_update
        } else {
            0
        },
        encrypted_content_files,
        provider_counts,
        warnings: collected.warnings,
    })
}

fn migrate_history_visibility_in_home(codex_home: &Path) -> Result<HistoryMigrationResult, String> {
    let target_provider = read_session_provider_from_config(&codex_home.join("config.toml"));
    if target_provider != PROVIDER_ID {
        return Err(format!(
            "历史迁移仅支持配置到 {PROVIDER_ID} 后使用。恢复默认时会自动撤销已记录的历史迁移。"
        ));
    }
    let mut collected = collect_history_migration(codex_home, &target_provider)?;
    let encrypted_content_files = collected
        .changes
        .iter()
        .filter(|change| change.encrypted_content)
        .count();
    if collected.changes.is_empty() {
        let sqlite_rows_updated =
            update_history_migration_sqlite(codex_home, &target_provider, &[])?;
        return Ok(HistoryMigrationResult {
            target_provider,
            changed_session_files: 0,
            sqlite_rows_updated,
            skipped_files: 0,
            encrypted_content_files,
            backup_path: None,
            warnings: collected.warnings,
        });
    }

    let backup_dir = create_history_migration_backup(codex_home, &target_provider)?;
    let mut applied = Vec::new();
    let mut skipped_files = 0;
    for change in &collected.changes {
        match apply_history_migration_change(change) {
            Ok(true) => applied.push(change.clone()),
            Ok(false) => skipped_files += 1,
            Err(err) => {
                let _ = restore_applied_history_migration_changes(&applied);
                return Err(err);
            }
        }
    }

    let sqlite_rows_updated =
        match update_history_migration_sqlite(codex_home, &target_provider, &applied) {
            Ok(updated) => updated,
            Err(err) => {
                let _ = restore_applied_history_migration_changes(&applied);
                return Err(err);
            }
        };

    write_history_migration_manifest(&backup_dir, &target_provider, &applied)?;
    collected.warnings.extend(history_migration_warning_text(
        encrypted_content_files,
        &target_provider,
    ));

    Ok(HistoryMigrationResult {
        target_provider,
        changed_session_files: applied.len(),
        sqlite_rows_updated,
        skipped_files,
        encrypted_content_files,
        backup_path: Some(display_path(&backup_dir)),
        warnings: collected.warnings,
    })
}

fn collect_history_migration(
    codex_home: &Path,
    target_provider: &str,
) -> Result<CollectedHistoryMigration, String> {
    let mut collected = CollectedHistoryMigration::default();
    for path in history_rollout_files(codex_home)? {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                collected.warnings.push(format!(
                    "跳过 {}：无法读取会话文件：{err}",
                    display_path(&path)
                ));
                continue;
            }
        };
        let (first_line, _) = split_first_line(&text);
        if first_line.trim().is_empty() {
            continue;
        }
        let mut value = match serde_json::from_str::<Value>(&first_line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) else {
            continue;
        };
        let original_provider = payload
            .get("model_provider")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let provider_key = original_provider
            .clone()
            .unwrap_or_else(|| "(missing)".to_string());
        *collected.provider_counts.entry(provider_key).or_insert(0) += 1;

        if original_provider.as_deref() == Some(target_provider) {
            continue;
        }

        let thread_id = payload
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string);
        payload.insert("model_provider".to_string(), json!(target_provider));
        let next_first_line =
            serde_json::to_string(&value).map_err(|err| format!("无法生成会话 metadata：{err}"))?;
        collected.changes.push(HistoryMigrationChange {
            path,
            thread_id,
            original_provider,
            original_first_line: first_line,
            next_first_line,
            encrypted_content: text.contains("encrypted_content"),
        });
    }
    Ok(collected)
}

fn history_rollout_files(codex_home: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for dir in [
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ] {
        if dir.exists() {
            collect_history_rollout_files(&dir, &mut files)?;
        }
    }
    files.sort();
    Ok(files)
}

fn collect_history_rollout_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|err| format!("无法读取目录 {}：{err}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|err| format!("无法读取目录项 {}：{err}", dir.display()))?
            .path();
        if path.is_dir() {
            collect_history_rollout_files(&path, files)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn split_first_line(text: &str) -> (String, String) {
    if let Some(index) = text.find('\n') {
        (text[..index].to_string(), text[index..].to_string())
    } else {
        (text.to_string(), String::new())
    }
}

fn apply_history_migration_change(change: &HistoryMigrationChange) -> Result<bool, String> {
    let current = fs::read_to_string(&change.path)
        .map_err(|err| format!("无法读取会话文件 {}：{err}", display_path(&change.path)))?;
    let (current_first_line, current_rest) = split_first_line(&current);
    if current_first_line != change.original_first_line {
        return Ok(false);
    }
    fs::write(
        &change.path,
        format!("{}{}", change.next_first_line, current_rest),
    )
    .map_err(|err| format!("无法写入会话文件 {}：{err}", display_path(&change.path)))?;
    Ok(true)
}

fn restore_applied_history_migration_changes(
    changes: &[HistoryMigrationChange],
) -> Result<(), String> {
    for change in changes {
        replace_history_first_line(&change.path, &change.original_first_line)?;
    }
    Ok(())
}

fn replace_history_first_line(path: &Path, first_line: &str) -> Result<(), String> {
    let current = fs::read_to_string(path)
        .map_err(|err| format!("无法读取会话文件 {}：{err}", display_path(path)))?;
    let (_, rest) = split_first_line(&current);
    fs::write(path, format!("{first_line}{rest}"))
        .map_err(|err| format!("无法写入会话文件 {}：{err}", display_path(path)))
}

fn create_history_migration_backup(
    codex_home: &Path,
    target_provider: &str,
) -> Result<PathBuf, String> {
    let backup_root = codex_home.join(HISTORY_MIGRATION_BACKUP_DIR_NAME);
    fs::create_dir_all(&backup_root).map_err(|err| format!("无法创建历史迁移备份目录：{err}"))?;
    let mut backup_dir = backup_root.join(Local::now().format("%Y%m%d%H%M%S").to_string());
    let mut suffix = 0;
    while backup_dir.exists() {
        suffix += 1;
        backup_dir = backup_root.join(format!("{}-{suffix}", Local::now().format("%Y%m%d%H%M%S")));
    }
    fs::create_dir_all(&backup_dir).map_err(|err| format!("无法创建历史迁移备份：{err}"))?;

    for name in [
        "config.toml",
        CODEX_STATE_DB_NAME,
        "state_5.sqlite-wal",
        "state_5.sqlite-shm",
    ] {
        let source = codex_home.join(name);
        if source.exists() {
            fs::copy(&source, backup_dir.join(name))
                .map_err(|err| format!("无法备份 {name}：{err}"))?;
        }
    }
    fs::write(
        backup_dir.join("metadata.json"),
        serde_json::to_string_pretty(&json!({
            "managedBy": "OceanWay history migration",
            "targetProvider": target_provider,
            "createdAt": Local::now().to_rfc3339(),
        }))
        .map_err(|err| format!("无法生成历史迁移备份元数据：{err}"))?,
    )
    .map_err(|err| format!("无法写入历史迁移备份元数据：{err}"))?;
    Ok(backup_dir)
}

fn write_history_migration_manifest(
    backup_dir: &Path,
    target_provider: &str,
    applied: &[HistoryMigrationChange],
) -> Result<(), String> {
    let manifest = HistoryMigrationManifest {
        version: 1,
        target_provider: target_provider.to_string(),
        created_at: Local::now().to_rfc3339(),
        files: applied
            .iter()
            .map(|change| HistoryMigrationManifestFile {
                path: display_path(&change.path),
                thread_id: change.thread_id.clone(),
                original_provider: change.original_provider.clone(),
                migrated_provider: target_provider.to_string(),
                original_first_line: change.original_first_line.clone(),
            })
            .collect(),
    };
    let rendered = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("无法生成历史迁移清单：{err}"))?
        + "\n";
    fs::write(backup_dir.join("history-migration.json"), rendered)
        .map_err(|err| format!("无法写入历史迁移清单：{err}"))
}

fn table_columns(db: &Connection, table: &str) -> Result<HashSet<String>, String> {
    let mut stmt = db
        .prepare(&format!(
            "PRAGMA table_info(\"{}\")",
            table.replace('"', "\"\"")
        ))
        .map_err(|err| format!("无法读取 SQLite 表结构：{err}"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| format!("无法读取 SQLite 表结构：{err}"))?
        .collect::<rusqlite::Result<HashSet<_>>>()
        .map_err(|err| format!("无法读取 SQLite 表结构：{err}"))?;
    Ok(columns)
}

fn count_history_migration_sqlite_rows(
    codex_home: &Path,
    target_provider: &str,
    changes: &[HistoryMigrationChange],
) -> Result<usize, String> {
    let db_path = codex_home.join(CODEX_STATE_DB_NAME);
    if !db_path.exists() {
        return Ok(0);
    }
    let db = Connection::open(&db_path)
        .map_err(|err| format!("无法打开 Codex 状态数据库 {}：{err}", db_path.display()))?;
    let columns = table_columns(&db, "threads")?;
    if !columns.contains("model_provider") {
        return Ok(0);
    }
    let mut seen = HashSet::new();
    let mut rows = 0;
    for thread_id in changes
        .iter()
        .filter_map(|change| change.thread_id.as_ref())
    {
        if !seen.insert(thread_id.clone()) {
            continue;
        }
        rows += db
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE id = ?1 AND COALESCE(model_provider, '') <> ?2",
                params![thread_id, target_provider],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|err| format!("无法统计历史迁移数据库行：{err}"))? as usize;
    }
    Ok(rows)
}

fn update_history_migration_sqlite(
    codex_home: &Path,
    target_provider: &str,
    changes: &[HistoryMigrationChange],
) -> Result<usize, String> {
    let db_path = codex_home.join(CODEX_STATE_DB_NAME);
    if !db_path.exists() {
        return Ok(0);
    }
    let mut db = Connection::open(&db_path)
        .map_err(|err| format!("无法打开 Codex 状态数据库 {}：{err}", db_path.display()))?;
    let columns = table_columns(&db, "threads")?;
    if !columns.contains("model_provider") {
        return Ok(0);
    }
    let tx = db
        .transaction()
        .map_err(|err| format!("无法开始历史迁移数据库事务：{err}"))?;
    let mut seen = HashSet::new();
    let mut rows = 0;
    for thread_id in changes
        .iter()
        .filter_map(|change| change.thread_id.as_ref())
    {
        if !seen.insert(thread_id.clone()) {
            continue;
        }
        rows += tx
            .execute(
                "UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND COALESCE(model_provider, '') <> ?1",
                params![target_provider, thread_id],
            )
            .map_err(|err| format!("无法更新历史迁移数据库行：{err}"))?;
    }
    tx.commit()
        .map_err(|err| format!("无法提交历史迁移数据库事务：{err}"))?;
    Ok(rows)
}

fn history_migration_warning_text(
    encrypted_content_files: usize,
    target_provider: &str,
) -> Vec<String> {
    if encrypted_content_files == 0 {
        return Vec::new();
    }
    vec![format!(
        "检测到 {encrypted_content_files} 个会话包含 encrypted_content。迁移只修复列表可见性；切到 {target_provider} 后继续对话或 compact 仍可能失败。"
    )]
}

fn restore_history_migrations_lossy(codex_home: &Path) -> HistoryMigrationRestoreResult {
    match restore_history_migrations(codex_home) {
        Ok(result) => result,
        Err(err) => HistoryMigrationRestoreResult {
            warnings: vec![format!("历史迁移撤销未完成：{err}")],
            ..HistoryMigrationRestoreResult::default()
        },
    }
}

fn restore_history_migrations(codex_home: &Path) -> Result<HistoryMigrationRestoreResult, String> {
    let backup_dirs = history_migration_backup_dirs(codex_home)?;
    let mut result = HistoryMigrationRestoreResult::default();
    for backup_dir in backup_dirs {
        if backup_dir.join("restored.json").exists() {
            continue;
        }
        let manifest_path = backup_dir.join("history-migration.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest_content = fs::read_to_string(&manifest_path)
            .map_err(|err| format!("无法读取历史迁移清单：{err}"))?;
        let manifest = serde_json::from_str::<HistoryMigrationManifest>(&manifest_content)
            .map_err(|err| format!("历史迁移清单无效：{err}"))?;
        for file in &manifest.files {
            let path = PathBuf::from(&file.path);
            if !path.exists() {
                result
                    .warnings
                    .push(format!("历史迁移撤销跳过缺失文件：{}", file.path));
                continue;
            }
            match replace_history_first_line(&path, &file.original_first_line) {
                Ok(()) => result.restored_session_files += 1,
                Err(err) => result.warnings.push(err),
            }
        }
        result.sqlite_rows_restored +=
            restore_history_migration_sqlite(codex_home, &manifest.files)?;
        fs::write(
            backup_dir.join("restored.json"),
            serde_json::to_string_pretty(&json!({
                "restoredAt": Local::now().to_rfc3339(),
            }))
            .map_err(|err| format!("无法生成历史迁移撤销元数据：{err}"))?,
        )
        .map_err(|err| format!("无法写入历史迁移撤销元数据：{err}"))?;
        result.restored_backups += 1;
    }
    Ok(result)
}

fn history_migration_backup_dirs(codex_home: &Path) -> Result<Vec<PathBuf>, String> {
    let root = codex_home.join(HISTORY_MIGRATION_BACKUP_DIR_NAME);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut dirs = Vec::new();
    for entry in fs::read_dir(&root).map_err(|err| format!("无法读取历史迁移备份目录：{err}"))?
    {
        let path = entry
            .map_err(|err| format!("无法读取历史迁移备份目录项：{err}"))?
            .path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    Ok(dirs)
}

fn restore_history_migration_sqlite(
    codex_home: &Path,
    files: &[HistoryMigrationManifestFile],
) -> Result<usize, String> {
    let db_path = codex_home.join(CODEX_STATE_DB_NAME);
    if !db_path.exists() {
        return Ok(0);
    }
    let mut db = Connection::open(&db_path)
        .map_err(|err| format!("无法打开 Codex 状态数据库 {}：{err}", db_path.display()))?;
    let columns = table_columns(&db, "threads")?;
    if !columns.contains("model_provider") {
        return Ok(0);
    }
    let tx = db
        .transaction()
        .map_err(|err| format!("无法开始历史迁移撤销数据库事务：{err}"))?;
    let mut seen = HashSet::new();
    let mut rows = 0;
    for file in files {
        let Some(thread_id) = file.thread_id.as_ref() else {
            continue;
        };
        if !seen.insert(thread_id.clone()) {
            continue;
        }
        let original_provider = file
            .original_provider
            .clone()
            .unwrap_or_else(|| "openai".to_string());
        rows += tx
            .execute(
                "UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND model_provider = ?3",
                params![original_provider, thread_id, file.migrated_provider],
            )
            .map_err(|err| format!("无法撤销历史迁移数据库行：{err}"))?;
    }
    tx.commit()
        .map_err(|err| format!("无法提交历史迁移撤销数据库事务：{err}"))?;
    Ok(rows)
}

fn read_session_provider_from_config(config_path: &Path) -> String {
    let content = fs::read_to_string(config_path).unwrap_or_default();
    read_root_string(&content, "model_provider").unwrap_or_else(|| "openai".to_string())
}

fn merge_config(
    original: &str,
    provider_id: &str,
    base_url: &str,
    model: &str,
    bearer_token: Option<&str>,
    auth_strategy: ProviderAuthStrategy,
) -> String {
    let lines = original.split_inclusive('\n').collect::<Vec<_>>();
    let (root_line_refs, table_lines) = split_root_and_tables(&lines);
    let mut root_lines = root_line_refs
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();

    root_lines = set_root_key(root_lines, "model_provider", &toml_string(provider_id));
    root_lines = set_root_key(root_lines, "model", &toml_string(model));
    root_lines = set_root_key(root_lines, "model_reasoning_effort", "\"high\"");
    root_lines = set_root_key(root_lines, "disable_response_storage", "true");

    let mut rendered = root_lines.join("");
    if !rendered.ends_with("\n\n") {
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push('\n');
    }

    let rest = remove_provider_table_family(&table_lines, provider_id).join("");
    let rest = rest.trim_start_matches('\n');
    if !rest.trim().is_empty() {
        rendered.push_str(rest.trim_end());
        rendered.push_str("\n\n");
    }

    rendered.push_str(&render_provider_block(
        provider_id,
        base_url,
        bearer_token,
        auth_strategy,
    ));
    rendered
}

fn remove_provider_config(original: &str, provider_id: &str) -> String {
    let lines = original.split_inclusive('\n').collect::<Vec<_>>();
    let (root_line_refs, table_lines) = split_root_and_tables(&lines);
    let mut root_lines = root_line_refs
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();

    root_lines = remove_root_key_if_value(root_lines, "model_provider", provider_id);

    let mut rendered = root_lines.join("");
    let rest = remove_provider_table_family(&table_lines, provider_id).join("");
    let rest = rest.trim_start_matches('\n');

    if !rendered.trim().is_empty() && !rest.trim().is_empty() && !rendered.ends_with("\n\n") {
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push('\n');
    }

    if !rest.trim().is_empty() {
        rendered.push_str(rest.trim_end());
        rendered.push('\n');
    }

    rendered
}

fn render_provider_block(
    provider_id: &str,
    base_url: &str,
    bearer_token: Option<&str>,
    auth_strategy: ProviderAuthStrategy,
) -> String {
    let mut rendered = format!(
        concat!(
            "[model_providers.{table_provider}]\n",
            "name = {provider}\n",
            "base_url = {base_url}\n",
            "wire_api = \"responses\"\n",
        ),
        table_provider = provider_id,
        provider = toml_string(provider_id),
        base_url = toml_string(base_url),
    );
    match auth_strategy {
        ProviderAuthStrategy::ApiKey => {
            rendered.push_str("requires_openai_auth = false\n");
            rendered.push_str(&format!(
                "http_headers = {{ \"x-openai-actor-authorization\" = {} }}\n",
                toml_string(IMAGE_EXTENSION_ACTOR_AUTHORIZATION)
            ));
        }
        ProviderAuthStrategy::ChatGptBearerToken => {
            if let Some(token) = bearer_token.filter(|token| !token.trim().is_empty()) {
                rendered.push_str(&format!(
                    "experimental_bearer_token = {}\n",
                    toml_string(token.trim())
                ));
            }
            rendered.push_str("requires_openai_auth = true\n");
        }
    }
    rendered
}

fn merge_imagegen_cli_environment(
    content: &str,
    api_key: &str,
    base_url: &str,
) -> Result<String, String> {
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|err| format!("现有 config.toml 无法解析，未写入 imagegen CLI 配置：{err}"))?;
    let policy_was_missing = !document.as_table().contains_key("shell_environment_policy");
    let policy = ensure_table(document.as_table_mut(), "shell_environment_policy")?;
    if policy_was_missing {
        policy.set_implicit(true);
    }
    let environment = ensure_table(policy, "set")?;
    environment[CODEX_AUTH_KEY] = value(api_key.trim());
    environment[OPENAI_BASE_URL_ENV_KEY] = value(base_url.trim_end_matches('/'));
    Ok(document.to_string())
}

fn ensure_table<'a>(parent: &'a mut Table, key: &str) -> Result<&'a mut Table, String> {
    if !parent.contains_key(key) {
        parent.insert(key, Item::Table(Table::new()));
    }

    let item = parent
        .get_mut(key)
        .ok_or_else(|| format!("无法创建 config.toml 表：{key}"))?;
    if item.is_table() {
        return item
            .as_table_mut()
            .ok_or_else(|| format!("无法读取 config.toml 表：{key}"));
    }

    let Some(inline) = item.as_value_mut().and_then(TomlValue::as_inline_table_mut) else {
        return Err(format!(
            "config.toml 中的 {key} 不是表，无法安全写入 imagegen CLI 配置。"
        ));
    };

    let mut table = Table::new();
    for (inline_key, inline_value) in inline.iter() {
        table.insert(inline_key, Item::Value(inline_value.clone()));
    }
    *item = Item::Table(table);
    item.as_table_mut()
        .ok_or_else(|| format!("无法转换 config.toml 表：{key}"))
}

fn read_shell_environment_string(content: &str, key: &str) -> Option<String> {
    let document = content.parse::<DocumentMut>().ok()?;
    document
        .get("shell_environment_policy")?
        .get("set")?
        .get(key)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn has_matching_imagegen_cli_environment(
    content: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> bool {
    let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) else {
        return false;
    };

    read_shell_environment_string(content, CODEX_AUTH_KEY).as_deref() == Some(api_key.trim())
        && read_shell_environment_string(content, OPENAI_BASE_URL_ENV_KEY).as_deref()
            == Some(base_url.trim_end_matches('/'))
}

fn remove_imagegen_cli_environment(content: &str) -> Result<String, String> {
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|err| format!("现有 config.toml 无法解析，未移除 imagegen CLI 配置：{err}"))?;
    let Some(policy_item) = document.as_table_mut().get_mut("shell_environment_policy") else {
        return Ok(content.to_string());
    };
    let Some(policy) = policy_item.as_table_mut() else {
        return Ok(content.to_string());
    };
    let Some(environment_item) = policy.get_mut("set") else {
        return Ok(content.to_string());
    };

    if let Some(environment) = environment_item.as_table_mut() {
        environment.remove(CODEX_AUTH_KEY);
        environment.remove(OPENAI_BASE_URL_ENV_KEY);
        if environment.is_empty() {
            policy.remove("set");
        }
    } else if let Some(environment) = environment_item
        .as_value_mut()
        .and_then(TomlValue::as_inline_table_mut)
    {
        environment.remove(CODEX_AUTH_KEY);
        environment.remove(OPENAI_BASE_URL_ENV_KEY);
        if environment.is_empty() {
            policy.remove("set");
        }
    }

    if policy.is_empty() {
        document.as_table_mut().remove("shell_environment_policy");
    }
    Ok(document.to_string())
}

fn remove_imagegen_cli_environment_from_file(config_path: &Path) -> Result<(), String> {
    let original = fs::read_to_string(config_path).unwrap_or_default();
    let rendered = remove_imagegen_cli_environment(&original)?;
    fs::write(config_path, rendered).map_err(|err| format!("无法写入 config.toml：{err}"))
}

fn split_root_and_tables<'a>(lines: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && !trimmed.starts_with('#') {
            return (lines[..index].to_vec(), lines[index..].to_vec());
        }
    }

    (lines.to_vec(), Vec::new())
}

fn set_root_key(lines: Vec<String>, key: &str, rendered_value: &str) -> Vec<String> {
    let rendered = format!("{key} = {rendered_value}\n");
    let mut output = Vec::with_capacity(lines.len() + 2);
    let mut replaced = false;

    for line in lines {
        let trimmed = line.trim_start();
        let current_key = line
            .split_once('=')
            .map(|(current_key, _)| current_key.trim());
        if !trimmed.starts_with('#') && current_key == Some(key) {
            if !replaced {
                output.push(rendered.clone());
                replaced = true;
            }
            continue;
        }
        output.push(line);
    }

    if !replaced {
        while output.last().is_some_and(|line| line.trim().is_empty()) {
            output.pop();
        }
        output.push(rendered);
    }

    output
}

fn remove_root_key_if_value(lines: Vec<String>, key: &str, expected_value: &str) -> Vec<String> {
    let mut output = Vec::with_capacity(lines.len());
    let mut removed = false;

    for line in lines {
        let trimmed = line.trim_start();
        let Some((current_key, value)) = line.split_once('=') else {
            output.push(line);
            continue;
        };

        if !removed
            && !trimmed.starts_with('#')
            && current_key.trim() == key
            && parse_quoted_toml_string(value.trim()).as_deref() == Some(expected_value)
        {
            removed = true;
            continue;
        }

        output.push(line);
    }

    while output.len() > 1
        && output.last().is_some_and(|line| line.trim().is_empty())
        && output
            .get(output.len() - 2)
            .is_some_and(|line| line.trim().is_empty())
    {
        output.pop();
    }

    output
}

fn remove_provider_table_family<'a>(lines: &[&'a str], provider_id: &str) -> Vec<&'a str> {
    let mut output = Vec::new();
    let mut skipping = false;
    let target = format!("model_providers.{provider_id}");
    let quoted_target = format!("model_providers.\"{provider_id}\"");

    for line in lines {
        if let Some(path) = table_path(line) {
            skipping = path == target
                || path == quoted_target
                || path.starts_with(&(target.clone() + "."))
                || path.starts_with(&(quoted_target.clone() + "."));
        }

        if !skipping {
            output.push(*line);
        }
    }

    output
}

fn table_path(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') || !trimmed.starts_with('[') {
        return None;
    }

    let trimmed = trimmed.trim_start_matches('[');
    let end = trimmed.find(']')?;
    Some(trimmed[..end].trim().to_string())
}

fn read_current_model(config_path: &Path) -> Option<String> {
    let content = fs::read_to_string(config_path).ok()?;
    read_current_model_from_content(&content)
}

fn read_current_model_from_content(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || !trimmed.starts_with("model") {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };

        if key.trim() != "model" {
            continue;
        }

        let value = value.trim();
        if let Some(stripped) = parse_quoted_toml_string(value) {
            return Some(stripped);
        }
    }

    None
}

fn read_root_string(content: &str, target_key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.starts_with('[') {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };

        if key.trim() == target_key {
            return parse_quoted_toml_string(value.trim());
        }
    }

    None
}

fn read_provider_base_url(content: &str, provider_id: &str) -> Option<String> {
    read_provider_string(content, provider_id, "base_url")
}

fn read_provider_bearer_token(content: &str, provider_id: &str) -> Option<String> {
    read_provider_string(content, provider_id, "experimental_bearer_token")
}

fn read_provider_string(content: &str, provider_id: &str, target_key: &str) -> Option<String> {
    read_provider_raw_value(content, provider_id, target_key)
        .and_then(|value| parse_quoted_toml_string(&value))
}

fn read_provider_bool(content: &str, provider_id: &str, target_key: &str) -> Option<bool> {
    read_provider_raw_value(content, provider_id, target_key).and_then(|value| {
        match value.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    })
}

fn read_provider_raw_value(content: &str, provider_id: &str, target_key: &str) -> Option<String> {
    let mut in_provider = false;
    let provider_header = format!("[model_providers.{provider_id}]");
    let quoted_provider_header = format!("[model_providers.\"{provider_id}\"]");

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') {
            in_provider = trimmed == provider_header || trimmed == quoted_provider_header;
            continue;
        }

        if !in_provider {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };

        if key.trim() == target_key {
            return Some(value.trim().to_string());
        }
    }

    None
}

fn choose_provider_auth_strategy(auth_path: &Path) -> ProviderAuthStrategy {
    if read_auth_has_chatgpt_login(auth_path) {
        ProviderAuthStrategy::ChatGptBearerToken
    } else {
        ProviderAuthStrategy::ApiKey
    }
}

fn read_auth_has_chatgpt_login(auth_path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(auth_path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };

    if object
        .get("auth_mode")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("chatgpt"))
    {
        return true;
    }

    ["tokens", "access_token", "refresh_token", "id_token"]
        .iter()
        .any(|key| object.get(*key).is_some_and(auth_value_looks_present))
}

fn read_auth_chatgpt_account_label(auth_path: &Path) -> Option<String> {
    let content = fs::read_to_string(auth_path).ok()?;
    let value = serde_json::from_str::<Value>(&content).ok()?;
    find_account_label_in_value(&value).or_else(|| find_account_label_in_jwts(&value))
}

fn find_account_label_in_value(value: &Value) -> Option<String> {
    match value {
        Value::Object(object) => {
            for key in [
                "email",
                "user_email",
                "account_email",
                "preferred_username",
                "name",
                "display_name",
            ] {
                if let Some(label) = object.get(key).and_then(clean_account_label) {
                    return Some(label);
                }
            }

            for child in object.values() {
                if let Some(label) = find_account_label_in_value(child) {
                    return Some(label);
                }
            }
            None
        }
        Value::Array(values) => values.iter().find_map(find_account_label_in_value),
        _ => None,
    }
}

fn find_account_label_in_jwts(value: &Value) -> Option<String> {
    match value {
        Value::String(token) => account_label_from_jwt(token),
        Value::Array(values) => values.iter().find_map(find_account_label_in_jwts),
        Value::Object(object) => object.values().find_map(find_account_label_in_jwts),
        _ => None,
    }
}

fn account_label_from_jwt(token: &str) -> Option<String> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _signature = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let decoded = URL_SAFE_NO_PAD.decode(payload.as_bytes()).ok()?;
    let value = serde_json::from_slice::<Value>(&decoded).ok()?;
    find_account_label_in_value(&value)
}

fn clean_account_label(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn auth_value_looks_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(values) => values.iter().any(auth_value_looks_present),
        Value::Object(values) => values.values().any(auth_value_looks_present),
        Value::Bool(_) | Value::Number(_) => true,
    }
}

fn parse_quoted_toml_string(value: &str) -> Option<String> {
    if !value.starts_with('"') {
        return None;
    }

    let mut escaped = false;
    let mut output = String::new();

    for ch in value[1..].chars() {
        if escaped {
            output.push(match ch {
                'n' => '\n',
                'r' => '\r',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '"' => return Some(output),
            other => output.push(other),
        }
    }

    None
}

fn backup_file(path: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("无法读取文件名：{}", path.display()))?;
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stamp = format!(
        "{}-{}-{sequence}",
        Local::now().format("%Y%m%d-%H%M%S-%f"),
        process::id()
    );
    let backup_path = path.with_file_name(format!("{file_name}.bak.{stamp}"));

    copy_private_new(path, &backup_path)
        .map_err(|err| format!("无法备份 {}：{err}", path.display()))?;
    Ok(Some(backup_path))
}

// Exclusive creation prevents collisions from truncating an existing backup.
fn copy_private_new(source: &Path, destination: &Path) -> std::io::Result<()> {
    let mut input = fs::File::open(source)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()
}

// Stage private contents next to the destination before replacement. Failed writes
// leave the previous file intact; newly created secret files never start at 0644.
fn write_private_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("invalid file path"))?;
    let temporary = path.with_file_name(format!(
        ".{}.tmp-{}-{}-{sequence}",
        name.to_string_lossy(),
        process::id(),
        Local::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = file.write_all(contents).and_then(|_| file.sync_all());
    drop(file);
    let result = result.and_then(|_| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn secure_snapshot_permissions(directory: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| "无法保护备份目录权限，已停止写入。".to_string())?;
    }
    for name in ["config.toml", "auth.json", "meta.json"] {
        let path = directory.join(name);
        if path.exists() {
            set_private_permissions(&path)?;
        }
    }
    Ok(())
}

fn validate_base_url(base_url: &str) -> Result<(), String> {
    let message = "Base URL 必须是完整的 HTTP(S) 地址，且不能包含账号密码、查询参数或片段。";
    let url = reqwest::Url::parse(base_url).map_err(|_| message.to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || base_url.chars().any(char::is_whitespace)
    {
        return Err(message.into());
    }
    Ok(())
}

fn rollback_auth(auth_path: &Path, old_auth: Option<Vec<u8>>) {
    match old_auth {
        Some(bytes) => {
            let _ = fs::write(auth_path, bytes);
        }
        None => {
            let _ = fs::remove_file(auth_path);
        }
    }
}

fn collect_system_info(codex_home: &Path) -> SystemInfo {
    let (codex_desktop_version, codex_host, codex_running) = codex_runtime_info();
    SystemInfo {
        operating_system: match env::consts::OS {
            "macos" => "macOS".to_string(),
            "windows" => "Windows".to_string(),
            other => other.to_string(),
        },
        operating_system_version: operating_system_version()
            .unwrap_or_else(|| "未知版本".to_string()),
        architecture: env::consts::ARCH.to_string(),
        codex_cli_version: command_output("codex", &["--version"]),
        codex_desktop_version,
        codex_host,
        codex_running,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        backup_created_at: read_restore_snapshot_created_at(codex_home),
    }
}

fn run_diagnostics_in_home(codex_home: &Path) -> Result<DiagnosticReport, String> {
    let config_path = codex_home.join("config.toml");
    let auth_path = codex_home.join("auth.json");
    let config = fs::read_to_string(&config_path).unwrap_or_default();
    let provider_id = read_root_string(&config, "model_provider");
    let base_url = read_provider_base_url(&config, PROVIDER_ID);
    let provider_token = read_provider_bearer_token(&config, PROVIDER_ID);
    let auth_api_key = read_auth_api_key(&auth_path);
    let api_key = provider_token.as_ref().or(auth_api_key.as_ref());
    let chatgpt_login = read_auth_has_chatgpt_login(&auth_path);
    let provider_active = provider_id.as_deref() == Some(PROVIDER_ID);
    let system = collect_system_info(codex_home);
    let mut checks = Vec::new();

    checks.push(diagnostic_check(
        "provider",
        "OceanWay provider",
        if provider_active { "pass" } else { "error" },
        if provider_active {
            format!(
                "当前 Base URL：{}",
                base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)
            )
        } else {
            "当前 Codex 尚未切换到 OceanWay。".to_string()
        },
        !provider_active,
    ));

    let has_api_key = api_key.is_some_and(|value| !value.trim().is_empty());
    checks.push(diagnostic_check(
        "credential",
        "认证凭据",
        if has_api_key { "pass" } else { "error" },
        if has_api_key {
            if provider_token.is_some() {
                "已检测到 provider 专用凭据，报告不会输出完整内容。".to_string()
            } else {
                "已检测到本机 API Key，报告不会输出完整内容。".to_string()
            }
        } else {
            "未找到已保存的 OceanWay API Key。".to_string()
        },
        !has_api_key,
    ));

    let imagegen_ready = provider_active
        && has_matching_imagegen_cli_environment(
            &config,
            api_key.map(String::as_str),
            base_url.as_deref(),
        );
    checks.push(diagnostic_check(
        "imagegen",
        "图片备用链路",
        if imagegen_ready { "pass" } else { "warning" },
        if imagegen_ready {
            "CLI 备用环境与当前 Key、Base URL 一致。".to_string()
        } else {
            "图片备用环境缺失或已过期，可在运维工具中修复。".to_string()
        },
        !imagegen_ready && provider_active && has_api_key,
    ));

    let compatibility_ready = if chatgpt_login {
        provider_token.is_some()
            && read_provider_bool(&config, PROVIDER_ID, "requires_openai_auth") == Some(true)
    } else {
        read_provider_bool(&config, PROVIDER_ID, "requires_openai_auth") == Some(false)
            && read_provider_raw_value(&config, PROVIDER_ID, "http_headers").is_some_and(|value| {
                value.contains("x-openai-actor-authorization")
                    && value.contains(IMAGE_EXTENSION_ACTOR_AUTHORIZATION)
            })
    };
    checks.push(diagnostic_check(
        "auth-mode",
        "Codex 图片兼容模式",
        if compatibility_ready {
            "pass"
        } else {
            "warning"
        },
        if compatibility_ready {
            if chatgpt_login {
                "已保留 ChatGPT 登录态并使用 provider 专用凭据。".to_string()
            } else {
                "API Key 模式所需的本地图片扩展标记已就绪。".to_string()
            }
        } else {
            "当前认证模式缺少图片工具所需的兼容字段。".to_string()
        },
        !compatibility_ready && provider_active && has_api_key,
    ));

    let version_value = system
        .codex_desktop_version
        .as_deref()
        .or(system.codex_cli_version.as_deref());
    let version_status = match version_value {
        Some(version) if version_at_least(version, MINIMUM_IMAGE_EXTENSION_VERSION) => "pass",
        Some(_) => "warning",
        None => "warning",
    };
    let version_detail = match version_value {
        Some(version) if version_status == "pass" => {
            format!("检测到 Codex {version}，满足当前兼容要求。")
        }
        Some(version) => format!(
            "检测到 Codex {version}；建议升级到 {MINIMUM_IMAGE_EXTENSION_VERSION} 或更高版本。"
        ),
        None => "未检测到 Codex 版本，请确认 Codex Desktop 或 CLI 已安装。".to_string(),
    };
    checks.push(diagnostic_check(
        "codex-version",
        "Codex 版本兼容",
        version_status,
        version_detail,
        false,
    ));

    let connection = if provider_active && has_api_key {
        Some(test_connection_internal(
            api_key.map(String::as_str).unwrap_or_default(),
            base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
        ))
    } else {
        None
    };
    checks.push(diagnostic_check(
        "connection",
        "OceanWay 服务连接",
        match connection.as_ref() {
            Some(result) if result.ok => "pass",
            Some(_) => "error",
            None => "warning",
        },
        connection
            .as_ref()
            .map(|result| result.message.clone())
            .unwrap_or_else(|| "完成 provider 和 API Key 配置后才能测试连接。".to_string()),
        false,
    ));

    checks.push(diagnostic_check(
        "codex-process",
        "Codex 生效状态",
        if system.codex_running { "pass" } else { "warning" },
        if system.codex_running {
            match system.codex_host.as_deref() {
                Some("ChatGPT") => {
                    "已检测到 Codex 正通过 ChatGPT 运行。配置发生变化后，请重启 ChatGPT 并新建任务。"
                        .to_string()
                }
                _ => "已检测到 Codex Desktop 正在运行。配置发生变化后，请重启并新建任务。"
                    .to_string(),
            }
        } else {
            "Codex 当前未运行；启动 Codex 或 ChatGPT 后会读取最新配置。".to_string()
        },
        false,
    ));

    checks.push(diagnostic_check(
        "backup",
        "恢复快照",
        if system.backup_created_at.is_some() {
            "pass"
        } else {
            "warning"
        },
        system
            .backup_created_at
            .as_ref()
            .map(|created_at| format!("首次配置快照创建于 {created_at}。"))
            .unwrap_or_else(|| "尚未创建首次配置快照。完成配置时会自动创建。".to_string()),
        false,
    ));

    let passed = checks.iter().filter(|check| check.status == "pass").count();
    let warnings = checks
        .iter()
        .filter(|check| check.status == "warning")
        .count();
    let errors = checks
        .iter()
        .filter(|check| check.status == "error")
        .count();
    let overall_status = if errors > 0 {
        "error"
    } else if warnings > 0 {
        "warning"
    } else {
        "pass"
    }
    .to_string();
    let generated_at = Local::now().to_rfc3339();
    let redacted_report =
        render_redacted_diagnostic_report(&generated_at, &overall_status, &system, &checks);

    Ok(DiagnosticReport {
        generated_at,
        overall_status,
        passed,
        warnings,
        errors,
        checks,
        system,
        redacted_report,
    })
}

fn diagnostic_check(
    id: &str,
    label: &str,
    status: &str,
    detail: String,
    repairable: bool,
) -> DiagnosticCheck {
    DiagnosticCheck {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        detail,
        repairable,
    }
}

fn render_redacted_diagnostic_report(
    generated_at: &str,
    overall_status: &str,
    system: &SystemInfo,
    checks: &[DiagnosticCheck],
) -> String {
    let mut lines = vec![
        "OceanWay Codex 脱敏诊断报告".to_string(),
        format!("生成时间：{generated_at}"),
        format!("总体状态：{overall_status}"),
        format!(
            "系统：{} {} ({})",
            system.operating_system, system.operating_system_version, system.architecture
        ),
        format!("配置工具版本：{}", system.app_version),
        format!(
            "Codex Desktop：{}",
            system
                .codex_desktop_version
                .as_deref()
                .unwrap_or("未检测到")
        ),
        format!(
            "Codex 宿主：{}",
            system.codex_host.as_deref().unwrap_or("未检测到")
        ),
        format!(
            "Codex CLI：{}",
            system.codex_cli_version.as_deref().unwrap_or("未检测到")
        ),
        format!(
            "Codex 进程：{}",
            if system.codex_running {
                "正在运行"
            } else {
                "未运行"
            }
        ),
        String::new(),
        "检查结果：".to_string(),
    ];
    for check in checks {
        lines.push(format!(
            "- [{}] {}：{}",
            check.status.to_uppercase(),
            check.label,
            check.detail
        ));
    }
    lines.extend([
        String::new(),
        "安全说明：本报告不会包含 API Key、Bearer Token 或完整认证内容。".to_string(),
    ]);
    lines.join("\n")
}

fn read_restore_snapshot_created_at(codex_home: &Path) -> Option<String> {
    let path = codex_home.join(BACKUP_DIR_NAME).join("meta.json");
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str::<RestoreSnapshotMeta>(&content)
        .ok()
        .map(|meta| meta.created_at)
}

fn command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn operating_system_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        command_output("sw_vers", &["-productVersion"])
    }

    #[cfg(target_os = "windows")]
    {
        return command_output(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_OperatingSystem).Version",
            ],
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        command_output("uname", &["-sr"])
    }
}

fn codex_runtime_info() -> (Option<String>, Option<String>, bool) {
    #[cfg(target_os = "macos")]
    {
        let process_list = macos_process_list().unwrap_or_default();
        let running_host = macos_codex_host_from_process_list(&process_list);
        let host = running_host.or_else(macos_installed_codex_host);
        let version = host.and_then(macos_codex_version);
        (
            version,
            host.map(|value| value.label().to_string()),
            running_host.is_some(),
        )
    }

    #[cfg(target_os = "windows")]
    {
        let version = find_windows_codex_executable().and_then(|executable| {
            command_output(
                "powershell",
                &[
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "(Get-Item '{}').VersionInfo.ProductVersion",
                        display_path(&executable).replace('\'', "''")
                    ),
                ],
            )
        });
        let installed = find_windows_codex_executable().is_some();
        (
            version,
            installed.then(|| "Codex Desktop".to_string()),
            is_codex_running(),
        )
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        (None, None, false)
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MacosCodexHost {
    Standalone,
    ChatGpt,
}

#[cfg(target_os = "macos")]
impl MacosCodexHost {
    fn app_name(self) -> &'static str {
        match self {
            Self::Standalone => "Codex",
            Self::ChatGpt => "ChatGPT",
        }
    }

    fn bundle_name(self) -> &'static str {
        match self {
            Self::Standalone => "Codex.app",
            Self::ChatGpt => "ChatGPT.app",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Standalone => "Codex Desktop",
            Self::ChatGpt => "ChatGPT",
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_app_path(host: MacosCodexHost) -> Option<PathBuf> {
    let system_path = PathBuf::from("/Applications").join(host.bundle_name());
    if system_path.exists() {
        return Some(system_path);
    }

    let user_path = env::var_os("HOME")
        .map(PathBuf::from)?
        .join("Applications")
        .join(host.bundle_name());
    user_path.exists().then_some(user_path)
}

#[cfg(target_os = "macos")]
fn macos_installed_codex_host() -> Option<MacosCodexHost> {
    [MacosCodexHost::Standalone, MacosCodexHost::ChatGpt]
        .into_iter()
        .find(|host| {
            macos_app_path(*host).is_some_and(|path| {
                path.join("Contents/Resources/codex").exists()
                    || matches!(host, MacosCodexHost::Standalone)
            })
        })
}

#[cfg(target_os = "macos")]
fn macos_process_list() -> Option<String> {
    command_output("ps", &["-axo", "command="])
}

#[cfg(target_os = "macos")]
fn macos_codex_host_from_process_list(process_list: &str) -> Option<MacosCodexHost> {
    let standalone_running = process_list
        .lines()
        .any(|line| line.contains("/Codex.app/Contents/MacOS/Codex"));
    if standalone_running {
        return Some(MacosCodexHost::Standalone);
    }

    let chatgpt_running = process_list
        .lines()
        .any(|line| line.contains("/ChatGPT.app/Contents/MacOS/ChatGPT"));
    let chatgpt_codex_server_running = process_list.lines().any(|line| {
        line.contains("/ChatGPT.app/Contents/Resources/codex") && line.contains("app-server")
    });
    (chatgpt_running && chatgpt_codex_server_running).then_some(MacosCodexHost::ChatGpt)
}

#[cfg(target_os = "macos")]
fn macos_codex_version(host: MacosCodexHost) -> Option<String> {
    let app_path = macos_app_path(host)?;
    let embedded_codex = app_path.join("Contents/Resources/codex");
    if embedded_codex.exists() {
        if let Some(version) = command_output(&display_path(&embedded_codex), &["--version"]) {
            return Some(version);
        }
    }

    if host == MacosCodexHost::Standalone {
        let info_plist = app_path.join("Contents/Info.plist");
        return command_output(
            "/usr/libexec/PlistBuddy",
            &[
                "-c",
                "Print :CFBundleShortVersionString",
                &display_path(&info_plist),
            ],
        );
    }

    None
}

#[cfg(target_os = "windows")]
fn is_codex_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Codex.exe"])
        .output()
        .ok()
        .is_some_and(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("Codex.exe")
        })
}

fn restart_codex_desktop() -> Result<RestartCodexResult, String> {
    #[cfg(target_os = "macos")]
    {
        let process_list = macos_process_list().unwrap_or_default();
        let running_host = macos_codex_host_from_process_list(&process_list);
        let host = running_host
            .or_else(macos_installed_codex_host)
            .ok_or_else(|| "未在“应用程序”目录检测到 Codex 或 ChatGPT。".to_string())?;
        let app_path = macos_app_path(host)
            .ok_or_else(|| format!("未找到 {} 的应用程序文件。", host.label()))?;
        let was_running = running_host.is_some();
        if was_running {
            let quit_script = format!("tell application \"{}\" to quit", host.app_name());
            let quit = Command::new("osascript")
                .args(["-e", &quit_script])
                .status()
                .map_err(|err| format!("无法退出应用：{err}"))?;
            if !quit.success() {
                return Err("应用拒绝退出，自动流程已停止，请保存任务后重试。".into());
            }
            for _ in 0..20 {
                let host_still_running = macos_process_list()
                    .as_deref()
                    .and_then(macos_codex_host_from_process_list)
                    == Some(host);
                if !host_still_running {
                    break;
                }
                thread::sleep(Duration::from_millis(150));
            }
            if macos_process_list()
                .as_deref()
                .and_then(macos_codex_host_from_process_list)
                == Some(host)
            {
                return Err("应用仍在运行，未完成重启；请保存任务后重试。".into());
            }
        }

        let opened = Command::new("open")
            .arg(&app_path)
            .status()
            .map_err(|err| format!("无法重新打开 {}：{err}", host.label()))?;
        if !opened.success() {
            return Err("启动应用失败，配置已保留，请手动打开应用。".into());
        }
        Ok(RestartCodexResult {
            restarted: true,
            was_running,
            message: format!("{} 已重新打开。请新建任务以刷新工具列表。", host.label()),
        })
    }

    #[cfg(target_os = "windows")]
    {
        let executable = find_windows_codex_executable()
            .ok_or_else(|| "未找到 Codex.exe，请手动重启 Codex。".to_string())?;
        let was_running = is_codex_running();
        if was_running {
            let quit = Command::new("taskkill")
                .args(["/IM", "Codex.exe", "/T"])
                .status()
                .map_err(|err| format!("无法退出 Codex：{err}"))?;
            if !quit.success() {
                return Err("Codex 未能退出，自动流程已停止。".into());
            }
            thread::sleep(Duration::from_millis(500));
            if is_codex_running() {
                return Err("Codex 仍在运行，请保存任务后重试。".into());
            }
        }
        Command::new(&executable)
            .spawn()
            .map_err(|err| format!("无法重新打开 Codex：{err}"))?;
        Ok(RestartCodexResult {
            restarted: true,
            was_running,
            message: "Codex 已重新打开。请新建任务以刷新工具列表。".to_string(),
        })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Err("当前系统暂不支持自动重启 Codex。".to_string())
    }
}

#[cfg(target_os = "windows")]
fn find_windows_codex_executable() -> Option<PathBuf> {
    if let Some(output) = command_output("where", &["Codex.exe"]) {
        if let Some(path) = output.lines().next() {
            let path = PathBuf::from(path.trim());
            if path.exists() {
                return Some(path);
            }
        }
    }
    let local_app_data = env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    [
        local_app_data.join("Programs/Codex/Codex.exe"),
        local_app_data.join("Codex/Codex.exe"),
    ]
    .into_iter()
    .find(|path| path.exists())
}

fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| format!("无法访问系统剪贴板：{err}"))?;

    #[cfg(target_os = "windows")]
    let mut child = Command::new("clip")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| format!("无法访问系统剪贴板：{err}"))?;

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| format!("无法访问系统剪贴板：{err}"))?;

    child
        .stdin
        .take()
        .ok_or_else(|| "无法写入系统剪贴板。".to_string())?
        .write_all(text.as_bytes())
        .map_err(|err| format!("无法写入系统剪贴板：{err}"))?;
    let status = child
        .wait()
        .map_err(|err| format!("无法完成剪贴板操作：{err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("系统剪贴板操作失败。".to_string())
    }
}

fn extract_version_parts(value: &str) -> Option<Vec<u64>> {
    value
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .filter(|part| part.contains('.'))
        .find_map(|part| {
            let parts = part
                .trim_matches('.')
                .split('.')
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            (parts.len() >= 2).then_some(parts)
        })
}

fn version_at_least(value: &str, minimum: &str) -> bool {
    let Some(mut current) = extract_version_parts(value) else {
        return false;
    };
    let Some(mut required) = extract_version_parts(minimum) else {
        return false;
    };
    let length = current.len().max(required.len());
    current.resize(length, 0);
    required.resize(length, 0);
    current >= required
}

fn codex_home() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(path));
    }

    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .ok_or_else(|| "无法定位用户主目录，也没有设置 CODEX_HOME".to_string())?;
    Ok(PathBuf::from(home).join(".codex"))
}

fn toml_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn model_endpoints(base_url: &str) -> Vec<String> {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        return vec![format!("{trimmed}/models")];
    }

    vec![format!("{trimmed}/v1/models"), format!("{trimmed}/models")]
}

fn open_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(path);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer");
        command.arg(path);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .spawn()
        .map_err(|err| format!("无法打开目录 {}：{err}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("无法设置文件权限 {}：{err}", path.display()))
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let gui_only = args.len() == 1 && args[0] == "--gui";

    if !args.is_empty() && !gui_only {
        if let Err(err) = run_cli(&args) {
            eprintln!("Error: {err}");
            process::exit(1);
        }
        return;
    }

    run_gui();
}

fn run_gui() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(PendingUpdate(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            check_for_updates,
            copy_support_report,
            configure_imagegen_cli,
            configure_provider,
            get_account_access_status,
            get_config_status,
            get_history_migration_status,
            get_system_info,
            install_update,
            migrate_history_visibility,
            open_config_dir,
            repair_configuration,
            restart_codex,
            restore_defaults,
            run_diagnostics,
            test_connection,
            resize_window_to_content,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("failed to run OceanWay Codex config app");
}

fn run_cli(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }

    let options = parse_cli_args(args)?;
    validate_provider_id(&options.provider_id)?;
    validate_base_url(&options.base_url)?;

    let codex_home = codex_home()?;
    let config_path = codex_home.join("config.toml");
    let auth_path = codex_home.join("auth.json");
    let auth_strategy = choose_provider_auth_strategy(&auth_path);
    let model = options
        .model
        .or_else(|| read_current_model(&config_path))
        .unwrap_or_else(|| MODEL_FALLBACK.to_string());
    let original_config = fs::read_to_string(&config_path).unwrap_or_default();
    let dry_run_provider_token = if auth_strategy == ProviderAuthStrategy::ChatGptBearerToken {
        options.api_key.as_deref()
    } else {
        None
    };
    let mut rendered_config = merge_config(
        &original_config,
        &options.provider_id,
        &options.base_url,
        &model,
        dry_run_provider_token,
        auth_strategy,
    );
    if let Some(api_key) = options.api_key.as_deref() {
        rendered_config =
            merge_imagegen_cli_environment(&rendered_config, api_key, &options.base_url)?;
    }

    if options.dry_run {
        println!("--- {} ---", display_path(&config_path));
        print!("{rendered_config}");
        if let Some(api_key) = options.api_key {
            let original_auth = fs::read_to_string(&auth_path).unwrap_or_else(|_| "{}".to_string());
            let rendered_auth = render_auth_json_content(&original_auth, &api_key, auth_strategy)?;
            println!("--- {} ---", display_path(&auth_path));
            print!("{rendered_auth}");
        }
        return Ok(());
    }

    let Some(api_key) = options.api_key else {
        return Err("请提供 --api-key，或使用 --dry-run 只预览配置".to_string());
    };

    fs::create_dir_all(&codex_home).map_err(|err| format!("无法创建 Codex 目录：{err}"))?;
    let old_auth = if auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|err| format!("无法读取旧 auth.json：{err}"))?)
    } else {
        None
    };

    ensure_restore_snapshot(&codex_home, &config_path, &auth_path)?;
    let auth_backup_path = write_auth_json(&auth_path, &api_key, auth_strategy)?;
    let provider_token = if auth_strategy == ProviderAuthStrategy::ChatGptBearerToken {
        Some(api_key.as_str())
    } else {
        None
    };
    let config_result = write_config_toml(
        &config_path,
        &options.provider_id,
        &options.base_url,
        &model,
        provider_token,
        auth_strategy,
        Some(&api_key),
    );

    match config_result {
        Ok(config_backup_path) => {
            println!("Configured provider: {}", options.provider_id);
            println!("Configured imagegen CLI fallback environment.");
            println!("Config: {}", display_path(&config_path));
            println!("Auth: {}", display_path(&auth_path));
            if let Some(path) = config_backup_path {
                println!("Config backup: {}", display_path(&path));
            }
            if let Some(path) = auth_backup_path {
                println!("Auth backup: {}", display_path(&path));
            }
            Ok(())
        }
        Err(err) => {
            rollback_auth(&auth_path, old_auth);
            Err(err)
        }
    }
}

fn parse_cli_args(args: &[String]) -> Result<CliOptions, String> {
    let mut options = CliOptions {
        provider_id: PROVIDER_ID.to_string(),
        base_url: DEFAULT_BASE_URL.to_string(),
        ..CliOptions::default()
    };
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--dry-run" => options.dry_run = true,
            "--gui" => {}
            "--provider" | "--name" => {
                index += 1;
                require_value(args, index)?;
            }
            "--provider-id" => {
                index += 1;
                options.provider_id = require_value(args, index)?;
            }
            "--base-url" => {
                index += 1;
                options.base_url = require_value(args, index)?;
            }
            "--model" => {
                index += 1;
                options.model = Some(require_value(args, index)?);
            }
            "--api-key" => {
                index += 1;
                options.api_key = Some(require_value(args, index)?);
            }
            other => return Err(format!("未知参数：{other}")),
        }

        index += 1;
    }

    Ok(options)
}

fn require_value(args: &[String], index: usize) -> Result<String, String> {
    let Some(value) = args.get(index) else {
        return Err("参数缺少值".to_string());
    };

    if value.starts_with("--") {
        return Err(format!("参数缺少值：{value}"));
    }

    Ok(value.to_string())
}

fn validate_provider_id(provider_id: &str) -> Result<(), String> {
    if provider_id.is_empty() {
        return Err("provider id 不能为空".to_string());
    }

    if provider_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Ok(());
    }

    Err("provider id 只能包含英文字母、数字、下划线或短横线".to_string())
}

fn print_help() {
    println!(
        concat!(
            "codex-config\n\n",
            "Usage:\n",
            "  codex-config [--gui]\n",
            "  codex-config --dry-run --provider custom --provider-id OceanWay --name OceanWay --base-url URL --model MODEL --api-key KEY\n\n",
            "Options:\n",
            "  --gui              打开图形界面\n",
            "  --dry-run          只打印将写入的配置\n",
            "  --provider ID      兼容旧参数，当前不影响输出\n",
            "  --provider-id ID   provider id，默认 OceanWay\n",
            "  --name NAME        兼容旧参数，当前不影响输出\n",
            "  --base-url URL     Base URL\n",
            "  --model MODEL      Codex model，默认沿用旧配置或 gpt-5.4\n",
            "  --api-key KEY      写入认证配置，并同步 imagegen CLI 备用环境\n"
        )
    );
}

#[cfg(test)]
mod regression_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_dir(name: &str) -> PathBuf {
        let stamp = Local::now().timestamp_nanos_opt().unwrap_or_default();
        env::temp_dir().join(format!("oceanway-{name}-{stamp}"))
    }

    #[test]
    fn version_compatibility_handles_cli_and_desktop_labels() {
        assert!(version_at_least(
            "codex-cli 0.143.0",
            MINIMUM_IMAGE_EXTENSION_VERSION
        ));
        assert!(version_at_least("0.144.1-beta.2", "0.143.0"));
        assert!(!version_at_least("Codex 0.142.9", "0.143.0"));
        assert!(!version_at_least("unknown", "0.143.0"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_process_detection_recognizes_standalone_codex() {
        let processes = concat!(
            "/Applications/Codex.app/Contents/MacOS/Codex\n",
            "/Applications/Codex.app/Contents/Frameworks/Codex Framework.framework/",
            "Helpers/Codex (Renderer).app/Contents/MacOS/Codex (Renderer)\n",
        );

        assert_eq!(
            macos_codex_host_from_process_list(processes),
            Some(MacosCodexHost::Standalone)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_process_detection_recognizes_chatgpt_hosted_codex() {
        let processes = concat!(
            "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT\n",
            "/Applications/ChatGPT.app/Contents/Resources/codex ",
            "-c features.code_mode_host=true app-server --analytics-default-enabled\n",
        );

        assert_eq!(
            macos_codex_host_from_process_list(processes),
            Some(MacosCodexHost::ChatGpt)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_process_detection_ignores_helpers_and_chatgpt_without_codex_server() {
        let helper_only = concat!(
            "/Applications/Codex.app/Contents/Frameworks/Codex Framework.framework/",
            "Helpers/Codex (Renderer).app/Contents/MacOS/Codex (Renderer)\n",
            "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT\n",
        );

        assert_eq!(macos_codex_host_from_process_list(helper_only), None);
    }

    #[test]
    fn account_access_placeholder_never_claims_live_data() {
        let status = get_account_access_status();
        assert!(!status.integration_available);
        assert_eq!(status.status, "reserved");
        assert!(status.balance.is_none());
        assert!(status.last_checked_at.is_none());
        assert!(status
            .model_permissions
            .iter()
            .all(|permission| permission.status == "pending"));
    }

    #[test]
    fn merge_config_preserves_other_providers_and_replaces_oceanway() {
        let original = concat!(
            "model_provider = \"openrouter\"\n",
            "model = \"gpt-5.4\"\n",
            "model_reasoning_effort = \"medium\"\n",
            "\n",
            "[model_providers.openrouter]\n",
            "name = \"OpenRouter\"\n",
            "base_url = \"https://openrouter.ai/api/v1\"\n",
            "wire_api = \"responses\"\n",
            "requires_openai_auth = true\n",
            "\n",
            "[model_providers.OceanWay]\n",
            "name = \"OceanWay\"\n",
            "base_url = \"http://64.188.30.215:8080/v1\"\n",
            "wire_api = \"responses\"\n",
            "requires_openai_auth = true\n",
            "\n",
            "[model_providers.deepseek]\n",
            "name = \"DeepSeek\"\n",
            "base_url = \"https://api.deepseek.com/v1\"\n",
            "wire_api = \"chat\"\n",
            "requires_openai_auth = true\n",
        );

        let rendered = merge_config(
            original,
            PROVIDER_ID,
            DEFAULT_BASE_URL,
            MODEL_FALLBACK,
            None,
            ProviderAuthStrategy::ApiKey,
        );

        assert!(rendered.contains("[model_providers.openrouter]"));
        assert!(rendered.contains("[model_providers.deepseek]"));
        assert!(rendered.contains("base_url = \"https://ocean-way.top\""));
        assert!(!rendered.contains("http://64.188.30.215:8080/v1"));
        assert!(rendered.contains("model_provider = \"OceanWay\""));
        assert!(rendered.contains("model_reasoning_effort = \"high\""));
        assert!(rendered.contains("requires_openai_auth = false"));
        assert!(rendered.contains(concat!(
            "http_headers = { \"x-openai-actor-authorization\" = ",
            "\"local-image-extension\" }"
        )));
    }

    #[test]
    fn merge_config_can_store_provider_bearer_token_for_chatgpt_auth() {
        let rendered = merge_config(
            "",
            PROVIDER_ID,
            DEFAULT_BASE_URL,
            MODEL_FALLBACK,
            Some("ow-secret-key"),
            ProviderAuthStrategy::ChatGptBearerToken,
        );

        assert!(rendered.contains("model_provider = \"OceanWay\""));
        assert!(rendered.contains("experimental_bearer_token = \"ow-secret-key\""));
        assert!(rendered.contains("requires_openai_auth = true"));
        assert!(!rendered.contains("x-openai-actor-authorization"));
    }

    #[test]
    fn merge_config_enables_local_image_extension_for_api_key_auth() {
        let rendered = merge_config(
            "",
            PROVIDER_ID,
            DEFAULT_BASE_URL,
            MODEL_FALLBACK,
            None,
            ProviderAuthStrategy::ApiKey,
        );

        assert!(rendered.contains("requires_openai_auth = false"));
        assert!(rendered.contains(concat!(
            "http_headers = { \"x-openai-actor-authorization\" = ",
            "\"local-image-extension\" }"
        )));
        assert!(!rendered.contains("experimental_bearer_token"));
    }

    #[test]
    fn saved_provider_token_is_reused_when_api_key_input_is_empty() {
        let dir = unique_test_dir("reuse-provider-token");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("config.toml"),
            concat!(
                "[model_providers.OceanWay]\n",
                "base_url = \"https://ocean-way.top\"\n",
                "experimental_bearer_token = \"remembered-provider-key\"\n",
            ),
        )
        .unwrap();

        assert_eq!(
            resolve_api_key_in_home(&dir, "").unwrap(),
            "remembered-provider-key"
        );
        assert_eq!(
            resolve_api_key_in_home(&dir, "replacement-key").unwrap(),
            "replacement-key"
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saved_auth_key_is_reused_without_returning_it_in_status() {
        let dir = unique_test_dir("reuse-auth-key");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("auth.json"),
            r#"{"OPENAI_API_KEY":"remembered-auth-key"}"#,
        )
        .unwrap();

        assert_eq!(
            resolve_api_key_in_home(&dir, "  ").unwrap(),
            "remembered-auth-key"
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_api_key_requires_an_existing_saved_credential() {
        let dir = unique_test_dir("reuse-key-missing");
        fs::create_dir_all(&dir).unwrap();

        let err = resolve_api_key_in_home(&dir, "").unwrap_err();
        assert!(err.contains("尚未保存 API Key"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imagegen_cli_environment_is_added_and_matches_provider_credentials() {
        let rendered =
            merge_imagegen_cli_environment("", "ow-secret-key", "https://ocean-way.top/").unwrap();

        assert_eq!(
            read_shell_environment_string(&rendered, CODEX_AUTH_KEY).as_deref(),
            Some("ow-secret-key")
        );
        assert_eq!(
            read_shell_environment_string(&rendered, OPENAI_BASE_URL_ENV_KEY).as_deref(),
            Some("https://ocean-way.top")
        );
        assert!(has_matching_imagegen_cli_environment(
            &rendered,
            Some("ow-secret-key"),
            Some("https://ocean-way.top/")
        ));
    }

    #[test]
    fn imagegen_cli_environment_preserves_existing_inline_settings_and_updates_idempotently() {
        let original = concat!(
            "[shell_environment_policy]\n",
            "inherit = \"core\"\n",
            "set = { EXISTING_FLAG = \"keep\" }\n",
        );
        let first =
            merge_imagegen_cli_environment(original, "first-key", DEFAULT_BASE_URL).unwrap();
        let second =
            merge_imagegen_cli_environment(&first, "second-key", DEFAULT_BASE_URL).unwrap();

        assert_eq!(
            read_shell_environment_string(&second, "EXISTING_FLAG").as_deref(),
            Some("keep")
        );
        assert_eq!(
            read_shell_environment_string(&second, CODEX_AUTH_KEY).as_deref(),
            Some("second-key")
        );
        assert_eq!(second.matches("OPENAI_API_KEY").count(), 1);
        assert!(second.contains("inherit = \"core\""));
    }

    #[test]
    fn removing_imagegen_cli_environment_keeps_other_shell_policy_settings() {
        let configured = concat!(
            "[shell_environment_policy]\n",
            "inherit = \"core\"\n",
            "\n",
            "[shell_environment_policy.set]\n",
            "EXISTING_FLAG = \"keep\"\n",
            "OPENAI_API_KEY = \"ow-secret-key\"\n",
            "OPENAI_BASE_URL = \"https://ocean-way.top\"\n",
        );
        let rendered = remove_imagegen_cli_environment(configured).unwrap();

        assert_eq!(
            read_shell_environment_string(&rendered, "EXISTING_FLAG").as_deref(),
            Some("keep")
        );
        assert!(read_shell_environment_string(&rendered, CODEX_AUTH_KEY).is_none());
        assert!(read_shell_environment_string(&rendered, OPENAI_BASE_URL_ENV_KEY).is_none());
        assert!(rendered.contains("inherit = \"core\""));
    }

    #[test]
    fn imagegen_cli_status_detects_stale_key_or_base_url() {
        let rendered = merge_imagegen_cli_environment("", "current-key", DEFAULT_BASE_URL).unwrap();

        assert!(!has_matching_imagegen_cli_environment(
            &rendered,
            Some("old-key"),
            Some(DEFAULT_BASE_URL)
        ));
        assert!(!has_matching_imagegen_cli_environment(
            &rendered,
            Some("current-key"),
            Some("https://other.example")
        ));
    }

    #[test]
    fn second_click_imagegen_configuration_reuses_saved_provider_token() {
        let dir = unique_test_dir("imagegen-second-click");
        fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.toml");
        fs::write(
            &config_path,
            concat!(
                "model_provider = \"OceanWay\"\n",
                "\n",
                "[model_providers.OceanWay]\n",
                "name = \"OceanWay\"\n",
                "base_url = \"https://ocean-way.top\"\n",
                "wire_api = \"responses\"\n",
                "experimental_bearer_token = \"saved-provider-key\"\n",
                "requires_openai_auth = true\n",
            ),
        )
        .unwrap();

        let result = configure_imagegen_cli_in_home(&dir).unwrap();
        let rendered = fs::read_to_string(&config_path).unwrap();

        assert!(result.configured);
        assert!(has_matching_imagegen_cli_environment(
            &rendered,
            Some("saved-provider-key"),
            Some(DEFAULT_BASE_URL)
        ));
        assert!(dir.join(BACKUP_DIR_NAME).join("meta.json").exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn second_click_imagegen_configuration_requires_active_oceanway_provider() {
        let dir = unique_test_dir("imagegen-wrong-provider");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("config.toml"),
            concat!(
                "model_provider = \"openai\"\n",
                "\n",
                "[model_providers.OceanWay]\n",
                "base_url = \"https://ocean-way.top\"\n",
                "experimental_bearer_token = \"saved-provider-key\"\n",
            ),
        )
        .unwrap();

        let err = match configure_imagegen_cli_in_home(&dir) {
            Ok(_) => panic!("imagegen repair should require active OceanWay provider"),
            Err(err) => err,
        };
        assert!(err.contains("当前 provider 切换到 OceanWay"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remove_provider_config_only_removes_oceanway() {
        let original = concat!(
            "model_provider = \"OceanWay\"\n",
            "model = \"gpt-5.4\"\n",
            "disable_response_storage = true\n",
            "\n",
            "[model_providers.openrouter]\n",
            "name = \"OpenRouter\"\n",
            "base_url = \"https://openrouter.ai/api/v1\"\n",
            "\n",
            "[model_providers.OceanWay]\n",
            "name = \"OceanWay\"\n",
            "base_url = \"https://ocean-way.top\"\n",
        );

        let rendered = remove_provider_config(original, PROVIDER_ID);

        assert!(!rendered.contains("model_provider = \"OceanWay\""));
        assert!(rendered.contains("model = \"gpt-5.4\""));
        assert!(rendered.contains("disable_response_storage = true"));
        assert!(rendered.contains("[model_providers.openrouter]"));
        assert!(!rendered.contains("[model_providers.OceanWay]"));
        assert!(!rendered.contains("https://ocean-way.top"));
    }

    #[test]
    fn remove_provider_config_keeps_non_oceanway_active_provider() {
        let original = concat!(
            "model_provider = \"openrouter\"\n",
            "model = \"gpt-5.4\"\n",
            "\n",
            "[model_providers.OceanWay]\n",
            "base_url = \"https://ocean-way.top\"\n",
        );

        let rendered = remove_provider_config(original, PROVIDER_ID);

        assert!(rendered.contains("model_provider = \"openrouter\""));
        assert!(!rendered.contains("[model_providers.OceanWay]"));
    }

    #[test]
    fn snapshot_restore_returns_existing_files_to_initial_state() {
        let dir = unique_test_dir("restore-existing");
        fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.toml");
        let auth_path = dir.join("auth.json");
        fs::write(&config_path, "model_provider = \"openrouter\"\n").unwrap();
        fs::write(&auth_path, "{\n  \"OTHER_KEY\": \"keep\"\n}\n").unwrap();

        ensure_restore_snapshot(&dir, &config_path, &auth_path).unwrap();
        fs::write(&config_path, "model_provider = \"OceanWay\"\n").unwrap();
        fs::write(&auth_path, "{\n  \"OPENAI_API_KEY\": \"test\"\n}\n").unwrap();

        assert!(restore_from_snapshot(&dir, &config_path, &auth_path).unwrap());

        assert_eq!(
            fs::read_to_string(&config_path).unwrap(),
            "model_provider = \"openrouter\"\n"
        );
        assert_eq!(
            fs::read_to_string(&auth_path).unwrap(),
            "{\n  \"OTHER_KEY\": \"keep\"\n}\n"
        );
        assert!(!dir.join(BACKUP_DIR_NAME).exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn snapshot_restore_removes_files_that_did_not_originally_exist() {
        let dir = unique_test_dir("restore-missing");
        fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.toml");
        let auth_path = dir.join("auth.json");

        ensure_restore_snapshot(&dir, &config_path, &auth_path).unwrap();
        fs::write(&config_path, "model_provider = \"OceanWay\"\n").unwrap();
        fs::write(&auth_path, "{\n  \"OPENAI_API_KEY\": \"test\"\n}\n").unwrap();

        assert!(restore_from_snapshot(&dir, &config_path, &auth_path).unwrap());

        assert!(!config_path.exists());
        assert!(!auth_path.exists());
        assert!(!dir.join(BACKUP_DIR_NAME).exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_auth_json_preserves_existing_login_fields() {
        let dir = unique_test_dir("auth-preserve-login");
        fs::create_dir_all(&dir).unwrap();
        let auth_path = dir.join("auth.json");
        fs::write(
            &auth_path,
            r#"{
  "OPENAI_API_KEY": "old-key",
  "tokens": {
    "id_token": "logged-in-user"
  },
  "account_id": "acct-123"
}
"#,
        )
        .unwrap();

        write_auth_json(&auth_path, "new-oceanway-key", ProviderAuthStrategy::ApiKey).unwrap();

        let value =
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&auth_path).unwrap())
                .unwrap();
        assert_eq!(value["OPENAI_API_KEY"], "new-oceanway-key");
        assert_eq!(value["tokens"]["id_token"], "logged-in-user");
        assert_eq!(value["account_id"], "acct-123");

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn chatgpt_account_label_can_be_read_from_auth_json_fields() {
        let dir = unique_test_dir("auth-account-field");
        fs::create_dir_all(&dir).unwrap();
        let auth_path = dir.join("auth.json");
        fs::write(
            &auth_path,
            r#"{
  "auth_mode": "chatgpt",
  "account": {
    "email": "user@example.com"
  }
}
"#,
        )
        .unwrap();

        assert_eq!(
            read_auth_chatgpt_account_label(&auth_path).as_deref(),
            Some("user@example.com")
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn chatgpt_account_label_can_be_read_from_jwt_payload() {
        let dir = unique_test_dir("auth-account-jwt");
        fs::create_dir_all(&dir).unwrap();
        let auth_path = dir.join("auth.json");
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(r#"{"email":"jwt-user@example.com"}"#);
        let token = format!("{header}.{payload}.signature");
        fs::write(
            &auth_path,
            serde_json::to_string_pretty(&json!({
                "auth_mode": "chatgpt",
                "tokens": {
                    "id_token": token
                }
            }))
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            read_auth_chatgpt_account_label(&auth_path).as_deref(),
            Some("jwt-user@example.com")
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn chatgpt_auth_strategy_preserves_login_and_nulls_openai_api_key() {
        let dir = unique_test_dir("auth-chatgpt-token");
        fs::create_dir_all(&dir).unwrap();
        let auth_path = dir.join("auth.json");
        fs::write(
            &auth_path,
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "id_token": "logged-in-user"
  },
  "OPENAI_API_KEY": "old-key"
}
"#,
        )
        .unwrap();

        assert_eq!(
            choose_provider_auth_strategy(&auth_path).as_str(),
            "chatgptBearerToken"
        );
        write_auth_json(
            &auth_path,
            "new-oceanway-key",
            ProviderAuthStrategy::ChatGptBearerToken,
        )
        .unwrap();

        let value =
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&auth_path).unwrap())
                .unwrap();
        assert_eq!(value["auth_mode"], "chatgpt");
        assert_eq!(value["tokens"]["id_token"], "logged-in-user");
        assert!(value["OPENAI_API_KEY"].is_null());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn api_key_strategy_is_used_when_chatgpt_login_is_absent() {
        let dir = unique_test_dir("auth-no-chatgpt");
        fs::create_dir_all(&dir).unwrap();
        let auth_path = dir.join("auth.json");
        fs::write(&auth_path, "{\n  \"OTHER_KEY\": \"keep\"\n}\n").unwrap();

        assert_eq!(choose_provider_auth_strategy(&auth_path).as_str(), "apiKey");
        write_auth_json(&auth_path, "new-oceanway-key", ProviderAuthStrategy::ApiKey).unwrap();

        let value =
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&auth_path).unwrap())
                .unwrap();
        assert_eq!(value["OPENAI_API_KEY"], "new-oceanway-key");
        assert_eq!(value["OTHER_KEY"], "keep");
        assert!(value.get("auth_mode").is_none());

        fs::remove_dir_all(dir).unwrap();
    }

    fn write_history_rollout(path: &Path, provider: &str, thread_id: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!(
                "{}\n{}\n",
                json!({
                    "type": "session_meta",
                    "payload": {
                        "id": thread_id,
                        "model_provider": provider,
                        "cwd": "/tmp/project"
                    }
                }),
                json!({"type": "event_msg", "payload": {"type": "user_message"}})
            ),
        )
        .unwrap();
    }

    fn read_rollout_provider(path: &Path) -> String {
        let first_line = fs::read_to_string(path)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string();
        let value = serde_json::from_str::<Value>(&first_line).unwrap();
        value["payload"]["model_provider"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn create_history_state_db(path: &Path, rows: &[(&str, &str)]) {
        let db = Connection::open(path).unwrap();
        db.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL)",
            [],
        )
        .unwrap();
        for (thread_id, provider) in rows {
            db.execute(
                "INSERT INTO threads (id, model_provider) VALUES (?1, ?2)",
                params![thread_id, provider],
            )
            .unwrap();
        }
    }

    fn read_thread_provider(path: &Path, thread_id: &str) -> String {
        let db = Connection::open(path).unwrap();
        db.query_row(
            "SELECT model_provider FROM threads WHERE id = ?1",
            [thread_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    }

    #[test]
    fn history_migration_updates_rollout_sqlite_and_creates_backup() {
        let dir = unique_test_dir("history-migrate");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), "model_provider = \"OceanWay\"\n").unwrap();
        let rollout = dir.join("sessions/2026/05/14/rollout-old.jsonl");
        write_history_rollout(&rollout, "openai", "thread-1");
        create_history_state_db(
            &dir.join(CODEX_STATE_DB_NAME),
            &[("thread-1", "openai"), ("thread-2", "OceanWay")],
        );

        let status = history_migration_status_in_home(&dir).unwrap();
        assert!(status.needs_migration);
        assert_eq!(status.rollout_files_to_update, 1);
        assert_eq!(status.sqlite_rows_to_update, 1);

        let result = migrate_history_visibility_in_home(&dir).unwrap();

        assert_eq!(result.changed_session_files, 1);
        assert_eq!(result.sqlite_rows_updated, 1);
        assert_eq!(read_rollout_provider(&rollout), "OceanWay");
        assert_eq!(
            read_thread_provider(&dir.join(CODEX_STATE_DB_NAME), "thread-1"),
            "OceanWay"
        );
        assert!(PathBuf::from(result.backup_path.unwrap())
            .join("history-migration.json")
            .exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn history_migration_restore_only_reverts_manifest_entries() {
        let dir = unique_test_dir("history-restore");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), "model_provider = \"OceanWay\"\n").unwrap();
        let old_rollout = dir.join("sessions/rollout-old.jsonl");
        let new_rollout = dir.join("sessions/rollout-new.jsonl");
        write_history_rollout(&old_rollout, "openai", "thread-1");
        create_history_state_db(
            &dir.join(CODEX_STATE_DB_NAME),
            &[("thread-1", "openai"), ("thread-2", "OceanWay")],
        );

        migrate_history_visibility_in_home(&dir).unwrap();
        write_history_rollout(&new_rollout, "OceanWay", "thread-2");
        let restored = restore_history_migrations(&dir).unwrap();

        assert_eq!(restored.restored_backups, 1);
        assert_eq!(read_rollout_provider(&old_rollout), "openai");
        assert_eq!(read_rollout_provider(&new_rollout), "OceanWay");
        assert_eq!(
            read_thread_provider(&dir.join(CODEX_STATE_DB_NAME), "thread-1"),
            "openai"
        );
        assert_eq!(
            read_thread_provider(&dir.join(CODEX_STATE_DB_NAME), "thread-2"),
            "OceanWay"
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn history_migration_can_run_again_after_restore() {
        let dir = unique_test_dir("history-remigrate");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), "model_provider = \"OceanWay\"\n").unwrap();
        let rollout = dir.join("sessions/rollout-old.jsonl");
        write_history_rollout(&rollout, "openai", "thread-1");
        create_history_state_db(&dir.join(CODEX_STATE_DB_NAME), &[("thread-1", "openai")]);

        let first_migration = migrate_history_visibility_in_home(&dir).unwrap();
        assert_eq!(first_migration.changed_session_files, 1);
        assert_eq!(read_rollout_provider(&rollout), "OceanWay");

        let first_restore = restore_history_migrations(&dir).unwrap();
        assert_eq!(first_restore.restored_backups, 1);
        assert_eq!(read_rollout_provider(&rollout), "openai");
        assert_eq!(
            read_thread_provider(&dir.join(CODEX_STATE_DB_NAME), "thread-1"),
            "openai"
        );

        fs::write(dir.join("config.toml"), "model_provider = \"OceanWay\"\n").unwrap();
        let second_migration = migrate_history_visibility_in_home(&dir).unwrap();
        assert_eq!(second_migration.changed_session_files, 1);
        assert_eq!(read_rollout_provider(&rollout), "OceanWay");
        assert_eq!(
            read_thread_provider(&dir.join(CODEX_STATE_DB_NAME), "thread-1"),
            "OceanWay"
        );

        let second_restore = restore_history_migrations(&dir).unwrap();
        assert_eq!(second_restore.restored_backups, 1);
        assert_eq!(read_rollout_provider(&rollout), "openai");
        assert_eq!(
            read_thread_provider(&dir.join(CODEX_STATE_DB_NAME), "thread-1"),
            "openai"
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn history_migration_is_only_supported_for_oceanway_target() {
        let dir = unique_test_dir("history-openai-target");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), "model_provider = \"openai\"\n").unwrap();
        let rollout = dir.join("sessions/rollout-oceanway.jsonl");
        write_history_rollout(&rollout, "OceanWay", "thread-1");
        create_history_state_db(&dir.join(CODEX_STATE_DB_NAME), &[("thread-1", "OceanWay")]);

        let status = history_migration_status_in_home(&dir).unwrap();
        assert!(!status.migration_supported);
        assert!(!status.needs_migration);
        assert_eq!(status.rollout_files_to_update, 0);
        assert_eq!(status.sqlite_rows_to_update, 0);

        let err = match migrate_history_visibility_in_home(&dir) {
            Ok(_) => panic!("migration should not be supported for openai target"),
            Err(err) => err,
        };
        assert!(err.contains("仅支持配置到 OceanWay 后使用"));
        assert_eq!(read_rollout_provider(&rollout), "OceanWay");
        assert_eq!(
            read_thread_provider(&dir.join(CODEX_STATE_DB_NAME), "thread-1"),
            "OceanWay"
        );

        fs::remove_dir_all(dir).unwrap();
    }
}
