//! Owned index/readback only. No directory discovery, provider loading, or HTTP.
use super::*;

pub(super) const ENGINE_ID: &str = "oceanway-shared-image-engine";
const STORE_DIRECTORY: &str = "oceanway-image-jobs";
static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnedManifest {
    directory: PathBuf,
    ownership_token: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EngineIndex {
    schema_version: u32,
    engine: String,
    codex_home: PathBuf,
    jobs: BTreeMap<String, OwnedManifest>,
}

fn index_directory(home: &Path, create: bool) -> Result<Option<PathBuf>, String> {
    verify_directory(home)?;
    let directory = home.join(STORE_DIRECTORY);
    if create {
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("Could not create the owned image index directory.".into()),
        }
    }
    match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Image index directory is unavailable.".into()),
        Ok(_) => {}
    }
    verify_directory(&directory)?;
    Ok(Some(directory))
}

fn read_regular(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "Owned image record/file is missing.".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file()
        || fs::canonicalize(path).ok().as_deref() != Some(path)
        || metadata.len() > limit as u64
    {
        return Err("Owned image record/file is not a regular contained file within its size limit.".into());
    }
    bounded_read(File::open(path).map_err(|_| "Could not open owned image record/file.".to_string())?, limit)
}

fn read_index(home: &Path, directory: &Path) -> Result<EngineIndex, String> {
    let path = directory.join("index.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(EngineIndex {
            schema_version: 1,
            engine: ENGINE_ID.into(),
            codex_home: home.into(),
            jobs: BTreeMap::new(),
        }),
        Err(_) => return Err("Owned image index is unavailable.".into()),
        Ok(_) => {}
    }
    let index: EngineIndex = serde_json::from_slice(&read_regular(&path, JSON_BYTES)?)
        .map_err(|_| "Owned image index is invalid JSON.".to_string())?;
    if index.schema_version != 1 || index.engine != ENGINE_ID || index.codex_home != home {
        return Err("Image index ownership/schema does not match the explicit CODEX_HOME.".into());
    }
    Ok(index)
}

fn write_index(directory: &Path, index: &EngineIndex) -> Result<(), String> {
    verify_directory(directory)?;
    let temporary = directory.join(format!(".index-{}.tmp", SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)
        .map_err(|_| "Could not create the owned image index record.".to_string())?;
    let result = (|| {
        serde_json::to_writer_pretty(&mut file, index)
            .map_err(|_| "Could not write the owned image index.".to_string())?;
        file.write_all(b"\n").and_then(|_| file.sync_all())
            .map_err(|_| "Could not flush the owned image index.".to_string())?;
        drop(file);
        fs::rename(&temporary, directory.join("index.json"))
            .map_err(|_| "Could not replace the owned image index.".to_string())?;
        #[cfg(unix)]
        File::open(directory).and_then(|directory| directory.sync_all())
            .map_err(|_| "Could not flush the owned image index directory.".to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn register_manifest(job: &StoredJob) -> Result<(), String> {
    let _guard = STORE_LOCK.lock().map_err(|_| "Owned image index is unavailable.".to_string())?;
    let home = &job.input.provider.home;
    let directory = index_directory(home, true)?.ok_or_else(|| "Missing image index directory.".to_string())?;
    let mut index = read_index(home, &directory)?;
    if index.jobs.contains_key(&job.id) {
        return Err("An owned image index entry already exists for this job.".into());
    }
    index.jobs.insert(job.id.clone(), OwnedManifest {
        directory: job.input.directory.clone(),
        ownership_token: job.ownership_token.clone(),
    });
    write_index(&directory, &index)
}

fn validate_owned_directory(home: &Path, id: &str, entry: &OwnedManifest) -> Result<(), String> {
    if !id.starts_with("image-") || id.len() > 160
        || !id[6..].bytes().all(|byte| byte.is_ascii_digit() || byte == b'-')
        || entry.directory.file_name().and_then(|name| name.to_str()) != Some(id)
        || entry.ownership_token.len() != 64
    {
        return Err("Invalid owned image job identity.".into());
    }
    verify_directory(&entry.directory)?;
    let root = entry.directory.parent().ok_or_else(|| "Missing output root.".to_string())?;
    let test_root = root == home.join("oceanway-image-tests");
    let task_root = root.file_name().and_then(|name| name.to_str()) == Some("images")
        && root.parent().and_then(Path::file_name).and_then(|name| name.to_str()) == Some("output");
    if !test_root && !task_root {
        return Err("Owned image output location is not an engine output directory.".into());
    }
    Ok(())
}

pub(super) fn recover_jobs(home: &Path) -> Result<BTreeMap<String, StoredJob>, String> {
    let _guard = STORE_LOCK.lock().map_err(|_| "Owned image index is unavailable.".to_string())?;
    let Some(directory) = index_directory(home, false)? else { return Ok(BTreeMap::new()) };
    let index = read_index(home, &directory)?;
    let mut jobs = BTreeMap::new();
    for (id, entry) in index.jobs {
        validate_owned_directory(home, &id, &entry)?;
        let manifest: JobManifest = serde_json::from_slice(
            &read_regular(&entry.directory.join("manifest.json"), JSON_BYTES)?
        ).map_err(|_| "Referenced image manifest is invalid; no jobs were resumed.".to_string())?;
        if manifest.schema_version != 2 || manifest.engine != ENGINE_ID
            || manifest.codex_home != home || manifest.ownership_token != entry.ownership_token
            || manifest.job.id != id || Path::new(&manifest.output_directory) != entry.directory
            || manifest.job.output_directory != manifest.output_directory
            || manifest.job.total != manifest.count || manifest.job.model != manifest.model
            || manifest.job.reference_paths != manifest.reference_paths
            || manifest.job.reference_hashes.len() != manifest.reference_paths.len()
        {
            return Err("Referenced image manifest ownership/content mismatch; no jobs were resumed.".into());
        }
        let request = validate_request(ImageTestRequest {
            model: manifest.model,
            prompt: manifest.prompt,
            prompts: manifest.prompts,
            count: manifest.count,
            reference_paths: manifest.reference_paths,
            size: manifest.size,
        })?;
        let input = Input {
            request,
            provider: SavedProvider {
                home: home.into(), config: String::new(), api_base: String::new(), key: String::new(),
                #[cfg(test)]
                local_mock: false,
            },
            references: Vec::new(),
            directory: entry.directory,
        };
        let mut job = StoredJob::new(id.clone(), input);
        job.reference_hashes = manifest.job.reference_hashes;
        job.ownership_token = manifest.ownership_token;
        job.leased = manifest.leased;
        job.persistence_error = manifest.job.persistence_error;
        for mut item in manifest.job.items {
            if item.index == 0 || item.index > manifest.count || job.items.contains_key(&item.index)
                || !matches!(item.status.as_str(), "queued" | "paused" | "running" | "cancelled"
                    | "failed" | "succeeded" | "uncertain" | "integrity_failed")
            {
                return Err("Invalid image slot in referenced manifest; no jobs were resumed.".into());
            }
            item.preview_data_url = None;
            if item.status == "running" {
                item.status = "uncertain".into();
                item.error = Some("Process ended before this request's outcome was recorded. It may have \
                    been billed. No automatic POST; explicit retry may incur another charge.".into());
                preserve_unrecorded_output(&job.input.directory, &mut item);
            } else if item.status == "queued" {
                item.status = "paused".into();
            }
            if item.status == "succeeded" {
                job.completed += 1;
            } else if matches!(item.status.as_str(), "failed" | "uncertain" | "integrity_failed") {
                job.failed += 1;
            }
            if item.status == "integrity_failed" {
                item.retry_blocked = true;
            }
            job.items.insert(item.index, item);
        }
        verify_saved_items(&mut job);
        job.skip_successes();
        job.recovered = job.completed != job.input.request.count;
        job.paused = job.recovered;
        // Rewrite the recovery outcome, but never start workers or load a provider.
        if let Err(error) = persist_manifest(&job) {
            job.persistence_error = Some(format!("Recovery progress could not be persisted: {error}"));
        }
        jobs.insert(id, job);
    }
    Ok(jobs)
}

fn valid_output_name(path: &Path, index: usize, primary: bool) -> bool {
    let extension = path.extension().and_then(|name| name.to_str()).unwrap_or("");
    if !matches!(extension, "png" | "jpeg" | "webp") {
        return false;
    }
    let stem = path.file_stem().and_then(|name| name.to_str()).unwrap_or("");
    if primary {
        stem == index.to_string()
    } else {
        stem.strip_prefix(&format!("{index}-extra-")).is_some_and(|suffix|
            !suffix.is_empty() && suffix.split('-').all(|part|
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())))
    }
}

fn read_output(directory: &Path, path: &Path, index: usize, primary: bool) -> Result<ImageOutput, String> {
    verify_directory(directory)?;
    if path.parent() != Some(directory) || !valid_output_name(path, index, primary) {
        return Err("Recorded output is not a contained image for this slot.".into());
    }
    let bytes = read_regular(path, IMAGE_BYTES)?;
    let (decoded, format) = decode_image(&bytes)?;
    if path.extension().and_then(|name| name.to_str()) != Some(format_info(format).1) {
        return Err("Recorded output extension does not match its decoded format.".into());
    }
    Ok(ImageOutput {
        path: path.to_string_lossy().into_owned(), width: decoded.width(), height: decoded.height(),
        sha256: hash(&bytes), bytes: bytes.len() as u64,
    })
}

pub(super) fn verify_item(directory: &Path, item: &ImageItem) -> Result<(), String> {
    let path = recorded_path(directory, item)?;
    if item.outputs.len() != item.additional_paths.len() + 1
        || item.outputs.first().is_none_or(|output| Path::new(&output.path) != path
            || item.sha256.as_ref() != Some(&output.sha256)
            || item.width != Some(output.width) || item.height != Some(output.height))
    {
        return Err("Recorded image has incomplete hash/dimension evidence.".into());
    }
    for (position, output) in item.outputs.iter().enumerate() {
        if position > 0 && item.additional_paths[position - 1] != output.path {
            return Err("Recorded additional image paths do not match their evidence.".into());
        }
        let actual = read_output(directory, Path::new(&output.path), item.index, position == 0)?;
        if actual.sha256 != output.sha256 || actual.bytes != output.bytes
            || actual.width != output.width || actual.height != output.height
        {
            return Err("Recorded image failed SHA-256/size/dimension verification.".into());
        }
    }
    Ok(())
}

pub(super) fn verify_saved_items(job: &mut StoredJob) -> bool {
    let mut changed = false;
    for item in job.items.values_mut().filter(|item| item.status == "succeeded") {
        if let Err(error) = verify_item(&job.input.directory, item) {
            item.status = "integrity_failed".into();
            item.retry_blocked = true;
            item.preview_data_url = None;
            item.error = Some(error);
            item.warning = Some("Saved output is missing or changed. This slot will never be re-POSTed \
                by retry; inspect its files or submit a new explicit job.".into());
            job.completed = job.completed.saturating_sub(1);
            job.failed += 1;
            changed = true;
        }
    }
    if changed {
        job.paused = true;
        job.revision = job.revision.wrapping_add(1);
    }
    changed
}

fn preserve_unrecorded_output(directory: &Path, item: &mut ImageItem) {
    // A crash can happen after create_new/sync_all but before the manifest commit.
    // Only inspect exact names for this already-recorded slot, never scan siblings.
    for extension in ["png", "jpeg", "webp"] {
        let path = directory.join(format!("{}.{extension}", item.index));
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            _ => item.retry_blocked = true,
        }
        if let Ok(output) = read_output(directory, &path, item.index, true) {
            item.path = Some(output.path.clone());
            item.width = Some(output.width);
            item.height = Some(output.height);
            item.sha256 = Some(output.sha256.clone());
            item.outputs.push(output);
        }
        item.warning = Some("An output file exists for an interrupted slot. It is preserved, but its \
            original manifest commit is missing. Retry will not POST this slot again.".into());
        return;
    }
}
