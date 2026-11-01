//! Single app-owned credential record. Secrets have no Serialize/Debug API.
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const API_BASE: &str = "https://console.oceanway.tech/v1";
const RECORD_NAME: &str = "image-provider.json";
const RECORD_LIMIT: u64 = 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub endpoint: String,
    pub key_configured: bool,
    pub default_image_model: Option<String>,
    pub allowed_image_models: Vec<String>,
}

// This disk-only type stays private and is never returned to IPC or logs.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DiskRecord {
    version: u32,
    api_key: String,
    default_model: Option<String>,
    allowed_models: Vec<String>,
    #[cfg(test)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fixture_base: Option<String>,
}

pub(super) struct ProviderRecord {
    pub api_key: String,
    pub api_base: String,
    pub default_model: Option<String>,
    pub allowed_models: Vec<String>,
    pub revision: String,
}

pub(super) fn is_oceanway_endpoint(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("console.oceanway.tech")
        && url.port_or_known_default() == Some(443)
        && url.path().trim_end_matches('/') == "/v1"
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 256
        && !model.starts_with('/')
        && !model
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
        && !model.contains([':', '\\', '?', '#'])
        && !model.split('/').any(|part| part == "..")
}

fn validate(record: &DiskRecord) -> Result<(), String> {
    if record.version != 1
        || record.api_key.trim().is_empty()
        || record.api_key.len() > 16 * 1024
        || record
            .api_key
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err("Save a valid Oceanway API key before continuing.".into());
    }
    if record.allowed_models.len() > 1024
        || record
            .allowed_models
            .iter()
            .any(|model| !valid_model(model) || model.contains(&record.api_key))
        || record
            .default_model
            .as_ref()
            .is_some_and(|model| !record.allowed_models.contains(model))
    {
        return Err(
            "The selected image model must be in the available Oceanway image models.".into(),
        );
    }
    Ok(())
}

fn home_path(home: &Path, create: bool) -> Result<PathBuf, String> {
    if !home.is_absolute() {
        return Err("The Oceanway application data directory must be absolute.".into());
    }
    if create {
        fs::create_dir_all(home)
            .map_err(|_| "Could not create Oceanway application data.".to_string())?;
    }
    let metadata = fs::symlink_metadata(home)
        .map_err(|_| "Oceanway application data is unavailable.".to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Oceanway application data must be a real directory.".into());
    }
    fs::canonicalize(home).map_err(|_| "Could not resolve Oceanway application data.".into())
}

fn validate_record_path(path: &Path, allow_missing: bool) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= RECORD_LIMIT
                && fs::canonicalize(path).ok().as_deref() == Some(path) =>
        {
            Ok(())
        }
        _ => Err("The Oceanway credential record is not a regular application-owned file.".into()),
    }
}

/// Save the one app-owned credential record. Empty image selection is supported;
/// image submission then asks the customer to choose a model. Never log api_key.
pub fn configure(
    home: &Path,
    api_key: &str,
    default_model: Option<&str>,
    allowed_models: &[String],
) -> Result<ProviderSummary, String> {
    let mut models = allowed_models.to_vec();
    models.sort();
    models.dedup();
    let record = DiskRecord {
        version: 1,
        api_key: api_key.trim().into(),
        default_model: default_model
            .filter(|model| !model.is_empty())
            .map(str::to_owned),
        allowed_models: models,
        #[cfg(test)]
        fixture_base: None,
    };
    validate(&record)?;
    let home = home_path(home, true)?;
    write_record(&home, &record)?;
    Ok(summary(&record))
}

fn summary(record: &DiskRecord) -> ProviderSummary {
    ProviderSummary {
        endpoint: API_BASE.into(),
        key_configured: true,
        default_image_model: record.default_model.clone(),
        allowed_image_models: record.allowed_models.clone(),
    }
}

fn write_record(home: &Path, record: &DiskRecord) -> Result<(), String> {
    let path = home.join(RECORD_NAME);
    validate_record_path(&path, true)?;
    let temporary = home.join(format!(
        ".image-provider-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| "Could not stage the Oceanway credential record.".to_string())?;
    let result = (|| {
        serde_json::to_writer(&mut file, record)
            .map_err(|_| "Could not write the Oceanway credential record.".to_string())?;
        file.write_all(b"\n")
            .and_then(|_| file.sync_all())
            .map_err(|_| "Could not persist the Oceanway credential record.".to_string())?;
        drop(file);
        validate_record_path(&path, true)?;
        fs::rename(&temporary, &path)
            .map_err(|_| "Could not replace the Oceanway credential record.".to_string())?;
        #[cfg(unix)]
        File::open(home)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| "Could not flush Oceanway application data.".to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn load(home: &Path) -> Result<(DiskRecord, Vec<u8>), String> {
    let home = home_path(home, false)?;
    let path = home.join(RECORD_NAME);
    validate_record_path(&path, false)?;
    let mut bytes = Vec::new();
    File::open(&path)
        .map_err(|_| "Could not read Oceanway credentials.".to_string())?
        .take(RECORD_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read Oceanway credentials.".to_string())?;
    if bytes.len() as u64 > RECORD_LIMIT {
        return Err("Oceanway credential record is too large.".into());
    }
    let record: DiskRecord = serde_json::from_slice(&bytes)
        .map_err(|_| "Oceanway credential record is invalid.".to_string())?;
    validate(&record)?;
    Ok((record, bytes))
}

pub fn read_provider_summary(home: &Path) -> Result<ProviderSummary, String> {
    Ok(summary(&load(home)?.0))
}

/// Backend-only access for configuring agent adapters. Never register this as an
/// IPC command, serialize its return value, or include it in diagnostic logs.
pub fn read_api_key(home: &Path) -> Result<String, String> {
    Ok(load(home)?.0.api_key)
}

pub(super) fn read_record(home: &Path) -> Result<ProviderRecord, String> {
    let (record, bytes) = load(home)?;
    let api_base = API_BASE.to_owned();
    #[cfg(test)]
    let api_base = record.fixture_base.unwrap_or(api_base);
    Ok(ProviderRecord {
        api_key: record.api_key,
        api_base,
        default_model: record.default_model,
        allowed_models: record.allowed_models,
        revision: format!("{:x}", Sha256::digest(bytes)),
    })
}

#[cfg(test)]
pub(super) fn fixture(home: &Path, base: &str, key: &str, model: &str) {
    let record = DiskRecord {
        version: 1,
        api_key: key.into(),
        default_model: Some(model.into()),
        allowed_models: vec![model.into()],
        fixture_base: Some(base.into()),
    };
    // Deliberately accept malformed fixture credentials so loader rejection is tested.
    write_record(home, &record).unwrap();
}
