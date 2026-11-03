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

/// Discover the current Key's full image catalog. Saved selections are user
/// preferences, not a replacement for the gateway's entitlement list.
pub(super) fn image_models(home: &Path) -> Result<serde_json::Value, String> {
    let record = read_record(home)?;
    let models = fetch_image_models(&record.api_base, &record.api_key)?;
    let default_model = record.default_model.filter(|id| models.contains(id));
    Ok(serde_json::json!({"default_model": default_model, "models": models}))
}

pub(super) fn fetch_image_models(base: &str, key: &str) -> Result<Vec<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|_| "Could not initialize Oceanway model discovery.")?;
    let read = |response: reqwest::blocking::Response| -> Result<serde_json::Value, String> {
        if !response.status().is_success() {
            return Err(format!("Could not load Oceanway models (HTTP {}).", response.status().as_u16()));
        }
        let mut bytes = Vec::new();
        response.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)
            .map_err(|_| "Could not read Oceanway models.")?;
        if bytes.len() > 4 * 1024 * 1024 { return Err("Oceanway model response is too large.".into()); }
        serde_json::from_slice(&bytes).map_err(|_| "Invalid Oceanway model response.".into())
    };
    let available = read(client.get(format!("{base}/models")).bearer_auth(key)
        .send().map_err(|_| "Could not load models available to this Key. Please retry.")?)?;
    let mut origin = Url::parse(base).map_err(|_| "Invalid Oceanway endpoint.")?;
    origin.set_path("/api/platform/v1/public/offering");
    let offering = read(client.get(origin).send()
        .map_err(|_| "Could not load Oceanway image capabilities. Please retry.")?)?;
    select_image_models(&available, &offering)
}

fn select_image_models(available: &serde_json::Value, offering: &serde_json::Value) -> Result<Vec<String>, String> {
    let ids = available["data"].as_array().ok_or("Invalid Oceanway model list.")?;
    let rows = offering.pointer("/data/models").and_then(serde_json::Value::as_array)
        .ok_or("Invalid Oceanway image capabilities.")?;
    let mut models: Vec<String> = rows.iter().filter(|row|
        row["model_type"] == "image" && row["invocation_status"] == "available"
            && ids.iter().any(|id| id["id"] == row["model_name"]))
        .filter_map(|row| row["model_name"].as_str().filter(|id| valid_model(id)).map(str::to_owned))
        .collect();
    models.sort();
    models.dedup();
    Ok(models)
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn discovery_reads_the_full_authenticated_catalog_without_changing_saved_preferences() {
        use std::net::TcpListener;
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", server.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = server.accept().unwrap();
                stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0]; stream.read_exact(&mut byte).unwrap(); header.push(byte[0]);
                }
                let header = String::from_utf8(header).unwrap().to_lowercase();
                let body = if index == 0 {
                    assert!(header.starts_with("get /v1/models "));
                    assert!(header.contains("authorization: bearer catalog-fixture-secret"));
                    json!({"data":[{"id":"saved-image"},{"id":"new-image"}]})
                } else {
                    assert!(header.starts_with("get /api/platform/v1/public/offering "));
                    assert!(!header.contains("authorization:"));
                    json!({"data":{"models":[
                        {"model_name":"saved-image","model_type":"image","invocation_status":"available"},
                        {"model_name":"new-image","model_type":"image","invocation_status":"available"},
                        {"model_name":"not-entitled","model_type":"image","invocation_status":"available"}]}})
                }.to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        let home = std::env::temp_dir().join(format!("oceanway-catalog-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&home).unwrap();
        let home = fs::canonicalize(home).unwrap();
        fixture(&home,&base,"catalog-fixture-secret","saved-image");
        let before = fs::read(home.join(RECORD_NAME)).unwrap();
        let result = image_models(&home).unwrap();
        worker.join().unwrap();
        assert_eq!(result,json!({"default_model":"saved-image","models":["new-image","saved-image"]}));
        assert!(!result.to_string().contains("catalog-fixture-secret"));
        assert_eq!(fs::read(home.join(RECORD_NAME)).unwrap(),before);
        fs::remove_file(home.join(RECORD_NAME)).unwrap(); fs::remove_dir(home).unwrap();
    }

    #[test]
    fn discovers_every_entitled_image_family_without_text_video_or_unavailable_models() {
        let available = json!({"data":[{"id":"gpt-image-2"},{"id":"gemini-nano-banana-2.1"},
            {"id":"grok-imagine-image"},{"id":"new-picture-model"},{"id":"gpt-6"},
            {"id":"seedance-2.0"},{"id":"offline-image"},{"id":"unclassified"}]});
        let mut rows: Vec<_> = ["gpt-image-2","gemini-nano-banana-2.1","grok-imagine-image",
            "new-picture-model","not-entitled"].iter().map(|id|
            json!({"model_name":id,"model_type":"image","invocation_status":"available"})).collect();
        rows.extend([
            json!({"model_name":"gpt-6","model_type":"text","invocation_status":"available"}),
            json!({"model_name":"seedance-2.0","model_type":"video","invocation_status":"available"}),
            json!({"model_name":"offline-image","model_type":"image","invocation_status":"unavailable"})]);
        assert_eq!(select_image_models(&available,&json!({"data":{"models":rows}})).unwrap(),
            vec!["gemini-nano-banana-2.1","gpt-image-2","grok-imagine-image","new-picture-model"]);
        assert!(select_image_models(&json!({}),&json!({})).is_err());
    }
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
