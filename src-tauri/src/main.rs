use chrono::Local;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;
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
const SESSION_SYNC_DIR_NAME: &str = "session-provider-sync";
const CODEX_STATE_DB_NAME: &str = "state_5.sqlite";
const KEY_PROFILES_FILE_NAME: &str = "oceanway-ai-keys.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OperationResult {
    config_path: String,
    auth_path: String,
    config_backup_path: Option<String>,
    auth_backup_path: Option<String>,
    session_sync: SessionSyncResult,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSyncResult {
    rollout_files_updated: usize,
    sqlite_rows_updated: usize,
    skipped_files: usize,
    backup_path: Option<String>,
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

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct KeyProfile {
    id: String,
    name: String,
    api_key: String,
    #[serde(default = "default_base_url_string")]
    base_url: String,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct KeyProfileStore {
    profiles: Vec<KeyProfile>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyProfileSummary {
    id: String,
    name: String,
    masked_key: String,
    base_url: String,
    active: bool,
    created_at: String,
    updated_at: String,
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
    let has_api_key = read_auth_has_api_key(&auth_path);
    let configured = base_url.as_deref() == Some(DEFAULT_BASE_URL);

    Ok(ConfigStatus {
        configured,
        provider_id,
        base_url,
        model,
        has_api_key,
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
fn configure_provider(api_key: String, base_url: String) -> Result<OperationResult, String> {
    configure_provider_internal(api_key, base_url)
}

#[tauri::command]
fn configure_with_key_profile(profile_id: String) -> Result<OperationResult, String> {
    let codex_home = codex_home()?;
    let profile = find_key_profile(&codex_home, &profile_id)?
        .ok_or_else(|| "未找到选中的密钥档案".to_string())?;
    configure_provider_internal(profile.api_key, profile.base_url)
}

#[tauri::command]
fn test_key_profile(profile_id: String) -> Result<ConnectionTestResult, String> {
    let codex_home = codex_home()?;
    let profile = find_key_profile(&codex_home, &profile_id)?
        .ok_or_else(|| "未找到选中的密钥档案".to_string())?;
    test_connection(profile.api_key, profile.base_url)
}

#[tauri::command]
fn list_key_profiles() -> Result<Vec<KeyProfileSummary>, String> {
    let codex_home = codex_home()?;
    list_key_profiles_in_home(&codex_home)
}

#[tauri::command]
fn save_key_profile(
    profile_id: Option<String>,
    name: String,
    api_key: String,
    base_url: String,
) -> Result<KeyProfileSummary, String> {
    let codex_home = codex_home()?;
    save_key_profile_in_home(&codex_home, profile_id, name, api_key, base_url)
}

#[tauri::command]
fn delete_key_profile(profile_id: String) -> Result<(), String> {
    let codex_home = codex_home()?;
    delete_key_profile_in_home(&codex_home, &profile_id)
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

    let old_auth = if auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|err| format!("无法读取旧 auth.json：{err}"))?)
    } else {
        None
    };

    let auth_backup_path = write_auth_json(&auth_path, &api_key)?;
    let model = read_current_model(&config_path).unwrap_or_else(|| MODEL_FALLBACK.to_string());
    let config_result = write_config_toml(&config_path, PROVIDER_ID, base_url, &model);

    let config_backup_path = match config_result {
        Ok(path) => path,
        Err(err) => {
            rollback_auth(&auth_path, old_auth);
            return Err(err);
        }
    };
    let session_sync = sync_codex_session_provider_lossy(&codex_home, PROVIDER_ID);

    Ok(OperationResult {
        config_path: display_path(&config_path),
        auth_path: display_path(&auth_path),
        config_backup_path: config_backup_path.as_ref().map(|path| display_path(path)),
        auth_backup_path: auth_backup_path.as_ref().map(|path| display_path(path)),
        session_sync,
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
    let session_sync = sync_restored_session_provider_lossy(&codex_home, &config_path);

    Ok(OperationResult {
        config_path: display_path(&config_path),
        auth_path: display_path(&auth_path),
        config_backup_path: config_backup_path.as_ref().map(|path| display_path(path)),
        auth_backup_path: auth_backup_path.as_ref().map(|path| display_path(path)),
        session_sync,
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
) -> Result<Option<PathBuf>, String> {
    let codex_home = config_path
        .parent()
        .ok_or_else(|| format!("无法定位 Codex 目录：{}", config_path.display()))?;
    let auth_path = codex_home.join("auth.json");
    ensure_restore_snapshot(codex_home, config_path, &auth_path)?;

    let backup_path = backup_file(config_path)?;
    let original = fs::read_to_string(config_path).unwrap_or_default();
    let rendered = merge_config(&original, provider_id, base_url, model);

    fs::write(config_path, rendered).map_err(|err| format!("无法写入 config.toml：{err}"))?;
    set_private_permissions(config_path)?;
    Ok(backup_path)
}

fn write_auth_json(auth_path: &Path, api_key: &str) -> Result<Option<PathBuf>, String> {
    let backup_path = backup_file(auth_path)?;
    let rendered = serde_json::to_string_pretty(&json!({ CODEX_AUTH_KEY: api_key }))
        .map_err(|err| format!("无法生成 auth.json：{err}"))?
        + "\n";

    fs::write(auth_path, rendered).map_err(|err| format!("无法写入 auth.json：{err}"))?;
    set_private_permissions(auth_path)?;
    Ok(backup_path)
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

fn list_key_profiles_in_home(codex_home: &Path) -> Result<Vec<KeyProfileSummary>, String> {
    let mut store = read_key_profile_store(codex_home)?;
    let active_state = read_active_oceanway_state(codex_home);
    store.profiles.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(store
        .profiles
        .iter()
        .map(|profile| key_profile_summary(profile, active_state.as_ref()))
        .collect())
}

fn save_key_profile_in_home(
    codex_home: &Path,
    profile_id: Option<String>,
    name: String,
    api_key: String,
    base_url: String,
) -> Result<KeyProfileSummary, String> {
    let name = name.trim();
    let api_key = api_key.trim();
    let base_url = normalize_profile_base_url(&base_url);
    if name.is_empty() {
        return Err("密钥名称不能为空".to_string());
    }
    if api_key.is_empty() && profile_id.as_deref().unwrap_or_default().is_empty() {
        return Err("API Key 不能为空".to_string());
    }

    let mut store = read_key_profile_store(codex_home)?;
    let now = Local::now().to_rfc3339();
    let existing_index = profile_id
        .as_deref()
        .and_then(|id| store.profiles.iter().position(|profile| profile.id == id))
        .or_else(|| {
            store
                .profiles
                .iter()
                .position(|profile| profile.name.eq_ignore_ascii_case(name))
        });

    let profile = if let Some(index) = existing_index {
        let profile = &mut store.profiles[index];
        if api_key.is_empty() {
            if profile.api_key.trim().is_empty() {
                return Err("API Key 不能为空".to_string());
            }
        } else {
            profile.api_key = api_key.to_string();
        }
        profile.name = name.to_string();
        profile.base_url = base_url;
        profile.updated_at = now;
        profile.clone()
    } else {
        let profile = KeyProfile {
            id: new_key_profile_id(),
            name: name.to_string(),
            api_key: api_key.to_string(),
            base_url,
            created_at: now.clone(),
            updated_at: now,
        };
        store.profiles.push(profile.clone());
        profile
    };

    write_key_profile_store(codex_home, &store)?;
    let active_state = read_active_oceanway_state(codex_home);
    Ok(key_profile_summary(&profile, active_state.as_ref()))
}

fn delete_key_profile_in_home(codex_home: &Path, profile_id: &str) -> Result<(), String> {
    let mut store = read_key_profile_store(codex_home)?;
    let original_len = store.profiles.len();
    store.profiles.retain(|profile| profile.id != profile_id);
    if store.profiles.len() == original_len {
        return Err("未找到选中的密钥档案".to_string());
    }
    write_key_profile_store(codex_home, &store)
}

fn find_key_profile(codex_home: &Path, profile_id: &str) -> Result<Option<KeyProfile>, String> {
    let store = read_key_profile_store(codex_home)?;
    Ok(store
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id))
}

fn read_key_profile_store(codex_home: &Path) -> Result<KeyProfileStore, String> {
    let path = codex_home.join(KEY_PROFILES_FILE_NAME);
    if !path.exists() {
        return Ok(KeyProfileStore::default());
    }

    let content = fs::read_to_string(&path)
        .map_err(|err| format!("无法读取密钥档案 {}：{err}", path.display()))?;
    serde_json::from_str::<KeyProfileStore>(&content)
        .map_err(|err| format!("密钥档案格式无效：{err}"))
}

fn write_key_profile_store(codex_home: &Path, store: &KeyProfileStore) -> Result<(), String> {
    fs::create_dir_all(codex_home).map_err(|err| format!("无法创建 Codex 目录：{err}"))?;
    let path = codex_home.join(KEY_PROFILES_FILE_NAME);
    let rendered = serde_json::to_string_pretty(store)
        .map_err(|err| format!("无法生成密钥档案：{err}"))?
        + "\n";
    fs::write(&path, rendered).map_err(|err| format!("无法写入密钥档案：{err}"))?;
    set_private_permissions(&path)
}

fn key_profile_summary(
    profile: &KeyProfile,
    active_state: Option<&ActiveProviderState>,
) -> KeyProfileSummary {
    KeyProfileSummary {
        id: profile.id.clone(),
        name: profile.name.clone(),
        masked_key: mask_api_key(&profile.api_key),
        base_url: profile.base_url.clone(),
        active: active_state.is_some_and(|state| {
            state.api_key == profile.api_key
                && normalize_base_url_for_compare(&state.base_url)
                    == normalize_base_url_for_compare(&profile.base_url)
        }),
        created_at: profile.created_at.clone(),
        updated_at: profile.updated_at.clone(),
    }
}

struct ActiveProviderState {
    api_key: String,
    base_url: String,
}

fn read_active_oceanway_state(codex_home: &Path) -> Option<ActiveProviderState> {
    let config_path = codex_home.join("config.toml");
    let config = fs::read_to_string(config_path).ok()?;
    if read_root_string(&config, "model_provider").as_deref() != Some(PROVIDER_ID) {
        return None;
    }

    Some(ActiveProviderState {
        api_key: read_auth_api_key(&codex_home.join("auth.json"))?,
        base_url: read_provider_base_url(&config, PROVIDER_ID)?,
    })
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

fn mask_api_key(api_key: &str) -> String {
    let trimmed = api_key.trim();
    if trimmed.len() <= 10 {
        return "已保存".to_string();
    }

    let prefix = &trimmed[..6.min(trimmed.len())];
    let suffix_start = trimmed.len().saturating_sub(4);
    format!("{prefix}...{}", &trimmed[suffix_start..])
}

fn new_key_profile_id() -> String {
    let stamp = Local::now().timestamp_nanos_opt().unwrap_or_default();
    format!("key-{stamp}")
}

fn normalize_profile_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        DEFAULT_BASE_URL.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_base_url_for_compare(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn default_base_url_string() -> String {
    DEFAULT_BASE_URL.to_string()
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

fn sync_codex_session_provider(
    codex_home: &Path,
    provider_id: &str,
) -> Result<SessionSyncResult, String> {
    let snapshot_dir = codex_home.join(BACKUP_DIR_NAME);
    fs::create_dir_all(&snapshot_dir)
        .map_err(|err| format!("无法创建 OceanWay 备份目录：{err}"))?;

    let sync_backup_dir = snapshot_dir.join(SESSION_SYNC_DIR_NAME);
    fs::create_dir_all(&sync_backup_dir).map_err(|err| format!("无法创建会话备份目录：{err}"))?;

    let mut result = SessionSyncResult {
        backup_path: Some(display_path(&sync_backup_dir)),
        ..SessionSyncResult::default()
    };

    sync_codex_session_provider_inner(codex_home, Some(&sync_backup_dir), provider_id, &mut result);
    Ok(result)
}

fn sync_codex_session_provider_without_backup(
    codex_home: &Path,
    provider_id: &str,
) -> SessionSyncResult {
    let mut result = SessionSyncResult::default();
    sync_codex_session_provider_inner(codex_home, None, provider_id, &mut result);
    result
}

fn sync_codex_session_provider_inner(
    codex_home: &Path,
    sync_backup_dir: Option<&Path>,
    provider_id: &str,
    result: &mut SessionSyncResult,
) {
    let rollout_dirs = [
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ];
    for dir in rollout_dirs {
        if let Err(err) =
            sync_rollout_dir_provider(codex_home, sync_backup_dir, &dir, provider_id, result)
        {
            result
                .warnings
                .push(format!("同步 {} 失败：{err}", display_path(&dir)));
        }
    }

    if let Err(err) = sync_state_db_provider(codex_home, sync_backup_dir, provider_id, result) {
        result.warnings.push(format!("同步状态数据库失败：{err}"));
    }
}

fn sync_codex_session_provider_lossy(codex_home: &Path, provider_id: &str) -> SessionSyncResult {
    match sync_codex_session_provider(codex_home, provider_id) {
        Ok(result) => result,
        Err(err) => SessionSyncResult {
            warnings: vec![format!("历史记录同步未完成：{err}")],
            ..SessionSyncResult::default()
        },
    }
}

fn sync_restored_session_provider_lossy(
    codex_home: &Path,
    config_path: &Path,
) -> SessionSyncResult {
    let provider_id = read_session_provider_from_config(config_path);
    sync_codex_session_provider_without_backup(codex_home, &provider_id)
}

fn read_session_provider_from_config(config_path: &Path) -> String {
    let content = fs::read_to_string(config_path).unwrap_or_default();
    read_root_string(&content, "model_provider").unwrap_or_else(|| "openai".to_string())
}

fn sync_rollout_dir_provider(
    codex_home: &Path,
    sync_backup_dir: Option<&Path>,
    dir: &Path,
    provider_id: &str,
    result: &mut SessionSyncResult,
) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    for path in jsonl_files(dir)? {
        match sync_rollout_file_provider(codex_home, sync_backup_dir, &path, provider_id) {
            Ok(true) => result.rollout_files_updated += 1,
            Ok(false) => {}
            Err(err) => {
                result.skipped_files += 1;
                result
                    .warnings
                    .push(format!("跳过 {}：{err}", display_path(&path)));
            }
        }
    }

    Ok(())
}

fn jsonl_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut output = Vec::new();
    collect_jsonl_files(dir, &mut output)?;
    Ok(output)
}

fn collect_jsonl_files(dir: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|err| format!("无法读取目录 {}：{err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("无法读取目录项 {}：{err}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("无法读取文件类型 {}：{err}", path.display()))?;
        if file_type.is_dir() {
            collect_jsonl_files(&path, output)?;
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            output.push(path);
        }
    }
    Ok(())
}

fn sync_rollout_file_provider(
    codex_home: &Path,
    sync_backup_dir: Option<&Path>,
    path: &Path,
    provider_id: &str,
) -> Result<bool, String> {
    let original = fs::read_to_string(path).map_err(|err| format!("无法读取会话文件：{err}"))?;
    let mut changed = false;
    let mut output = Vec::new();

    for line in original.lines() {
        if line.trim().is_empty() {
            output.push(String::new());
            continue;
        }

        let mut value = serde_json::from_str::<serde_json::Value>(line)
            .map_err(|err| format!("会话 JSON 无效：{err}"))?;
        if value.get("type").and_then(|value| value.as_str()) == Some("session_meta") {
            if let Some(payload) = value
                .get_mut("payload")
                .and_then(|value| value.as_object_mut())
            {
                let needs_update = payload
                    .get("model_provider")
                    .and_then(|value| value.as_str())
                    .is_some_and(|current| current != provider_id);
                if needs_update {
                    payload.insert("model_provider".to_string(), json!(provider_id));
                    changed = true;
                }
            }
        }

        output.push(
            serde_json::to_string(&value).map_err(|err| format!("无法生成会话 JSON：{err}"))?,
        );
    }

    if !changed {
        return Ok(false);
    }

    if let Some(sync_backup_dir) = sync_backup_dir {
        backup_session_file(codex_home, sync_backup_dir, path)?;
    }
    let mut rendered = output.join("\n");
    rendered.push('\n');
    fs::write(path, rendered).map_err(|err| format!("无法写入会话文件：{err}"))?;
    Ok(true)
}

fn backup_session_file(
    codex_home: &Path,
    sync_backup_dir: &Path,
    path: &Path,
) -> Result<(), String> {
    let relative = path.strip_prefix(codex_home).unwrap_or(path);
    let backup_path = sync_backup_dir.join("files").join(relative);
    if backup_path.exists() {
        return Ok(());
    }
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("无法创建会话文件备份目录：{err}"))?;
    }
    fs::copy(path, &backup_path).map_err(|err| format!("无法备份会话文件：{err}"))?;
    Ok(())
}

fn sync_state_db_provider(
    codex_home: &Path,
    sync_backup_dir: Option<&Path>,
    provider_id: &str,
    result: &mut SessionSyncResult,
) -> Result<(), String> {
    let db_path = codex_home.join(CODEX_STATE_DB_NAME);
    if !db_path.exists() {
        return Ok(());
    }

    let connection = Connection::open(&db_path)
        .map_err(|err| format!("无法打开 Codex 状态数据库 {}：{err}", db_path.display()))?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|err| format!("无法设置数据库等待时间：{err}"))?;

    let table_exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'threads'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| format!("无法检查 threads 表：{err}"))?;
    if table_exists == 0 {
        return Ok(());
    }

    let rows_to_update: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM threads WHERE model_provider <> ?1",
            params![provider_id],
            |row| row.get(0),
        )
        .map_err(|err| format!("无法统计会话数据库 provider：{err}"))?;
    if rows_to_update == 0 {
        return Ok(());
    }

    if let Some(sync_backup_dir) = sync_backup_dir {
        backup_state_db(&connection, &db_path, sync_backup_dir)?;
    }
    result.sqlite_rows_updated = connection
        .execute(
            "UPDATE threads SET model_provider = ?1 WHERE model_provider <> ?1",
            params![provider_id],
        )
        .map_err(|err| format!("无法同步会话数据库 provider：{err}"))?;
    Ok(())
}

fn backup_state_db(
    connection: &Connection,
    db_path: &Path,
    sync_backup_dir: &Path,
) -> Result<(), String> {
    let backup_path = sync_backup_dir.join(CODEX_STATE_DB_NAME);
    if backup_path.exists() {
        return Ok(());
    }

    let backup_path_string = display_path(&backup_path);
    match connection.execute("VACUUM INTO ?1", params![backup_path_string]) {
        Ok(_) => Ok(()),
        Err(_) => {
            fs::copy(db_path, &backup_path)
                .map_err(|err| format!("无法备份 Codex 状态数据库：{err}"))?;
            Ok(())
        }
    }
}

fn merge_config(original: &str, provider_id: &str, base_url: &str, model: &str) -> String {
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

    rendered.push_str(&render_provider_block(provider_id, base_url));
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

fn render_provider_block(provider_id: &str, base_url: &str) -> String {
    format!(
        concat!(
            "[model_providers.{table_provider}]\n",
            "name = {provider}\n",
            "base_url = {base_url}\n",
            "wire_api = \"responses\"\n",
            "requires_openai_auth = true\n",
        ),
        table_provider = provider_id,
        provider = toml_string(provider_id),
        base_url = toml_string(base_url),
    )
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
    let mut in_provider = false;
    let provider_header = format!("[model_providers.{provider_id}]");

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') {
            in_provider = trimmed == provider_header;
            continue;
        }

        if !in_provider {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };

        if key.trim() == "base_url" {
            return parse_quoted_toml_string(value.trim());
        }
    }

    None
}

fn read_auth_has_api_key(auth_path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(auth_path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };

    value
        .get(CODEX_AUTH_KEY)
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.trim().is_empty())
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
            configure_with_key_profile,
            get_config_status,
            list_key_profiles,
            open_config_dir,
            restore_defaults,
            save_key_profile,
            delete_key_profile,
            test_key_profile,
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
    let model = options
        .model
        .or_else(|| read_current_model(&config_path))
        .unwrap_or_else(|| MODEL_FALLBACK.to_string());
    let original_config = fs::read_to_string(&config_path).unwrap_or_default();
    let rendered_config = merge_config(
        &original_config,
        &options.provider_id,
        &options.base_url,
        &model,
    );

    if options.dry_run {
        println!("--- {} ---", display_path(&config_path));
        print!("{rendered_config}");
        if let Some(api_key) = options.api_key {
            let rendered_auth = serde_json::to_string_pretty(&json!({ CODEX_AUTH_KEY: api_key }))
                .map_err(|err| format!("无法生成 auth.json：{err}"))?
                + "\n";
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

    let auth_backup_path = write_auth_json(&auth_path, &api_key)?;
    let config_result = write_config_toml(
        &config_path,
        &options.provider_id,
        &options.base_url,
        &model,
    );

    match config_result {
        Ok(config_backup_path) => {
            let session_sync = sync_codex_session_provider_lossy(&codex_home, &options.provider_id);
            println!("Configured provider: {}", options.provider_id);
            println!("Config: {}", display_path(&config_path));
            println!("Auth: {}", display_path(&auth_path));
            println!(
                "Session sync: {} rollout files, {} database rows",
                session_sync.rollout_files_updated, session_sync.sqlite_rows_updated
            );
            if let Some(path) = config_backup_path {
                println!("Config backup: {}", display_path(&path));
            }
            if let Some(path) = auth_backup_path {
                println!("Auth backup: {}", display_path(&path));
            }
            if let Some(path) = session_sync.backup_path {
                println!("Session backup: {path}");
            }
            for warning in session_sync.warnings {
                println!("Session warning: {warning}");
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

        let rendered = merge_config(original, PROVIDER_ID, DEFAULT_BASE_URL, MODEL_FALLBACK);

        assert!(rendered.contains("[model_providers.openrouter]"));
        assert!(rendered.contains("[model_providers.deepseek]"));
        assert!(rendered.contains("base_url = \"https://ocean-way.top\""));
        assert!(!rendered.contains("http://64.188.30.215:8080/v1"));
        assert!(rendered.contains("model_provider = \"OceanWay\""));
        assert!(rendered.contains("model_reasoning_effort = \"high\""));
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
    fn sync_codex_session_provider_updates_rollouts_and_database() {
        let dir = unique_test_dir("session-sync");
        let sessions_dir = dir.join("sessions").join("2026").join("05").join("12");
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::create_dir_all(dir.join("archived_sessions")).unwrap();
        let rollout_path = sessions_dir.join("rollout-test.jsonl");
        fs::write(
            &rollout_path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"one\",\"model_provider\":\"openai\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\"}}\n"
            ),
        )
        .unwrap();

        let db_path = dir.join(CODEX_STATE_DB_NAME);
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO threads (id, model_provider) VALUES ('one', 'openai'), ('two', 'custom')",
                [],
            )
            .unwrap();
        drop(connection);

        let result = sync_codex_session_provider(&dir, PROVIDER_ID).unwrap();

        assert_eq!(result.rollout_files_updated, 1);
        assert_eq!(result.sqlite_rows_updated, 2);
        assert!(result.warnings.is_empty());
        let rendered = fs::read_to_string(&rollout_path).unwrap();
        assert!(rendered.contains("\"model_provider\":\"OceanWay\""));
        assert!(dir
            .join(BACKUP_DIR_NAME)
            .join(SESSION_SYNC_DIR_NAME)
            .join("files")
            .join("sessions")
            .join("2026")
            .join("05")
            .join("12")
            .join("rollout-test.jsonl")
            .exists());

        let connection = Connection::open(&db_path).unwrap();
        let oceanway_rows: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'OceanWay'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(oceanway_rows, 2);
        drop(connection);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn restore_flow_resyncs_all_sessions_to_restored_provider() {
        let dir = unique_test_dir("session-restore");
        fs::create_dir_all(dir.join("sessions")).unwrap();
        let config_path = dir.join("config.toml");
        let auth_path = dir.join("auth.json");
        fs::write(&config_path, "model_provider = \"openai\"\n").unwrap();
        fs::write(&auth_path, "{}\n").unwrap();
        let rollout_path = dir.join("sessions").join("rollout-test.jsonl");
        let new_rollout_path = dir.join("sessions").join("rollout-new.jsonl");
        fs::write(
            &rollout_path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"one\",\"model_provider\":\"openai\"}}\n",
        )
        .unwrap();

        let db_path = dir.join(CODEX_STATE_DB_NAME);
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO threads (id, model_provider) VALUES ('one', 'openai')",
                [],
            )
            .unwrap();
        drop(connection);

        ensure_restore_snapshot(&dir, &config_path, &auth_path).unwrap();
        sync_codex_session_provider(&dir, PROVIDER_ID).unwrap();
        fs::write(&config_path, "model_provider = \"OceanWay\"\n").unwrap();
        fs::write(
            &new_rollout_path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"two\",\"model_provider\":\"OceanWay\"}}\n",
        )
        .unwrap();
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute(
                "INSERT INTO threads (id, model_provider) VALUES ('two', 'OceanWay')",
                [],
            )
            .unwrap();
        drop(connection);

        assert!(fs::read_to_string(&rollout_path)
            .unwrap()
            .contains("\"model_provider\":\"OceanWay\""));

        assert!(restore_from_snapshot(&dir, &config_path, &auth_path).unwrap());
        let result = sync_restored_session_provider_lossy(&dir, &config_path);

        assert_eq!(result.rollout_files_updated, 2);
        assert_eq!(result.sqlite_rows_updated, 2);
        assert!(fs::read_to_string(&rollout_path)
            .unwrap()
            .contains("\"model_provider\":\"openai\""));
        assert!(fs::read_to_string(&new_rollout_path)
            .unwrap()
            .contains("\"model_provider\":\"openai\""));
        let connection = Connection::open(&db_path).unwrap();
        let openai_rows: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'openai'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(openai_rows, 2);
        let total_rows: i64 = connection
            .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
            .unwrap();
        assert_eq!(total_rows, 2);
        drop(connection);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn key_profiles_can_be_saved_updated_listed_and_deleted() {
        let dir = unique_test_dir("key-profiles");
        fs::create_dir_all(&dir).unwrap();

        let first = save_key_profile_in_home(
            &dir,
            None,
            "订阅密钥".to_string(),
            "sk-subscription-123456".to_string(),
            DEFAULT_BASE_URL.to_string(),
        )
        .unwrap();
        assert_eq!(first.name, "订阅密钥");
        assert_eq!(first.masked_key, "sk-sub...3456");
        assert_eq!(first.base_url, DEFAULT_BASE_URL);

        let updated = save_key_profile_in_home(
            &dir,
            Some(first.id.clone()),
            "订阅密钥".to_string(),
            "sk-subscription-abcdef".to_string(),
            "https://balance.example/v1".to_string(),
        )
        .unwrap();
        assert_eq!(updated.id, first.id);
        assert_eq!(updated.masked_key, "sk-sub...cdef");
        assert_eq!(updated.base_url, "https://balance.example/v1");

        let second = save_key_profile_in_home(
            &dir,
            None,
            "余额密钥".to_string(),
            "sk-balance-654321".to_string(),
            "".to_string(),
        )
        .unwrap();
        fs::write(
            dir.join("config.toml"),
            "model_provider = \"OceanWay\"\n\n[model_providers.OceanWay]\nbase_url = \"https://balance.example/v1/\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("auth.json"),
            "{\n  \"OPENAI_API_KEY\": \"sk-subscription-abcdef\"\n}\n",
        )
        .unwrap();

        let profiles = list_key_profiles_in_home(&dir).unwrap();
        assert_eq!(profiles.len(), 2);
        assert!(profiles.iter().any(|profile| profile.name == "订阅密钥"));
        assert!(profiles.iter().any(|profile| profile.name == "余额密钥"));
        assert!(profiles
            .iter()
            .any(|profile| profile.name == "订阅密钥" && profile.active));
        assert!(!profiles
            .iter()
            .any(|profile| profile.name == "余额密钥" && profile.active));

        let profile = find_key_profile(&dir, &updated.id).unwrap().unwrap();
        assert_eq!(profile.api_key, "sk-subscription-abcdef");
        assert_eq!(profile.base_url, "https://balance.example/v1");

        delete_key_profile_in_home(&dir, &second.id).unwrap();
        let profiles = list_key_profiles_in_home(&dir).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, updated.id);

        fs::remove_dir_all(dir).unwrap();
    }
}
