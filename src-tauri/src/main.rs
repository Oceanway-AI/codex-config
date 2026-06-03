#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Local;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::time::Duration;
use tauri::{LogicalSize, Manager, Size};

const PROVIDER_ID: &str = "OceanWay";
const DEFAULT_BASE_URL: &str = "https://ocean-way.top";
const MODEL_FALLBACK: &str = "gpt-5.4";
const CODEX_AUTH_KEY: &str = "OPENAI_API_KEY";
const BACKUP_DIR_NAME: &str = "oceanway-ai-backup";
const HISTORY_MIGRATION_BACKUP_DIR_NAME: &str = "oceanway-history-migration-backup";
const CODEX_STATE_DB_NAME: &str = "state_5.sqlite";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationResult {
    config_path: String,
    auth_path: String,
    config_backup_path: Option<String>,
    auth_backup_path: Option<String>,
    auth_strategy: String,
    history_migration_restore: Option<HistoryMigrationRestoreResult>,
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
    let auth_strategy = if provider_token.is_some() {
        ProviderAuthStrategy::ChatGptBearerToken
    } else {
        ProviderAuthStrategy::ApiKey
    };
    let configured = base_url.as_deref() == Some(DEFAULT_BASE_URL);

    Ok(ConfigStatus {
        configured,
        provider_id,
        base_url,
        model,
        has_api_key,
        auth_strategy: auth_strategy.as_str().to_string(),
        chatgpt_login_detected,
        config_path: display_path(&config_path),
        auth_path: display_path(&auth_path),
    })
}

#[tauri::command]
fn test_connection(api_key: String, base_url: String) -> Result<ConnectionTestResult, String> {
    let api_key = api_key.trim();
    let base_url = base_url.trim();

    if api_key.is_empty() {
        return Err("请先输入 API Key".to_string());
    }

    let base_url = if base_url.is_empty() {
        DEFAULT_BASE_URL
    } else {
        base_url
    };
    let endpoints = model_endpoints(base_url);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|err| format!("无法创建测试客户端：{err}"))?;

    let mut last_error = String::new();
    for endpoint in endpoints {
        match client.get(&endpoint).bearer_auth(api_key).send() {
            Ok(response) if response.status().is_success() => {
                return Ok(ConnectionTestResult {
                    ok: true,
                    message: "连接成功，API Key 和 Base URL 可用。".to_string(),
                    endpoint,
                });
            }
            Ok(response) => {
                let status = response.status();
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return Ok(ConnectionTestResult {
                        ok: false,
                        message: format!("连接到服务，但 API Key 无效或无权限。HTTP {status}"),
                        endpoint,
                    });
                }
                last_error = format!("HTTP {status}");
            }
            Err(err) => {
                last_error = err.to_string();
            }
        }
    }

    Ok(ConnectionTestResult {
        ok: false,
        message: format!("连接失败：{last_error}"),
        endpoint: base_url.to_string(),
    })
}

#[tauri::command]
fn open_config_dir() -> Result<(), String> {
    let codex_home = codex_home()?;
    fs::create_dir_all(&codex_home).map_err(|err| format!("无法创建 Codex 目录：{err}"))?;
    open_path(&codex_home)
}

#[tauri::command]
fn get_history_migration_status() -> Result<HistoryMigrationStatus, String> {
    let codex_home = codex_home()?;
    history_migration_status_in_home(&codex_home)
}

#[tauri::command]
fn migrate_history_visibility() -> Result<HistoryMigrationResult, String> {
    let codex_home = codex_home()?;
    migrate_history_visibility_in_home(&codex_home)
}

#[tauri::command]
fn configure_provider(api_key: String, base_url: String) -> Result<OperationResult, String> {
    configure_provider_internal(api_key, base_url)
}

fn configure_provider_internal(
    api_key: String,
    base_url: String,
) -> Result<OperationResult, String> {
    let api_key = api_key.trim().to_string();
    let base_url = base_url.trim();

    if api_key.is_empty() {
        return Err("API Key 不能为空".to_string());
    }

    let base_url = if base_url.is_empty() {
        DEFAULT_BASE_URL
    } else {
        base_url
    };

    let codex_home = codex_home()?;
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
    let config_result =
        write_config_toml(&config_path, PROVIDER_ID, base_url, &model, provider_token);

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
        history_migration_restore: None,
    })
}

#[tauri::command]
fn restore_defaults() -> Result<OperationResult, String> {
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
    let width = (size.width as f64 / scale_factor).clamp(760.0, 1100.0);
    let height = height.clamp(440.0, 760.0);
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
) -> Result<Option<PathBuf>, String> {
    let codex_home = config_path
        .parent()
        .ok_or_else(|| format!("无法定位 Codex 目录：{}", config_path.display()))?;
    let auth_path = codex_home.join("auth.json");
    ensure_restore_snapshot(codex_home, config_path, &auth_path)?;

    let backup_path = backup_file(config_path)?;
    let original = fs::read_to_string(config_path).unwrap_or_default();
    let rendered = merge_config(&original, provider_id, base_url, model, bearer_token);

    fs::write(config_path, rendered).map_err(|err| format!("无法写入 config.toml：{err}"))?;
    set_private_permissions(config_path)?;
    Ok(backup_path)
}

fn write_auth_json(
    auth_path: &Path,
    api_key: &str,
    strategy: ProviderAuthStrategy,
) -> Result<Option<PathBuf>, String> {
    let backup_path = backup_file(auth_path)?;
    let content = fs::read_to_string(auth_path).unwrap_or_else(|_| "{}".to_string());
    let rendered = render_auth_json_content(&content, api_key, strategy)?;

    fs::write(auth_path, rendered).map_err(|err| format!("无法写入 auth.json：{err}"))?;
    set_private_permissions(auth_path)?;
    Ok(backup_path)
}

fn render_auth_json_content(
    content: &str,
    api_key: &str,
    strategy: ProviderAuthStrategy,
) -> Result<String, String> {
    let mut value =
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| json!({}));

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
        return Ok(());
    }

    fs::create_dir_all(&snapshot_dir)
        .map_err(|err| format!("无法创建 OceanWay 备份目录：{err}"))?;

    let meta = RestoreSnapshotMeta {
        config_existed: config_path.exists(),
        auth_existed: auth_path.exists(),
        created_at: Local::now().to_rfc3339(),
    };

    if meta.config_existed {
        fs::copy(config_path, snapshot_dir.join("config.toml"))
            .map_err(|err| format!("无法保存 config.toml 初始快照：{err}"))?;
    }
    if meta.auth_existed {
        fs::copy(auth_path, snapshot_dir.join("auth.json"))
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

    rendered.push_str(&render_provider_block(provider_id, base_url, bearer_token));
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

    if !rendered.trim().is_empty() && !rest.trim().is_empty() {
        if !rendered.ends_with("\n\n") {
            if !rendered.ends_with('\n') {
                rendered.push('\n');
            }
            rendered.push('\n');
        }
    }

    if !rest.trim().is_empty() {
        rendered.push_str(rest.trim_end());
        rendered.push('\n');
    }

    rendered
}

fn render_provider_block(provider_id: &str, base_url: &str, bearer_token: Option<&str>) -> String {
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
    if let Some(token) = bearer_token.filter(|token| !token.trim().is_empty()) {
        rendered.push_str(&format!(
            "experimental_bearer_token = {}\n",
            toml_string(token.trim())
        ));
    }
    rendered.push_str("requires_openai_auth = true\n");
    rendered
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

        if !removed && !trimmed.starts_with('#') && current_key.trim() == key {
            if parse_quoted_toml_string(value.trim()).as_deref() == Some(expected_value) {
                removed = true;
                continue;
            }
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
            return parse_quoted_toml_string(value.trim());
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
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let backup_path = path.with_file_name(format!("{file_name}.bak.{stamp}"));

    fs::copy(path, &backup_path).map_err(|err| format!("无法备份 {}：{err}", path.display()))?;
    Ok(Some(backup_path))
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
        .invoke_handler(tauri::generate_handler![
            configure_provider,
            get_config_status,
            get_history_migration_status,
            migrate_history_visibility,
            open_config_dir,
            restore_defaults,
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
    let rendered_config = merge_config(
        &original_config,
        &options.provider_id,
        &options.base_url,
        &model,
        dry_run_provider_token,
    );

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
    );

    match config_result {
        Ok(config_backup_path) => {
            println!("Configured provider: {}", options.provider_id);
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
            "  --api-key KEY      写入 auth.json 的 API Key\n"
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_dir(name: &str) -> PathBuf {
        let stamp = Local::now().timestamp_nanos_opt().unwrap_or_default();
        env::temp_dir().join(format!("oceanway-{name}-{stamp}"))
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
        );

        assert!(rendered.contains("[model_providers.openrouter]"));
        assert!(rendered.contains("[model_providers.deepseek]"));
        assert!(rendered.contains("base_url = \"https://ocean-way.top\""));
        assert!(!rendered.contains("http://64.188.30.215:8080/v1"));
        assert!(rendered.contains("model_provider = \"OceanWay\""));
        assert!(rendered.contains("model_reasoning_effort = \"high\""));
    }

    #[test]
    fn merge_config_can_store_provider_bearer_token_for_chatgpt_auth() {
        let rendered = merge_config(
            "",
            PROVIDER_ID,
            DEFAULT_BASE_URL,
            MODEL_FALLBACK,
            Some("ow-secret-key"),
        );

        assert!(rendered.contains("model_provider = \"OceanWay\""));
        assert!(rendered.contains("experimental_bearer_token = \"ow-secret-key\""));
        assert!(rendered.contains("requires_openai_auth = true"));
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
