//! Shared image engine for explicit Tauri/MCP submissions. Register ImageJobs as Tauri state.
//!
//! `completed` counts successes, not all terminal items. Indices are one-based.
//! The queue is a cursor, not a count-sized allocation. `items` contains every
//! attempted index plus at most two unattempted queue previews. Absent indices
//! are queued (or cancelled after cancellation); aggregate counters remain exact.
//! Model discovery is metadata only and never proves image or batch support.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use reqwest::blocking::{multipart, Client, ClientBuilder, Response};
use reqwest::{redirect::Policy, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::State;

const IMAGE_BYTES: usize = 50 * 1024 * 1024;
const REFERENCE_BYTES: usize = 256 * 1024 * 1024;
const JSON_BYTES: usize = 72 * 1024 * 1024;
const MODEL_BYTES: usize = 4 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(240);
const LEASE: Duration = Duration::from_secs(120);
const PREVIEW_BUDGET: usize = 96 * 1024;
const MAX_PREVIEWS: usize = 2;
const LOCAL_FIXTURE_KEY: &str = "oceanway-local-fixture";
const BILLING_NOTICE: &str = "Each attempt sends n=1; batch support is unverified. No automatic retry. \
    Timeout, cancellation, or an unusable response does not prove that billing did not occur. \
    Cancellation stops queued work; in-flight requests finish. Explicit retry may incur another charge.";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// True cancels submission. Called under the registry lock: keep it nonblocking
/// and do not call back into ImageJobs. Never serialized into job manifests.
pub type ImageSubmissionGuard = Arc<dyn Fn() -> bool + Send + Sync>;

fn no_submission_guard() -> ImageSubmissionGuard {
    Arc::new(|| false)
}

fn check_submission_guard(guard: &ImageSubmissionGuard) -> Result<(), Failure> {
    let message = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| guard())) {
        Ok(false) => return Ok(()),
        Ok(true) => "Image submission cancelled before admission; no POST was sent for this index.",
        Err(_) => "Image submission cancellation guard failed; no POST was sent for this index.",
    };
    Err(Failure { stop_job: true, ..Failure::not_sent(message) })
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageTestRequest {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub prompts: Vec<String>,
    pub count: usize,
    #[serde(default)]
    pub reference_paths: Vec<String>,
    #[serde(default = "default_size")]
    pub size: String,
}

fn default_model() -> String {
    "gpt-image-2".into()
}

fn default_size() -> String {
    "1024x1024".into()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageCapabilities {
    pub model: String,
    pub available: bool,
    pub models: Vec<String>,
    pub endpoint: String,
    pub message: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageItem {
    pub index: usize,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_data_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<ImageOutput>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub count_mismatch: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub retry_blocked: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub post_started: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOutput {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
    pub bytes: u64,
}

impl ImageItem {
    fn new(index: usize, status: &str) -> Self {
        Self {
            index,
            status: status.into(),
            path: None,
            preview_data_url: None,
            error: None,
            request_id: None,
            elapsed_ms: None,
            warning: None,
            additional_paths: Vec::new(),
            width: None,
            height: None,
            sha256: None,
            outputs: Vec::new(),
            count_mismatch: false,
            retry_blocked: false,
            post_started: false,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageJob {
    pub id: String,
    pub status: String,
    pub model: String,
    pub mode: String,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub items: Vec<ImageItem>,
    pub message: String,
    pub output_directory: String,
    pub reference_paths: Vec<String>,
    pub reference_hashes: Vec<String>,
    pub warnings: Vec<String>,
    pub count_mismatch: bool,
    pub persistence_error: Option<String>,
    pub paused: bool,
    pub leased: bool,
    pub recovered: bool,
    pub outcome_unknown: usize,
    pub lease_remaining_ms: Option<u64>,
}

// Credentials and the config snapshot never implement Serialize or Debug.
#[derive(Clone)]
struct SavedProvider {
    home: PathBuf,
    config: String,
    api_base: String,
    key: String,
    #[cfg(test)]
    local_mock: bool,
}

#[derive(Clone)]
struct Reference {
    bytes: Arc<[u8]>,
    mime: &'static str,
    extension: &'static str,
}

#[derive(Clone)]
struct Input {
    request: ImageTestRequest,
    provider: SavedProvider,
    references: Vec<Reference>,
    directory: PathBuf,
}

struct StoredJob {
    id: String,
    input: Arc<Input>,
    next: Option<usize>,
    stopped: bool,
    active: usize,
    completed: usize,
    failed: usize,
    items: BTreeMap<usize, ImageItem>,
    persistence_error: Option<String>,
    reference_hashes: Vec<String>,
    ownership_token: String,
    leased: bool,
    lease_deadline: Option<Instant>,
    paused: bool,
    recovered: bool,
    revision: u64,
    submission_guard: ImageSubmissionGuard,
    // The kernel lock lives as long as this registry entry, including while
    // cancellation drains admitted requests. It is never cloned or serialized.
    ownership: Mutex<Option<File>>,
}

impl StoredJob {
    fn new(id: String, input: Input) -> Self {
        let reference_hashes = input.references.iter().map(|reference| hash(&reference.bytes)).collect();
        let ownership_token = hash(format!(
            "{}:{id}:{}", input.provider.home.display(), SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ).as_bytes());
        Self {
            id,
            input: Arc::new(input),
            next: Some(1),
            stopped: false,
            active: 0,
            completed: 0,
            failed: 0,
            items: BTreeMap::new(),
            persistence_error: None,
            reference_hashes,
            ownership_token,
            leased: false,
            lease_deadline: None,
            paused: false,
            recovered: false,
            revision: 0,
            submission_guard: no_submission_guard(),
            ownership: Mutex::new(None),
        }
    }

    fn advance(&mut self) {
        self.next = self.next.and_then(|index| index.checked_add(1))
            .filter(|index| *index <= self.input.request.count);
    }

    fn skip_successes(&mut self) {
        while self.next.is_some_and(|index| {
            self.items.get(&index).is_some_and(|item| item.status == "succeeded" || item.retry_blocked)
        }) {
            self.advance();
        }
    }

    fn running(&self) -> bool {
        self.active > 0 || (!self.stopped && !self.paused && !self.recovered && self.next.is_some())
    }

    fn claim(&mut self) -> Option<usize> {
        if check_submission_guard(&self.submission_guard).is_err() {
            self.cancel();
            return None;
        }
        self.expire_lease();
        if self.stopped || self.paused || self.recovered {
            return None;
        }
        let index = self.next?;
        self.advance();
        self.skip_successes();
        self.items.insert(index, ImageItem::new(index, "running"));
        self.active += 1;
        Some(index)
    }

    fn snapshot(&self) -> ImageJob {
        let total = self.input.request.count;
        let cancelled = if self.stopped {
            total.saturating_sub(self.completed).saturating_sub(self.failed).saturating_sub(self.active)
        } else {
            0
        };
        let status = if self.running() {
            "running"
        } else if self.completed == total {
            if self.items.values().any(|item| item.warning.is_some() || item.count_mismatch) {
                "completed_with_warnings"
            } else {
                "completed"
            }
        } else if self.recovered {
            "interrupted"
        } else if self.paused {
            "paused"
        } else if self.stopped {
            "cancelled"
        } else if self.completed > 0 {
            "partial"
        } else {
            "failed"
        };
        let mut items: Vec<_> = self.items.values().cloned().collect();
        if !self.stopped {
            let mut cursor = self.next;
            let mut previews = 0;
            while let Some(index) = cursor {
                if !self.items.contains_key(&index) {
                    items.push(ImageItem::new(index, if self.paused || self.recovered { "paused" } else { "queued" }));
                    previews += 1;
                    if previews == 2 {
                        break;
                    }
                }
                cursor = index.checked_add(1).filter(|next| *next <= total);
            }
        }
        items.sort_by_key(|item| item.index);
        bound_previews(&mut items);
        let warning_count = self.items.values().filter(|item| item.warning.is_some()).count();
        let response_warning = self.items.values().find_map(|item| item.warning.as_ref())
            .map(|warning| format!(" {warning_count} index/indices have response warnings. {warning}"))
            .unwrap_or_default();
        ImageJob {
            id: self.id.clone(),
            status: status.into(),
            model: self.input.request.model.clone(),
            mode: if self.input.request.reference_paths.is_empty() { "generate" } else { "edit" }.into(),
            total,
            completed: self.completed,
            failed: self.failed,
            cancelled,
            items,
            output_directory: self.input.directory.to_string_lossy().into_owned(),
            reference_paths: self.input.request.reference_paths.clone(),
            reference_hashes: self.reference_hashes.clone(),
            warnings: self.items.values().filter_map(|item| item.warning.clone()).take(8).collect(),
            count_mismatch: self.items.values().any(|item| item.count_mismatch),
            persistence_error: self.persistence_error.clone(),
            paused: self.paused || self.recovered,
            leased: self.leased,
            recovered: self.recovered,
            outcome_unknown: self.items.values().filter(|item| item.status == "uncertain").count(),
            lease_remaining_ms: self.lease_deadline.map(|deadline|
                u64::try_from(deadline.saturating_duration_since(Instant::now()).as_millis()).unwrap_or(u64::MAX)),
            message: format!(
                "{BILLING_NOTICE} Items include attempted indices and at most two queue previews; \
                 other indices are {}. manifest.json is the durable progress record unless a \
                 persistenceError is reported. Restarting never automatically resumes POSTs. \
                 Paused/recovered jobs require explicit retry; status only renews the 120s lease.{}{}",
                if self.stopped { "cancelled" } else if self.paused || self.recovered { "paused" } else { "queued" },
                self.persistence_error.as_ref().map(|error| format!(" {error}")).unwrap_or_default(),
                response_warning
            ),
        }
    }

    fn cancel(&mut self) {
        self.stopped = true;
        self.paused = false;
        self.recovered = false;
        self.revision = self.revision.wrapping_add(1);
        for item in self.items.values_mut().filter(|item| item.status == "queued") {
            item.status = "cancelled".into();
        }
    }

    fn retry(&mut self) -> Result<(), String> {
        if self.running() {
            return Err("Wait for all in-flight image requests to finish before retrying.".into());
        }
        if self.items.values().filter(|item| item.status == "succeeded" || item.retry_blocked).count()
            == self.input.request.count
        {
            return Err("All indices are already saved or blocked by output integrity evidence; nothing to retry.".into());
        }
        for item in self.items.values_mut().filter(|item| item.status != "succeeded" && !item.retry_blocked) {
            // Retain the last failure's evidence until this index is actually claimed.
            item.status = "queued".into();
        }
        self.failed = self.items.values().filter(|item| item.retry_blocked && item.status != "succeeded").count();
        self.stopped = false;
        self.paused = false;
        self.recovered = false;
        self.revision = self.revision.wrapping_add(1);
        self.renew_lease();
        self.next = Some(1);
        self.skip_successes();
        self.persistence_error = None;
        persist_or_stop(self)?;
        self.check_submission().map_err(|failure| failure.message)
    }

    fn check_submission(&mut self) -> Result<(), Failure> {
        if let Err(failure) = check_submission_guard(&self.submission_guard) {
            self.cancel();
            let _ = persist_or_stop(self);
            return Err(failure);
        }
        Ok(())
    }

    fn renew_lease(&mut self) {
        self.lease_deadline = self.leased.then(|| Instant::now() + LEASE);
    }

    fn expire_lease(&mut self) -> bool {
        if !self.stopped && !self.paused && !self.recovered
            && (self.next.is_some() || self.active > 0)
            && self.lease_deadline.is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.paused = true;
            self.revision = self.revision.wrapping_add(1);
            return true;
        }
        false
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn bound_previews(items: &mut [ImageItem]) {
    let mut remaining = PREVIEW_BUDGET;
    let mut count = 0;
    for item in items.iter_mut().rev() {
        if let Some(preview) = &item.preview_data_url {
            if count < MAX_PREVIEWS && preview.len() <= remaining {
                remaining -= preview.len();
                count += 1;
            } else {
                item.preview_data_url = None;
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobManifest {
    schema_version: u32,
    updated_at_ms: u64,
    requires_explicit_resume: bool,
    recovery_message: String,
    engine: String,
    codex_home: PathBuf,
    ownership_token: String,
    prompt: String,
    prompts: Vec<String>,
    model: String,
    reference_paths: Vec<String>,
    size: String,
    count: usize,
    output_directory: String,
    leased: bool,
    job: ImageJob,
}

fn redact_manifest(value: &mut Value, key: &str) {
    match value {
        Value::String(text) if !key.is_empty() => *text = text.replace(key, "[REDACTED]"),
        Value::Array(values) => {
            for value in values {
                redact_manifest(value, key);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                redact_manifest(value, key);
            }
        }
        _ => {}
    }
}

fn persist_manifest(job: &StoredJob) -> Result<(), String> {
    ensure_job_ownership(job)?;
    verify_directory(&job.input.directory)?;
    let mut snapshot = job.snapshot();
    for item in &mut snapshot.items {
        item.preview_data_url = None;
    }
    let manifest = JobManifest {
        schema_version: 2,
        updated_at_ms: u64::try_from(
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        ).unwrap_or(u64::MAX),
        requires_explicit_resume: true,
        recovery_message: "This is the last saved progress, not a resumed job. No POST is resumed \
            automatically. Previously running indices may already have been billed. Review saved \
            outputs and explicitly choose recovery before sending any further paid request.".into(),
        engine: ENGINE_ID.into(),
        codex_home: job.input.provider.home.clone(),
        ownership_token: job.ownership_token.clone(),
        prompt: job.input.request.prompt.clone(),
        prompts: job.input.request.prompts.clone(),
        model: job.input.request.model.clone(),
        reference_paths: job.input.request.reference_paths.clone(),
        size: job.input.request.size.clone(),
        count: job.input.request.count,
        output_directory: job.input.directory.to_string_lossy().into_owned(),
        leased: job.leased,
        job: snapshot,
    };
    let mut manifest = serde_json::to_value(manifest)
        .map_err(|_| "Could not serialize the image progress record.".to_string())?;
    // Even a credential accidentally pasted into prompt/path/error text must not
    // enter this disk record. The provider/config snapshot is never serialized.
    redact_manifest(&mut manifest, &job.input.provider.key);
    let temporary = job.input.directory.join(format!(
        ".manifest-{}-{}.tmp", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)
        .map_err(|_| "Could not create the image progress record.".to_string())?;
    let result = (|| {
        serde_json::to_writer_pretty(&mut file, &manifest)
            .map_err(|_| "Could not write the image progress record.".to_string())?;
        file.write_all(b"\n").and_then(|_| file.sync_all())
            .map_err(|_| "Could not flush the image progress record.".to_string())?;
        drop(file);
        // The old record survives a failed write; readers only see whole JSON.
        fs::rename(&temporary, job.input.directory.join("manifest.json"))
            .map_err(|_| "Could not replace the image progress record.".to_string())?;
        #[cfg(unix)]
        File::open(&job.input.directory).and_then(|directory| directory.sync_all())
            .map_err(|_| "Could not flush the image progress directory.".to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn persist_or_stop(job: &mut StoredJob) -> Result<(), String> {
    if let Err(error) = persist_manifest(job) {
        let message = format!("Progress could not be persisted: {error} Queued work stopped; in-flight requests \
            may finish and may have been billed. Existing images are not deleted.");
        job.persistence_error = Some(message.clone());
        job.cancel();
        return Err(message);
    }
    Ok(())
}

#[derive(Default)]
struct Registry {
    jobs: BTreeMap<String, StoredJob>,
    workers: usize,
    last_job: Option<String>,
    shutdown: bool,
    home: Option<PathBuf>,
}

#[derive(Default)]
struct Shared {
    registry: Mutex<Registry>,
    wake: Condvar,
}

#[derive(Clone)]
pub struct ImageJobs {
    shared: Arc<Shared>,
    _lifetime: Arc<ServiceLifetime>,
}

struct ServiceLifetime {
    shared: Arc<Shared>,
}

impl Default for ImageJobs {
    fn default() -> Self {
        let shared = Arc::new(Shared::default());
        Self { _lifetime: Arc::new(ServiceLifetime { shared: Arc::clone(&shared) }), shared }
    }
}

impl Drop for ServiceLifetime {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.shared.registry.lock() {
            registry.shutdown = true;
            for job in registry.jobs.values_mut() {
                if job.running() || job.paused || job.recovered {
                    job.cancel();
                    let _ = persist_or_stop(job);
                }
            }
            self.shared.wake.notify_all();
        }
    }
}

#[path = "image_engine_store.rs"]
mod durable;
use durable::*;

fn lock(shared: &Shared) -> Result<MutexGuard<'_, Registry>, String> {
    shared.registry.lock().map_err(|_| "Image job state is unavailable.".into())
}

fn find_job<'a>(registry: &'a Registry, id: &str) -> Result<&'a StoredJob, String> {
    registry.jobs.get(id).ok_or_else(|| "Unknown image job.".into())
}

fn find_job_mut<'a>(registry: &'a mut Registry, id: &str) -> Result<&'a mut StoredJob, String> {
    registry.jobs.get_mut(id).ok_or_else(|| "Unknown image job.".into())
}

fn ensure_workers(shared: &Arc<Shared>, registry: &mut Registry) -> Result<(), String> {
    let pool = worker_pool();
    let mut state = pool.state.lock().map_err(|_| "Image worker pool is unavailable.".to_string())?;
    if !state.services.iter().any(|service| service.ptr_eq(&Arc::downgrade(shared))) {
        state.services.push(Arc::downgrade(shared));
    }
    while state.workers < 2 {
        let pool = Arc::clone(pool);
        thread::Builder::new()
            .name(format!("image-api-{}", state.workers + 1))
            .spawn(move || worker(pool))
            .map_err(|_| "Could not start image workers; no new job was submitted.".to_string())?;
        state.workers += 1;
    }
    registry.workers = state.workers;
    pool.wake.notify_all();
    Ok(())
}

#[derive(Default)]
struct PoolState {
    services: Vec<Weak<Shared>>,
    cursor: usize,
    workers: usize,
}

#[derive(Default)]
struct WorkerPool {
    state: Mutex<PoolState>,
    wake: Condvar,
}

fn worker_pool() -> &'static Arc<WorkerPool> {
    static POOL: OnceLock<Arc<WorkerPool>> = OnceLock::new();
    POOL.get_or_init(|| Arc::new(WorkerPool::default()))
}

fn claim_task(shared: &Arc<Shared>) -> Option<(String, usize, Arc<Input>)> {
    // Never hold the pool lock while acquiring a service registry.
    let mut registry = shared.registry.try_lock().ok()?;
    if registry.shutdown {
        return None;
    }
    for job in registry.jobs.values_mut() {
        if !job.stopped && !job.recovered && (job.next.is_some() || job.active > 0)
            && job.check_submission().is_err()
        {
            continue;
        }
        if job.expire_lease() {
            let _ = persist_or_stop(job);
        }
    }
    let eligible = |job: &&StoredJob| !job.stopped && !job.paused && !job.recovered && job.next.is_some();
    let id = registry.jobs.values().filter(eligible)
        .find(|job| registry.last_job.as_ref().is_none_or(|last| job.id > *last))
        .or_else(|| registry.jobs.values().find(eligible))
        .map(|job| job.id.clone())?;
    let job = registry.jobs.get_mut(&id)?;
    let index = match job.claim() {
        Some(index) => index,
        None => {
            let _ = persist_or_stop(job);
            return None;
        }
    };
    if persist_or_stop(job).is_err() {
        job.active -= 1;
        let item = job.items.get_mut(&index)?;
        item.status = "cancelled".into();
        item.error = Some("Progress write failed; no request was sent for this index.".into());
        return None;
    }
    let input = Arc::clone(&job.input);
    registry.last_job = Some(id.clone());
    Some((id, index, input))
}

fn worker(pool: Arc<WorkerPool>) {
    loop {
        let services = {
            let mut state = match pool.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            state.services.retain(|service| service.strong_count() > 0);
            let len = state.services.len();
            if len > 0 {
                state.cursor = (state.cursor + 1) % len;
            }
            (0..len).filter_map(|offset|
                state.services[(state.cursor + offset) % len].upgrade()).collect::<Vec<_>>()
        };
        let selected = services.iter().find_map(|shared|
            claim_task(shared).map(|task| (Arc::clone(shared), task)));
        drop(services);
        let Some((shared, (id, index, input))) = selected else {
            let state = match pool.state.lock() { Ok(state) => state, Err(_) => return };
            // Also expires leases without callers polling; a notification is only an optimization.
            drop(pool.wake.wait_timeout(state, Duration::from_millis(100)));
            continue;
        };
        let started = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute_with_guard(&input, index, started, || authorize_post(&shared, &id, index, &input))
        })).unwrap_or_else(|_| Err(Failure::new("Image worker failed; billing may be unknown.")));
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        drop(input);
        if let Ok(mut registry) = lock(&shared) {
            if let Some(job) = registry.jobs.get_mut(&id) {
                finish_item(job, index, outcome, elapsed);
                let _ = persist_or_stop(job);
                release_completed_input(job);
            }
            shared.wake.notify_all();
        } else {
            return;
        };
    }
}

fn finish_item(job: &mut StoredJob, index: usize, outcome: Result<SavedResult, Failure>, elapsed: u64) {
    let stop_job = outcome.as_ref().err().is_some_and(|failure| failure.stop_job);
    let item = job.items.get_mut(&index).expect("claimed index");
    item.elapsed_ms = Some(elapsed);
    match outcome {
        Ok(result) => {
            item.status = "succeeded".into();
            item.path = Some(result.path.to_string_lossy().into_owned());
            item.preview_data_url = Some(result.preview);
            item.request_id = result.request_id;
            item.warning = result.warning;
            item.additional_paths = result.additional_paths;
            item.width = Some(result.outputs[0].width);
            item.height = Some(result.outputs[0].height);
            item.sha256 = Some(result.outputs[0].sha256.clone());
            item.outputs = result.outputs;
            item.count_mismatch = result.count_mismatch;
            job.completed += 1;
        }
        Err(failure) => {
            item.status = if failure.not_sent {
                if job.paused { "paused" } else { "cancelled" }
            } else {
                "failed"
            }.into();
            item.error = Some(failure.message);
            item.request_id = failure.request_id;
            item.count_mismatch = failure.count_mismatch;
            if !failure.not_sent {
                job.failed += 1;
            }
        }
    }
    job.active -= 1;
    if stop_job {
        job.cancel();
    }
    let mut preview_bytes = 0;
    let mut previews = 0;
    for item in job.items.values_mut().rev() {
        if let Some(preview) = &item.preview_data_url {
            preview_bytes += preview.len();
            previews += 1;
            if preview_bytes > PREVIEW_BUDGET || previews > MAX_PREVIEWS {
                item.preview_data_url = None;
            }
        }
    }
}

fn authorize_post(shared: &Shared, id: &str, index: usize, input: &Input) -> Result<SavedProvider, Failure> {
    authorize_post_with_loader(shared, id, index, input, || load_post_provider(&input.provider))
}

fn authorize_post_with_loader(
    shared: &Shared, id: &str, index: usize, input: &Input,
    mut load: impl FnMut() -> Result<SavedProvider, Failure>,
) -> Result<SavedProvider, Failure> {
    let mut registry = lock(shared)?;
    let shutdown = registry.shutdown;
    let job = find_job_mut(&mut registry, id)?;
    if job.expire_lease() {
        let _ = persist_or_stop(job);
    }
    if shutdown || job.stopped || job.paused || job.recovered {
        return Err(Failure::not_sent("Image queue stopped before dispatch; no POST was sent for this index."));
    }
    job.check_submission()?;
    // The body is fully built. Re-read this service's explicit home, never the
    // ambient CODEX_HOME. Cancellation and dispatch share this admission lock.
    let provider = load()?;
    validate_submission_provider(input, &provider)?;
    verify_unused_slot(input, index)?;
    Arc::make_mut(&mut job.input).provider = provider;
    let item = job.items.get_mut(&index).ok_or_else(|| Failure::not_sent("Missing image slot."))?;
    item.post_started = true;
    if let Err(error) = persist_or_stop(job) {
        return Err(Failure::not_sent(error));
    }
    job.check_submission()?;
    // A second read closes the disk-persistence preflight window. A cancellation
    // after this admission is in-flight; blocking send intentionally runs unlocked.
    let provider = load()?;
    validate_submission_provider(input, &provider)?;
    verify_unused_slot(input, index)?;
    if job.expire_lease() {
        let _ = persist_or_stop(job);
        return Err(Failure::not_sent("Image lease expired before dispatch; no POST was sent."));
    }
    Arc::make_mut(&mut job.input).provider = provider.clone();
    job.check_submission()?;
    Ok(provider)
}

fn validate_submission_provider(input: &Input, provider: &SavedProvider) -> Result<(), Failure> {
    if provider.home != input.provider.home {
        return Err(Failure::not_sent("Image submission owner changed; no POST was sent."));
    }
    if checked_provider_transport(&input.provider)? != checked_provider_transport(provider)? {
        return Err(Failure {
            stop_job: true,
            ..Failure::not_sent("Image provider transport changed during preflight; no POST was sent. \
                Review the saved provider and explicitly retry.")
        });
    }
    ensure_inputs_do_not_contain_key(&input.request, &provider.key)?;
    ensure_no_credential_in_values(
        [input.directory.to_string_lossy().as_ref(), provider.home.to_string_lossy().as_ref()],
        &provider.key,
    )?;
    Ok(())
}

fn release_completed_input(job: &mut StoredJob) {
    if job.active == 0 && job.completed == job.input.request.count {
        // Arc references to image bytes are cheap to clone if a concurrent command
        // still holds this input. That command releases its old snapshot on return.
        let input = Arc::make_mut(&mut job.input);
        input.references.clear();
        input.provider.key.clear();
        input.provider.config.clear();
    }
}

#[tauri::command]
pub async fn check_image_capabilities(model: String) -> Result<ImageCapabilities, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let model = validate_model(&model)?;
        let provider = load_provider()?;
        ensure_no_credential_in_values([model.as_str()], &provider.key)?;
        Ok(probe_capabilities(&provider, model))
    }).await.map_err(|_| "Image capability task failed.".to_string())?
}

fn probe_capabilities(provider: &SavedProvider, model: String) -> ImageCapabilities {
    let endpoint = format!("{}/models", provider.api_base);
    let result = (|| {
        let client = provider_client(provider).map_err(|failure| failure.message)?;
        let response = client.get(&endpoint).bearer_auth(&provider.key).send()
            .map_err(|err| network_failure(&err).message)?;
        let (body, _) = json_response(response, &provider.key, MODEL_BYTES)
            .map_err(|failure| failure.message)?;
        let data = body.get("data").and_then(Value::as_array)
            .ok_or_else(|| "Model metadata is missing its data array.".to_string())?;
        let mut models = Vec::new();
        for entry in data {
            if let Some(id) = entry.get("id").and_then(Value::as_str) {
                if let Ok(id) = validate_model(id) {
                    if !id.contains(&provider.key) {
                        models.push(id);
                    }
                }
            }
        }
        models.sort();
        models.dedup();
        Ok::<_, String>(models)
    })();
    match result {
        Ok(models) => ImageCapabilities {
            available: models.iter().any(|id| id == &model),
            model,
            models,
            endpoint,
            message: "GET /models only. Availability means listed metadata, not verified image \
                generation, edits, pricing, sizes, or batch support. No image POST was sent.".into(),
        },
        Err(message) => ImageCapabilities {
            model,
            available: false,
            models: Vec::new(),
            endpoint,
            message: format!("Image capability is unverified: {message} No image POST was sent."),
        },
    }
}

#[tauri::command]
pub async fn test_image_api(
    request: ImageTestRequest,
    state: State<'_, ImageJobs>,
) -> Result<ImageJob, String> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.start(request, None, false))
        .await.map_err(|_| "Image submission task failed.".to_string())?
}

#[cfg(test)]
fn submit(shared: &Arc<Shared>, id: String, input: Input) -> Result<ImageJob, String> {
    submit_leased(shared, id, input, false)
}

#[cfg(test)]
fn submit_leased(shared: &Arc<Shared>, id: String, input: Input, leased: bool) -> Result<ImageJob, String> {
    submit_guarded(shared, id, input, leased, no_submission_guard())
}

fn submit_guarded(
    shared: &Arc<Shared>, id: String, input: Input, leased: bool, guard: ImageSubmissionGuard,
) -> Result<ImageJob, String> {
    check_submission_guard(&guard).map_err(|failure| failure.message)?;
    ensure_inputs_do_not_contain_key(&input.request, &input.provider.key)?;
    ensure_no_credential_in_values(
        [input.directory.to_string_lossy().as_ref(), input.provider.home.to_string_lossy().as_ref()],
        &input.provider.key,
    )?;
    let mut registry = lock(shared)?;
    if registry.shutdown {
        return Err("Image jobs are shutting down.".into());
    }
    if registry.jobs.contains_key(&id) {
        return Err("Image job already exists.".into());
    }
    if registry.home.as_ref().is_some_and(|home| home != &input.provider.home) {
        return Err("Image service cannot mix CODEX_HOME owners.".into());
    }
    registry.home = Some(input.provider.home.clone());
    let mut job = StoredJob::new(id.clone(), input);
    job.submission_guard = guard;
    job.leased = leased;
    job.renew_lease();
    check_submission_guard(&job.submission_guard).map_err(|failure| failure.message)?;
    persist_manifest(&job)?;
    job.check_submission().map_err(|failure| failure.message)?;
    register_manifest(&job)?;
    ensure_workers(shared, &mut registry)?;
    // Workers cannot claim this entry before publication under the registry lock.
    // They retain the same guard for cancellation after this final check.
    job.check_submission().map_err(|failure| failure.message)?;
    let snapshot = job.snapshot();
    registry.jobs.insert(id, job);
    shared.wake.notify_all();
    worker_pool().wake.notify_all();
    Ok(snapshot)
}

impl ImageJobs {
    /// An explicit home is mandatory for the stdio service. Recovery reads only
    /// the engine index and its owned manifests, and never loads keys or sends HTTP.
    pub fn new(codex_home: impl AsRef<Path>) -> Result<Self, String> {
        let service = Self::default();
        service.initialize(codex_home.as_ref())?;
        Ok(service)
    }

    pub fn with_home(codex_home: impl AsRef<Path>) -> Result<Self, String> {
        Self::new(codex_home)
    }

    fn initialize(&self, home: &Path) -> Result<(), String> {
        let home = fs::canonicalize(home).map_err(|_| "CODEX_HOME does not exist.".to_string())?;
        let mut registry = lock(&self.shared)?;
        if let Some(previous) = &registry.home {
            return if previous == &home { Ok(()) } else { Err("Image service owner changed.".into()) };
        }
        registry.jobs = recover_jobs(&home)?;
        registry.home = Some(home);
        Ok(())
    }

    fn home(&self) -> Result<PathBuf, String> {
        if let Some(home) = lock(&self.shared)?.home.clone() {
            return Ok(home);
        }
        let home = super::codex_home().map_err(|_| "Could not locate CODEX_HOME.".to_string())?;
        self.initialize(&home)?;
        lock(&self.shared)?.home.clone().ok_or_else(|| "Image service owner is unavailable.".into())
    }

    pub fn start(&self, request: ImageTestRequest, workspace: Option<&Path>, leased: bool)
        -> Result<ImageJob, String>
    {
        self.start_guarded(request, workspace, leased, no_submission_guard())
    }

    pub fn start_guarded(
        &self, request: ImageTestRequest, workspace: Option<&Path>, leased: bool,
        guard: ImageSubmissionGuard,
    ) -> Result<ImageJob, String> {
        self.start_with_loaders(request, workspace, leased, guard,
            load_provider_in_home, load_references_in_home)
    }

    fn start_with_loaders(
        &self, request: ImageTestRequest, workspace: Option<&Path>, leased: bool,
        guard: ImageSubmissionGuard,
        load_provider: impl FnOnce(&Path) -> Result<SavedProvider, String>,
        load_references: impl FnOnce(&[String], &Path) -> Result<Vec<Reference>, String>,
    ) -> Result<ImageJob, String> {
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        let request = validate_request(request)?;
        let home = self.home()?;
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        let provider = load_provider(&home)?;
        ensure_inputs_do_not_contain_key(&request, &provider.key)?;
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        let references = load_references(&request.reference_paths, &home)?;
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        let (id, directory) = match workspace {
            Some(workspace) => create_workspace_directory(workspace)?,
            None => create_directory(&home)?,
        };
        submit_guarded(&self.shared, id, Input { request, provider, references, directory }, leased, guard)
    }

    pub fn status(&self, id: &str) -> Result<ImageJob, String> {
        self.home()?;
        let mut registry = lock(&self.shared)?;
        let job = find_job_mut(&mut registry, id)?;
        let expired = job.expire_lease();
        job.renew_lease();
        let changed = verify_saved_items(job);
        if expired || changed {
            let _ = persist_or_stop(job);
        }
        Ok(job.snapshot())
    }

    pub fn cancel(&self, id: &str) -> Result<ImageJob, String> {
        self.home()?;
        let mut registry = lock(&self.shared)?;
        let job = find_job_mut(&mut registry, id)?;
        // A terminal completed/partial/failed job is not retroactively cancelled.
        if job.running() || job.paused || job.recovered {
            job.cancel();
            let _ = persist_or_stop(job);
        } else {
            // Invalidate a concurrent retry preflight even for terminal jobs.
            job.revision = job.revision.wrapping_add(1);
        }
        let snapshot = job.snapshot();
        worker_pool().wake.notify_all();
        Ok(snapshot)
    }

    pub fn retry(&self, id: &str) -> Result<ImageJob, String> {
        self.retry_guarded(id, no_submission_guard())
    }

    pub fn retry_guarded(&self, id: &str, guard: ImageSubmissionGuard) -> Result<ImageJob, String> {
        self.retry_with_loaders(id, guard, load_provider_in_home, load_references_in_home)
    }

    #[cfg(test)]
    fn retry_with_loader(
        &self, id: &str, load: impl FnOnce(&Path) -> Result<SavedProvider, String>,
    ) -> Result<ImageJob, String> {
        self.retry_with_loaders(id, no_submission_guard(), load, load_references_in_home)
    }

    fn retry_with_loaders(
        &self, id: &str, guard: ImageSubmissionGuard,
        load: impl FnOnce(&Path) -> Result<SavedProvider, String>,
        load_references: impl FnOnce(&[String], &Path) -> Result<Vec<Reference>, String>,
    ) -> Result<ImageJob, String> {
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        let home = self.home()?;
        let (previous, hashes, revision) = {
            let registry = lock(&self.shared)?;
            let job = find_job(&registry, id)?;
            if job.running() {
                return Err("Wait for all in-flight image requests to finish before retrying.".into());
            }
            if job.completed == job.input.request.count {
                return Err("All image indices already succeeded; nothing to retry.".into());
            }
            (Arc::clone(&job.input), job.reference_hashes.clone(), job.revision)
        };
        // Explicit retry is a new submission using the *current* saved credentials.
        // Reference content is immutable across retries, including duplicate roles.
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        let provider = load(&home)?;
        if provider.home != home {
            return Err("Retry provider does not belong to this service's CODEX_HOME.".into());
        }
        ensure_inputs_do_not_contain_key(&previous.request, &provider.key)?;
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        let references = load_references(&previous.request.reference_paths, &home)?;
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        if references.iter().map(|reference| hash(&reference.bytes)).collect::<Vec<_>>() != hashes {
            return Err("Original reference hashes changed; create a new explicit image job.".into());
        }
        verify_directory(&previous.directory)?;
        let mut registry = lock(&self.shared)?;
        if registry.shutdown {
            return Err("Image jobs are shutting down.".into());
        }
        ensure_workers(&self.shared, &mut registry)?;
        let job = find_job_mut(&mut registry, id)?;
        if job.revision != revision {
            return Err("Image job changed during retry preflight; no new request was queued.".into());
        }
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        verify_saved_items(job);
        let mut input = (*previous).clone();
        input.provider = provider;
        input.references = references;
        check_submission_guard(&guard).map_err(|failure| failure.message)?;
        job.input = Arc::new(input);
        job.submission_guard = guard;
        job.retry()?;
        let snapshot = job.snapshot();
        worker_pool().wake.notify_all();
        Ok(snapshot)
    }

    /// Stops queued work and future starts. Already admitted HTTP requests finish.
    pub fn cancel_all(&self) {
        if let Ok(mut registry) = lock(&self.shared) {
            registry.shutdown = true;
            for job in registry.jobs.values_mut() {
                if job.running() || job.paused || job.recovered {
                    job.cancel();
                    let _ = persist_or_stop(job);
                }
            }
        }
        worker_pool().wake.notify_all();
    }

    pub fn has_active_requests(&self) -> bool {
        lock(&self.shared).map(|registry| registry.jobs.values().any(|job| job.active > 0))
            .unwrap_or(true)
    }
}

#[tauri::command]
pub async fn get_image_test_status(
    job_id: String,
    state: State<'_, ImageJobs>,
) -> Result<ImageJob, String> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.status(&job_id))
        .await.map_err(|_| "Image status task failed.".to_string())?
}

#[tauri::command]
pub async fn cancel_image_test(
    job_id: String,
    state: State<'_, ImageJobs>,
) -> Result<ImageJob, String> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.cancel(&job_id))
        .await.map_err(|_| "Image cancellation task failed.".to_string())?
}

#[tauri::command]
pub async fn retry_image_test(
    job_id: String,
    state: State<'_, ImageJobs>,
) -> Result<ImageJob, String> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.retry(&job_id))
        .await.map_err(|_| "Image retry task failed.".to_string())?
}

#[tauri::command]
pub async fn open_image_result(
    job_id: String,
    index: usize,
    state: State<'_, ImageJobs>,
) -> Result<(), String> {
    let shared = Arc::clone(&state.shared);
    tauri::async_runtime::spawn_blocking(move || {
        let (directory, item) = {
            let registry = lock(&shared)?;
            let job = find_job(&registry, &job_id)?;
            let item = job.items.get(&index).cloned()
                .ok_or_else(|| "No recorded result for this index.".to_string())?;
            (job.input.directory.clone(), item)
        };
        let path = recorded_path(&directory, &item)?;
        verify_item(&directory, &item)?;
        super::open_path(&path).map_err(|_| "Could not open the recorded image file.".into())
    }).await.map_err(|_| "Image open task failed.".to_string())?
}

fn validate_model(model: &str) -> Result<String, String> {
    let model = model.trim();
    if model.is_empty() || model.len() > 256
        || model.chars().any(|c| c.is_control() || c.is_whitespace())
        || model.contains([':', '\\', '?', '#'])
        || model.starts_with('/') || model.split('/').any(|part| part == "..")
    {
        return Err("Model must be a nonblank model identifier, not a URL or filesystem path.".into());
    }
    Ok(model.into())
}

fn ensure_no_credential_in_values<'a>(
    values: impl IntoIterator<Item = &'a str>,
    key: &str,
) -> Result<(), String> {
    let key = key.trim();
    if !key.is_empty() && values.into_iter().any(|value| value.contains(key)) {
        return Err("An image input contains the saved API credential. Remove it before continuing.".into());
    }
    Ok(())
}

fn ensure_inputs_do_not_contain_key(request: &ImageTestRequest, key: &str) -> Result<(), String> {
    ensure_no_credential_in_values(
        [request.model.as_str(), request.prompt.as_str(), request.size.as_str()]
            .into_iter().chain(request.reference_paths.iter().map(String::as_str)),
        key,
    )?;
    ensure_no_credential_in_values(
        request.prompts.iter().map(String::as_str),
        key,
    )
}

fn validate_request(mut request: ImageTestRequest) -> Result<ImageTestRequest, String> {
    request.model = validate_model(&request.model)?;
    if request.count == 0 {
        return Err("Image count must be a positive integer.".into());
    }
    if request.prompt.len() > 256 * 1024 {
        return Err("Prompt must be at most 256 KiB.".into());
    }
    if request.prompts.is_empty() {
        if request.prompt.trim().is_empty() {
            return Err("Prompt must be nonblank, or supply one prompt per image slot.".into());
        }
    } else if request.prompts.len() != request.count
        || request.prompts.iter().any(|prompt| prompt.trim().is_empty() || prompt.len() > 256 * 1024)
    {
        return Err("prompts must contain exactly count nonblank prompts of at most 256 KiB each.".into());
    }
    request.size = request.size.trim().into();
    if request.size.is_empty() {
        request.size = default_size();
    }
    if request.size != "auto" {
        let valid = request.size.split_once('x').is_some_and(|(w, h)| {
            matches!((w.parse::<u32>(), h.parse::<u32>()),
                (Ok(w), Ok(h)) if w > 0 && h > 0 && w <= MAX_DIMENSION && h <= MAX_DIMENSION)
        });
        if !valid {
            return Err("Size must be blank, auto, or positive WIDTHxHEIGHT (up to 16384 per side).".into());
        }
    }
    Ok(request)
}

fn normalize_api_base(base: &str) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    super::validate_base_url(base).map_err(|_| "Saved provider base URL is invalid.".to_string())?;
    let url = Url::parse(base).map_err(|_| "Saved provider base URL is invalid.".to_string())?;
    Ok(if url.path().trim_matches('/').is_empty() { format!("{base}/v1") } else { base.into() })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProviderTransport {
    Https,
    LoopbackHttp,
}

fn provider_transport(base: &str, key: &str) -> Result<ProviderTransport, String> {
    super::validate_base_url(base).map_err(|_| "Saved provider base URL is invalid.".to_string())?;
    let url = Url::parse(base).map_err(|_| "Saved provider base URL is invalid.".to_string())?;
    if url.scheme() == "https" {
        return Ok(ProviderTransport::Https);
    }
    // Numeric hosts never invoke DNS. IpAddr::is_loopback excludes mapped IPv6,
    // private networks and wildcard listeners; no localhost/domain exception.
    let loopback = url.host_str().and_then(|host|
        host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>().ok()
    ).is_some_and(|ip| ip.is_loopback());
    if url.scheme() == "http" && loopback && key == LOCAL_FIXTURE_KEY {
        return Ok(ProviderTransport::LoopbackHttp);
    }
    Err("Image providers require HTTPS. HTTP fixtures require a numeric loopback address \
        and the reserved local fixture credential.".into())
}

fn checked_provider_transport(provider: &SavedProvider) -> Result<ProviderTransport, Failure> {
    #[cfg(test)]
    if provider.local_mock {
        return Ok(ProviderTransport::LoopbackHttp);
    }
    provider_transport(&provider.api_base, &provider.key).map_err(|message| Failure {
        stop_job: true,
        ..Failure::not_sent(message)
    })
}

fn load_provider() -> Result<SavedProvider, String> {
    let home = super::codex_home().map_err(|_| "Could not locate CODEX_HOME.".to_string())?;
    load_provider_in_home(&home)
}

fn load_provider_in_home(home: &Path) -> Result<SavedProvider, String> {
    let home = fs::canonicalize(home).map_err(|_| "CODEX_HOME does not exist.".to_string())?;
    let config = fs::read_to_string(home.join("config.toml"))
        .map_err(|_| "Could not read the saved provider configuration.".to_string())?;
    let document = config.parse::<toml_edit::DocumentMut>()
        .map_err(|_| "Saved provider configuration is not valid TOML.".to_string())?;
    // The legacy root-string helper also scans table entries. Verify the real
    // TOML root as well, so a nested model_provider cannot activate paid tests.
    if super::read_root_string(&config, "model_provider").as_deref() != Some(super::PROVIDER_ID)
        || document.get("model_provider").and_then(toml_edit::Item::as_str) != Some(super::PROVIDER_ID)
        || document.get("model_providers").and_then(|providers| providers.get(super::PROVIDER_ID))
            .and_then(toml_edit::Item::as_table_like).is_none()
    {
        return Err("Image API tests require the active saved OceanWay provider.".into());
    }
    let base = super::read_provider_base_url(&config, super::PROVIDER_ID)
        .unwrap_or_else(|| super::DEFAULT_BASE_URL.into());
    if let Some(value) = document.get("model_providers")
        .and_then(|providers| providers.get(super::PROVIDER_ID))
        .and_then(|provider| provider.get("base_url"))
    {
        if value.as_str() != Some(base.as_str()) {
            return Err("Saved provider URL cannot be resolved unambiguously.".into());
        }
    }
    let api_base = normalize_api_base(&base)?;
    // Native MCP does not require a shell environment variable. auth.json is
    // authoritative when present; legacy inline bearer tokens remain supported.
    let auth_path = home.join("auth.json");
    let key = match fs::symlink_metadata(&auth_path) {
        Ok(_) => super::read_auth_api_key(&auth_path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound =>
            super::read_provider_bearer_token(&config, super::PROVIDER_ID),
        Err(_) => return Err("Could not read saved auth.json credentials.".into()),
    }.filter(|key| !key.trim().is_empty())
        .ok_or_else(|| "Save an OceanWay API key in auth.json before running image requests.".to_string())?;
    provider_transport(&api_base, &key)?;
    // Do not pair a key read from a changed config with an earlier endpoint snapshot.
    if fs::read_to_string(home.join("config.toml")).ok().as_ref() != Some(&config) {
        return Err("Provider configuration changed while loading; try again.".into());
    }
    Ok(SavedProvider {
        home,
        config,
        api_base,
        key,
        #[cfg(test)]
        local_mock: false,
    })
}

#[cfg(test)]
fn compare_provider(previous: &SavedProvider, current: &SavedProvider) -> Result<(), String> {
    if current.home != previous.home || current.config != previous.config
        || current.api_base != previous.api_base || current.key != previous.key
    {
        return Err("Saved provider or credentials changed; create a new explicit image test.".into());
    }
    Ok(())
}

fn bounded_read(reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let maximum = u64::try_from(limit).ok().and_then(|n| n.checked_add(1))
        .ok_or_else(|| "Invalid byte limit.".to_string())?;
    let mut bytes = Vec::new();
    reader.take(maximum).read_to_end(&mut bytes)
        .map_err(|_| "Could not read image or response bytes; billing may be unknown.".to_string())?;
    if bytes.len() > limit {
        return Err("Image or response exceeds the byte limit.".into());
    }
    Ok(bytes)
}

fn decode_image(bytes: &[u8]) -> Result<(DynamicImage, ImageFormat), String> {
    if bytes.is_empty() || bytes.len() > IMAGE_BYTES {
        return Err("Image must contain 1 byte to 50 MiB.".into());
    }
    let format = image::guess_format(bytes).map_err(|_| "Unrecognized image content.".to_string())?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP) {
        return Err("Only decoded PNG, JPEG, and WebP images are accepted.".into());
    }
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions().map_err(|_| "Invalid image dimensions.".to_string())?;
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err("Image exceeds safe decoding dimensions (16384 per side, 64M pixels).".into());
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| "Image decoding or integrity validation failed.".to_string())?;
    Ok((image, format))
}

fn format_info(format: ImageFormat) -> (&'static str, &'static str) {
    match format {
        ImageFormat::Png => ("image/png", "png"),
        ImageFormat::Jpeg => ("image/jpeg", "jpeg"),
        ImageFormat::WebP => ("image/webp", "webp"),
        _ => unreachable!("only validated image formats"),
    }
}

fn load_references(paths: &[String]) -> Result<Vec<Reference>, String> {
    let mut references = Vec::new();
    let mut total = 0usize;
    for (index, path) in paths.iter().enumerate() {
        let result = (|| {
            validate_reference_path(Path::new(path), None)?;
            let file = File::open(path).map_err(|_| "Reference file cannot be opened.".to_string())?;
            let metadata = file.metadata().map_err(|_| "Reference metadata is unavailable.".to_string())?;
            if !metadata.is_file() || metadata.len() > IMAGE_BYTES as u64 {
                return Err("Reference must be a regular file of at most 50 MiB.".into());
            }
            // Check the aggregate before reading the next large file.
            if metadata.len() > (REFERENCE_BYTES - total) as u64 {
                return Err("Reference images exceed the 256 MiB memory budget.".into());
            }
            let bytes = bounded_read(file, IMAGE_BYTES.min(REFERENCE_BYTES - total))?;
            let (_, format) = decode_image(&bytes)?;
            let (mime, extension) = format_info(format);
            total = total.checked_add(bytes.len()).ok_or_else(|| "Reference byte overflow.".to_string())?;
            Ok(Reference { bytes: bytes.into(), mime, extension })
        })();
        references.push(result.map_err(|error: String| format!("Reference {}: {error}", index + 1))?);
    }
    Ok(references)
}

fn load_references_in_home(paths: &[String], home: &Path) -> Result<Vec<Reference>, String> {
    for path in paths {
        validate_reference_path(Path::new(path), Some(home))?;
    }
    load_references(paths)
}

fn validate_reference_path(path: &Path, home: Option<&Path>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "Reference original is unavailable.".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Reference original must be a regular file, not a link or directory.".into());
    }
    let resolved = fs::canonicalize(path).map_err(|_| "Reference path cannot be resolved.".to_string())?;
    for candidate in [path, resolved.as_path()] {
        let name = candidate.file_name().and_then(|name| name.to_str()).unwrap_or("").to_ascii_lowercase();
        if name.starts_with("config.toml") || name.starts_with("auth.json")
            || name.starts_with(".env") || matches!(name.as_str(), "credentials" | "credentials.json" | "id_rsa" | "id_ed25519")
            || candidate.components().any(|part| part.as_os_str().to_string_lossy().eq_ignore_ascii_case(".ssh"))
        {
            return Err("Credential/configuration paths cannot be image references.".into());
        }
    }
    if let Some(home) = home {
        if resolved == home.join("config.toml") || resolved == home.join("auth.json") {
            return Err("Credential/configuration paths cannot be image references.".into());
        }
    }
    Ok(())
}

fn create_directory(home: &Path) -> Result<(String, PathBuf), String> {
    let root = home.join("oceanway-image-tests");
    fs::create_dir_all(&root).map_err(|_| "Could not create image output root.".to_string())?;
    let metadata = fs::symlink_metadata(&root).map_err(|_| "Output root is unavailable.".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Image output root must be a real directory.".into());
    }
    let root = fs::canonicalize(&root).map_err(|_| "Could not resolve output root.".to_string())?;
    let canonical_home = fs::canonicalize(home).map_err(|_| "Could not resolve CODEX_HOME.".to_string())?;
    if root.parent() != Some(canonical_home.as_path()) {
        return Err("Image output root escaped CODEX_HOME.".into());
    }
    allocate_directory(&root)
}

fn create_workspace_directory(workspace: &Path) -> Result<(String, PathBuf), String> {
    let workspace = fs::canonicalize(workspace).map_err(|_| "Task workspace does not exist.".to_string())?;
    verify_directory(&workspace)?;
    let mut root = workspace;
    for segment in ["output", "images"] {
        root = root.join(segment);
        match fs::create_dir(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("Could not create task image output directory.".into()),
        }
        verify_directory(&root)?;
    }
    allocate_directory(&root)
}

fn allocate_directory(root: &Path) -> Result<(String, PathBuf), String> {
    for _ in 0..16 {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let id = format!("image-{}-{stamp}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed));
        let directory = root.join(&id);
        match fs::create_dir(&directory) {
            Ok(()) => return Ok((id, directory)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("Could not create a unique image output directory.".into()),
        }
    }
    Err("Could not allocate a unique image job ID.".into())
}

fn verify_directory(directory: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(directory).map_err(|_| "Output directory is missing.".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir()
        || fs::canonicalize(directory).ok().as_deref() != Some(directory)
    {
        return Err("Output directory no longer matches the recorded directory.".into());
    }
    Ok(())
}

fn recorded_path(directory: &Path, item: &ImageItem) -> Result<PathBuf, String> {
    if item.status != "succeeded" || item.index == 0 {
        return Err("Only a successful recorded image can be opened.".into());
    }
    verify_directory(directory)?;
    let path = PathBuf::from(item.path.as_ref().ok_or_else(|| "Missing recorded image path.".to_string())?);
    let extension = path.extension().and_then(|part| part.to_str()).unwrap_or("");
    if !matches!(extension, "png" | "jpeg" | "webp")
        || path != directory.join(format!("{}.{}", item.index, extension))
    {
        return Err("Image path does not match its recorded index.".into());
    }
    let metadata = fs::symlink_metadata(&path).map_err(|_| "Recorded image is missing.".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file()
        || fs::canonicalize(&path).ok().as_ref() != Some(&path)
    {
        return Err("Recorded image path is not a regular contained file.".into());
    }
    Ok(path)
}

#[derive(Debug)]
struct Failure {
    message: String,
    request_id: Option<String>,
    stop_job: bool,
    not_sent: bool,
    count_mismatch: bool,
}

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), request_id: None, stop_job: false, not_sent: false, count_mismatch: false }
    }

    fn not_sent(message: impl Into<String>) -> Self {
        Self { not_sent: true, ..Self::new(message) }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

struct SavedResult {
    path: PathBuf,
    preview: String,
    request_id: Option<String>,
    warning: Option<String>,
    additional_paths: Vec<String>,
    outputs: Vec<ImageOutput>,
    count_mismatch: bool,
}

fn client_builder(timeout: Duration) -> ClientBuilder {
    Client::builder()
        .use_rustls_tls()
        .redirect(Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .no_proxy()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(15).min(timeout))
}

fn provider_client(provider: &SavedProvider) -> Result<Client, Failure> {
    let https_only = checked_provider_transport(provider)? == ProviderTransport::Https;
    client_builder(TIMEOUT).https_only(https_only).build()
        .map_err(|_| Failure::new("Could not initialize the image HTTP client."))
}

fn network_failure(error: &reqwest::Error) -> Failure {
    if error.is_timeout() {
        Failure::new("Image request timed out (240s budget); billing is unknown. No automatic retry.")
    } else if error.is_connect() {
        Failure::new("Could not connect to the image endpoint. No automatic retry; billing may be unknown.")
    } else {
        Failure::new("Image HTTP request failed. No automatic retry; billing may be unknown.")
    }
}

fn status_failure(status: reqwest::StatusCode) -> Failure {
    let explanation = match status.as_u16() {
        401 => "Authentication rejected; check the saved OceanWay API key",
        403 => "Access denied; check model and account permissions",
        404 => "Image endpoint or model was not found",
        413 => "Provider rejected the image payload size",
        429 => "Provider rate limit or quota was reached",
        400 | 422 => "Provider rejected the model, prompt, references, or size",
        300..=399 => "Redirect refused; credentials were not forwarded",
        500..=599 => "Provider service failed; billing may be unknown",
        _ => "Image endpoint returned an unsuccessful status",
    };
    Failure::new(format!("HTTP {}: {explanation}. No automatic retry.", status.as_u16()))
}

fn safe_request_id(response: &Response, key: &str) -> Option<String> {
    let id = response.headers().get("x-request-id")
        .or_else(|| response.headers().get("request-id"))?.to_str().ok()?;
    if id.is_empty() || id.len() > 128 || (!key.is_empty() && id.contains(key))
        || !id.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return None;
    }
    Some(id.into())
}

fn json_response(response: Response, key: &str, limit: usize) -> Result<(Value, Option<String>), Failure> {
    let request_id = safe_request_id(&response, key);
    let result = (|| {
        if !response.status().is_success() {
            // Never reflect provider error bodies, signed URLs, or raw reqwest errors.
            return Err(status_failure(response.status()));
        }
        let bytes = bounded_read(response, limit)?;
        let body = serde_json::from_slice(&bytes)
            .map_err(|_| Failure::new("Image endpoint returned invalid JSON."))?;
        Ok(body)
    })();
    result.map(|body| (body, request_id.clone())).map_err(|mut failure: Failure| {
        failure.request_id = request_id;
        failure
    })
}

fn load_post_provider(provider: &SavedProvider) -> Result<SavedProvider, Failure> {
    #[cfg(test)]
    if provider.local_mock {
        return Ok(provider.clone());
    }
    load_provider_in_home(&provider.home).map_err(|message| Failure {
        message: format!("Current saved OceanWay provider is unavailable; no POST was sent. {message}"),
        stop_job: true,
        ..Failure::not_sent("")
    })
}

#[cfg(test)]
fn provider_guard_result(result: Result<(), String>) -> Result<(), Failure> {
    result.map_err(|message| Failure {
        message: format!(
            "Saved provider changed or is unavailable; no POST was sent for this index. \
             Remaining queued work is cancelled. {message}"
        ),
        request_id: None,
        stop_job: true,
        not_sent: false,
        count_mismatch: false,
    })
}

#[cfg(test)]
fn post_image_with_guard(
    input: &Input,
    guard: impl FnOnce() -> Result<(), Failure>,
) -> Result<(Value, Option<String>), Failure> {
    post_image_at_with_guard(input, 1, Instant::now(), || {
        guard()?;
        Ok(input.provider.clone())
    })
}

fn post_image_at_with_guard(
    input: &Input,
    index: usize,
    started: Instant,
    guard: impl FnOnce() -> Result<SavedProvider, Failure>,
) -> Result<(Value, Option<String>), Failure> {
    let client = provider_client(&input.provider)?;
    let request = &input.request;
    let prompt = if request.prompts.is_empty() {
        &request.prompt
    } else {
        request.prompts.get(index.wrapping_sub(1))
            .ok_or_else(|| Failure::not_sent("Missing prompt for the requested image slot."))?
    };
    let editing = !input.references.is_empty();
    let endpoint = format!("{}/images/{}", input.provider.api_base,
        if editing { "edits" } else { "generations" });
    let builder = client.post(endpoint);
    let builder = if editing {
        let mut form = multipart::Form::new()
            .text("model", request.model.clone())
            .text("prompt", prompt.clone())
            .text("n", "1");
        if !request.size.is_empty() {
            form = form.text("size", request.size.clone());
        }
        for (index, reference) in input.references.iter().enumerate() {
            let part = multipart::Part::reader_with_length(
                Cursor::new(Arc::clone(&reference.bytes)), reference.bytes.len() as u64,
            )
            .file_name(format!("reference-{}.{}", index + 1, reference.extension))
            .mime_str(reference.mime).map_err(|_| Failure::new("Invalid reference MIME type."))?;
            form = form.part("image[]", part);
        }
        builder.multipart(form)
    } else {
        let mut body = json!({ "model": request.model, "prompt": prompt, "n": 1 });
        if !request.size.is_empty() {
            body["size"] = Value::String(request.size.clone());
        }
        builder.json(&body)
    };
    // Build the body first, then re-read the active config/key immediately before
    // every paid POST. Already in-flight requests retain their original snapshot.
    let mut request = builder.build().map_err(|_| Failure::not_sent("Could not build the image request."))?;
    let provider = guard()?;
    // The body contains no credentials. Bind the latest endpoint/key only after
    // final admission, so auth.json updates during preflight cannot use an old key.
    let endpoint = format!("{}/images/{}", provider.api_base, if editing { "edits" } else { "generations" });
    *request.url_mut() = Url::parse(&endpoint).map_err(|_| Failure::not_sent("Invalid saved image endpoint."))?;
    let authorization = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", provider.key))
        .map_err(|_| Failure::not_sent("Saved API credential is not a valid HTTP header."))?;
    request.headers_mut().insert(reqwest::header::AUTHORIZATION, authorization);
    *request.timeout_mut() = Some(remaining(started)?);
    let response = client.execute(request).map_err(|error| network_failure(&error))?;
    json_response(response, &provider.key, JSON_BYTES)
}

#[cfg(test)]
fn execute(input: &Input, index: usize, started: Instant) -> Result<SavedResult, Failure> {
    execute_with_guard(input, index, started, || load_post_provider(&input.provider))
}

fn execute_with_guard(
    input: &Input,
    index: usize,
    started: Instant,
    guard: impl FnOnce() -> Result<SavedProvider, Failure>,
) -> Result<SavedResult, Failure> {
    verify_unused_slot(input, index)?;
    let (body, request_id) = post_image_at_with_guard(input, index, started, guard)?;
    save_response_images(input, index, &body, request_id, started)
}

fn verify_unused_slot(input: &Input, index: usize) -> Result<(), Failure> {
    // Detect an altered output directory before sending a billable request.
    verify_directory(&input.directory)?;
    if index == 0 || index > input.request.count {
        return Err(Failure::new("Image index is outside the recorded job."));
    }
    for extension in ["png", "jpeg", "webp"] {
        match fs::symlink_metadata(input.directory.join(format!("{index}.{extension}"))) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(Failure::new(
                "Output index already exists or is inaccessible; no image request was sent.",
            )),
        }
    }
    Ok(())
}

fn save_image_bytes(input: &Input, index: usize, extra: Option<usize>, bytes: &[u8])
    -> Result<(PathBuf, String, ImageOutput), Failure>
{
    let (decoded, format) = decode_image(bytes)?;
    let preview = if extra.is_none() {
        let mut thumbnail = Cursor::new(Vec::new());
        decoded.thumbnail(256, 256).write_to(&mut thumbnail, ImageFormat::Png)
            .map_err(|_| Failure::new("Could not encode the image thumbnail."))?;
        format!("data:image/png;base64,{}", STANDARD.encode(thumbnail.into_inner()))
    } else {
        String::new()
    };
    let (_, extension) = format_info(format);
    verify_directory(&input.directory)?;
    let filename = match extra {
        Some(position) => format!("{index}-extra-{position}.{extension}"),
        None => format!("{index}.{extension}"),
    };
    let mut path = input.directory.join(filename);
    let mut collisions = 0;
    let mut file = loop {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break file,
            Err(error) => {
                if let Some(position) = extra {
                    if error.kind() == std::io::ErrorKind::AlreadyExists && collisions < 16 {
                        collisions += 1;
                        path = input.directory.join(format!(
                            "{index}-extra-{position}-{}.{}",
                            SEQUENCE.fetch_add(1, Ordering::Relaxed), extension,
                        ));
                        continue;
                    }
                }
                return Err(Failure::new("Could not create the output image without overwriting a file."));
            }
        }
    };
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(Failure::new("Could not persist the output image; billing may have occurred."));
    }
    let output = ImageOutput {
        path: path.to_string_lossy().into_owned(),
        width: decoded.width(),
        height: decoded.height(),
        sha256: hash(bytes),
        bytes: bytes.len() as u64,
    };
    Ok((path, preview, output))
}

fn save_response_images(
    input: &Input,
    index: usize,
    body: &Value,
    request_id: Option<String>,
    started: Instant,
) -> Result<SavedResult, Failure> {
    let result = (|| {
        let data = body.get("data").and_then(Value::as_array).ok_or_else(|| {
            let mut failure = Failure::new("Expected an image for n=1; data array is missing.");
            failure.count_mismatch = true;
            failure
        })?;
        let mut primary: Option<(PathBuf, String)> = None;
        let mut additional_paths = Vec::new();
        let mut outputs = Vec::new();
        let mut unusable = 0usize;
        let mut first_problem = None;
        for (offset, entry) in data.iter().enumerate() {
            let saved = response_entry_image(entry, started).and_then(|bytes| {
                save_image_bytes(input, index, primary.as_ref().map(|_| offset + 1), &bytes)
            });
            match saved {
                Ok((path, preview, output)) => {
                    outputs.push(output);
                    if primary.is_none() {
                        primary = Some((path, preview));
                    } else {
                        additional_paths.push(path.to_string_lossy().into_owned());
                    }
                }
                Err(failure) => {
                    unusable += 1;
                    if first_problem.is_none() {
                        first_problem = Some(format!("Response item {}: {}", offset + 1, failure.message));
                    }
                }
            }
        }
        let (path, preview) = primary.ok_or_else(|| {
            let mut failure = Failure::new(format!(
            "Expected one image for n=1; received {} entries but no image could be saved. {} \
             Billing may have occurred. No automatic retry.",
            data.len(), first_problem.as_deref().unwrap_or("The response contained no images.")
            ));
            failure.count_mismatch = data.len() != 1;
            failure
        })?;
        let provider_error = body.get("error").is_some_and(|error| !error.is_null());
        let warning = if data.len() != 1 || unusable > 0 || provider_error {
            Some(format!(
                "Expected one image for n=1; provider returned {} entries. Saved {} valid image(s); \
                 {unusable} unusable entry/entries. Additional images are preserved as separate files. \
                 No extra POST was sent and this successful index will not be retried.{}{}",
                data.len(), 1 + additional_paths.len(),
                first_problem.map(|problem| format!(" {problem}")).unwrap_or_default(),
                if provider_error { " An accompanying provider error was withheld; valid images were retained." } else { "" }
            ))
        } else {
            None
        };
        Ok(SavedResult {
            path, preview, request_id: request_id.clone(), warning, additional_paths,
            outputs, count_mismatch: data.len() != 1,
        })
    })();
    result.map_err(|mut failure: Failure| {
        failure.request_id = request_id;
        failure
    })
}

fn response_entry_image(entry: &Value, started: Instant) -> Result<Vec<u8>, Failure> {
    response_entry_with_download(entry, started, download_image)
}

fn response_entry_with_download(
    entry: &Value,
    started: Instant,
    download: impl FnOnce(&str, Instant) -> Result<Vec<u8>, Failure>,
) -> Result<Vec<u8>, Failure> {
    let encoded = entry.get("b64_json").and_then(Value::as_str).filter(|s| !s.is_empty());
    let url = entry.get("url").and_then(Value::as_str).filter(|s| !s.is_empty());
    let mut base64_error = None;
    if let Some(encoded) = encoded {
        let decoded = (|| {
            if encoded.len() > IMAGE_BYTES.div_ceil(3) * 4 {
                return Err(Failure::new("Base64 image exceeds the 50 MiB limit."));
            }
            let bytes = STANDARD.decode(encoded).map_err(|_| Failure::new("Invalid base64 image."))?;
            if bytes.len() > IMAGE_BYTES {
                return Err(Failure::new("Decoded image exceeds the 50 MiB limit."));
            }
            decode_image(&bytes)?;
            Ok::<_, Failure>(bytes)
        })();
        match decoded {
            Ok(bytes) => return Ok(bytes),
            Err(error) => base64_error = Some(error),
        }
    }
    if let Some(url) = url {
        let bytes = download(url, started)?;
        decode_image(&bytes)?;
        return Ok(bytes);
    }
    Err(base64_error.unwrap_or_else(|| Failure::new("Image result contains neither usable URL nor base64.")))
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0 || a == 10 || a == 127 || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254) || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || (b == 0 && (c == 0 || c == 2)) || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            // Conservative global unicast only; exclude transition/special ranges.
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && segments[1] < 0x1000)
        }
    }
}

fn download_url(raw: &str) -> Result<Url, Failure> {
    if raw.len() > 16 * 1024 || raw.chars().any(char::is_control) {
        return Err(Failure::new("Invalid image download URL."));
    }
    let url = Url::parse(raw).map_err(|_| Failure::new("Invalid image download URL."))?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
        || url.fragment().is_some() || url.port().is_some_and(|port| port != 443)
    {
        return Err(Failure::new("Image downloads require HTTPS on port 443 without user information."));
    }
    let host = url.host_str().ok_or_else(|| Failure::new("Missing download host."))?
        .trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !public_ip(ip) {
            return Err(Failure::new("Non-public image download address rejected."));
        }
    } else {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if !host.contains('.') || host.ends_with(".localhost") || host.ends_with(".local")
            || host.ends_with(".internal") || host.ends_with(".home.arpa")
        {
            return Err(Failure::new("Non-public image download host rejected."));
        }
    }
    Ok(url)
}

fn remaining(started: Instant) -> Result<Duration, Failure> {
    TIMEOUT.checked_sub(started.elapsed()).filter(|duration| !duration.is_zero())
        .ok_or_else(|| Failure::new("Image attempt exceeded 240s; billing may have occurred."))
}

fn resolve_public(url: &Url, timeout: Duration) -> Result<Vec<SocketAddr>, Failure> {
    let host = url.host_str().ok_or_else(|| Failure::new("Missing download host."))?
        .trim_start_matches('[').trim_end_matches(']').to_string();
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if public_ip(ip) { Ok(vec![SocketAddr::new(ip, 443)]) }
            else { Err(Failure::new("Non-public image download address rejected.")) };
    }
    let (send, receive) = mpsc::sync_channel(1);
    thread::Builder::new().name("image-download-dns".into()).spawn(move || {
        let result = (host.as_str(), 443).to_socket_addrs().map(|addresses| {
            addresses.take(65).collect::<Vec<_>>()
        });
        let _ = send.send(result);
    }).map_err(|_| Failure::new("Could not start image download DNS lookup."))?;
    let addresses = receive.recv_timeout(timeout.min(Duration::from_secs(5)))
        .map_err(|_| Failure::new("Image download DNS lookup timed out."))?
        .map_err(|_| Failure::new("Image download DNS lookup failed."))?;
    if addresses.is_empty() || addresses.len() > 64 || addresses.iter().any(|addr| !public_ip(addr.ip())) {
        return Err(Failure::new("Image download DNS returned non-public or excessive addresses."));
    }
    Ok(addresses)
}

fn download_image(raw: &str, started: Instant) -> Result<Vec<u8>, Failure> {
    let url = download_url(raw)?;
    let addresses = resolve_public(&url, remaining(started)?)?;
    let host = url.host_str().ok_or_else(|| Failure::new("Missing download host."))?;
    // A fresh client has no provider Authorization/cookies. Pin the validated DNS
    // answer and disable proxies/redirects so it cannot be resolved again elsewhere.
    let client = client_builder(remaining(started)?).https_only(true)
        .resolve_to_addrs(host, &addresses).build()
        .map_err(|_| Failure::new("Could not initialize the image download client."))?;
    download_from(&client, url, &addresses)
}

fn download_from(client: &Client, url: Url, addresses: &[SocketAddr]) -> Result<Vec<u8>, Failure> {
    let response = client.get(url).send().map_err(|err| network_failure(&err))?;
    if !response.status().is_success() {
        return Err(Failure::new(format!("Image download returned HTTP {}; redirects are disabled.",
            response.status().as_u16())));
    }
    if response.remote_addr().is_none_or(|peer| !addresses.iter().any(|addr| addr.ip() == peer.ip())) {
        return Err(Failure::new("Image download peer did not match validated DNS addresses."));
    }
    if response.content_length().is_some_and(|length| length > IMAGE_BYTES as u64) {
        return Err(Failure::new("Image download exceeds 50 MiB."));
    }
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE)
        .and_then(|header| header.to_str().ok()).unwrap_or("")
        .split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let bytes = bounded_read(response, IMAGE_BYTES)?;
    let (_, format) = decode_image(&bytes)?;
    let (mime, _) = format_info(format);
    if !content_type.is_empty() && content_type != "application/octet-stream" && content_type != mime {
        return Err(Failure::new("Downloaded image MIME type does not match decoded content."));
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "image_api_tests.rs"]
mod tests;
