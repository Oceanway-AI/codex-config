//! Accepted image jobs are durable GET-only receipts. Polling never re-POSTs.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTask {
    pub id: String,
    pub api_base: String,
    pub query_base: String,
    pub credential_sha256: String,
}

pub(super) fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 180
        && value.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c,b'-'|b'_'))
}

pub(super) fn validate_receipt(task: &RemoteTask) -> Result<(), String> {
    if !valid_id(&task.id) || task.credential_sha256.len() != 64
        || !task.credential_sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid accepted image task receipt.".into());
    }
    let base = Url::parse(&task.api_base).map_err(|_| "Invalid image receipt endpoint.")?;
    let mut gemini = base.clone();
    gemini.set_path("/v1beta");
    if task.query_base != task.api_base && task.query_base != gemini.as_str().trim_end_matches('/') {
        return Err("Image receipt query endpoint does not match its provider.".into());
    }
    Ok(())
}

pub(super) fn safe_error_code(body: &Value, key: &str) -> Option<String> {
    let code = body.pointer("/error/code").or_else(||body.get("code"))?.as_str()?;
    if code.len() > 96 || (!key.is_empty() && code.contains(key))
        || !code.bytes().all(|c|c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c,b'_'|b'-')) {
        return None;
    }
    Some(code.into())
}

pub(super) fn submit_item(shared: &Arc<Shared>, id: &str, input: &Input, index: usize, started: Instant) -> Result<SavedResult, Failure> {
    verify_unused_slot(input,index)?;
    let (body,request_id,provider,query_base) = image_protocol::submit(input,index,started,||authorize_post(shared,id,index,input))?;
    let task_id = body.get("task_id").or_else(||body.get("id")).and_then(Value::as_str);
    if let Some(task_id) = task_id.filter(|_|body.get("status").is_some() || body.get("task_id").is_some()) {
        let task = RemoteTask { id: task_id.into(), api_base: provider.api_base.clone(), query_base,
            credential_sha256: hash(provider.key.as_bytes()) };
        validate_receipt(&task)?;
        if task.id.contains(&provider.key) { return Err(Failure::new("Invalid accepted image task ID.")); }
        let mut registry = lock(shared)?;
        let job = find_job_mut(&mut registry,id)?;
        let item = job.items.get_mut(&index).ok_or("Missing accepted image slot.".to_string())?;
        item.remote_task = Some(task);
        item.retry_blocked = true;
        item.request_id = request_id.clone();
        // Commit the receipt before any query or download can fail.
        persist_or_stop(job)?;
        return Err(Failure { remote_pending: true, request_id,
            ..Failure::new("Oceanway accepted the image task. Query this job to retrieve the result.") });
    }
    save_response_images(input,index,&image_protocol::normalize_sync(body),request_id,started)
}

pub(super) fn schedule_refresh(shared: &Arc<Shared>, id: &str) -> Result<(), String> {
    let mut registry = lock(shared)?;
    if registry.shutdown { return Ok(()); }
    let job = find_job_mut(&mut registry,id)?;
    if job.remote_polling || job.remote_poll_after.is_some_and(|after|Instant::now()<after) { return Ok(()); }
    let tasks: Vec<_> = job.items.values().filter(|item|item.status=="remote_pending")
        .filter_map(|item|item.remote_task.clone().map(|task|(item.index,task))).collect();
    if tasks.is_empty() { return Ok(()); }
    job.remote_polling = true;
    let input = Arc::clone(&job.input);
    let shared_copy = Arc::clone(shared);
    let id = id.to_owned();
    let thread_id = id.clone();
    if thread::Builder::new().name("oceanway-image-query".into()).spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for (index,task) in tasks {
                let result = refresh(&input,index,&task);
                if let Ok(mut registry) = lock(&shared_copy) {
                    if let Some(job) = registry.jobs.get_mut(&thread_id) {
                        apply_refresh(job,index,result);
                        let _ = persist_or_stop(job);
                    }
                }
            }
        }));
        if let Ok(mut registry) = lock(&shared_copy) {
            if let Some(job) = registry.jobs.get_mut(&thread_id) {
                job.remote_polling = false;
                job.remote_poll_after = Some(Instant::now()+Duration::from_secs(3));
                if outcome.is_err() {
                    for item in job.items.values_mut().filter(|item|item.status=="remote_pending") {
                        item.error = Some("Image status check interrupted. Query this original job again.".into());
                    }
                }
                release_completed_input(job);
            }
            shared_copy.wake.notify_all();
        }
    }).is_err() {
        job.remote_polling = false;
        return Err("Could not start the image status query. Query this job again.".into());
    }
    Ok(())
}

enum Refresh { Pending, Failed(String), Completed(SavedResult) }

fn apply_refresh(job: &mut StoredJob, index: usize, result: Result<Refresh,Failure>) {
    match result {
        Ok(Refresh::Completed(result)) => {
            job.active += 1;
            finish_item(job,index,Ok(result),0);
        }
        Ok(Refresh::Failed(message)) => {
            let item = job.items.get_mut(&index).expect("accepted slot");
            item.status = "failed".into();
            item.error = Some(message);
            job.failed += 1;
        }
        Ok(Refresh::Pending) => {}
        Err(failure) => {
            let item = job.items.get_mut(&index).expect("accepted slot");
            item.error = Some(format!("{} Query this original job again; no generation was resubmitted.",failure.message));
            if failure.request_id.is_some() { item.request_id = failure.request_id; }
        }
    }
}

fn refresh(input: &Input, index: usize, task: &RemoteTask) -> Result<Refresh,Failure> {
    validate_receipt(task)?;
    let provider = load_post_provider(&input.provider)?;
    if task.api_base != provider.api_base || task.credential_sha256 != hash(provider.key.as_bytes()) {
        return Err(Failure::new("Restore the original Oceanway Key to read this accepted image task."));
    }
    let client = provider_client(&provider)?;
    let response = client.get(format!("{}/images/{}",task.query_base,task.id)).bearer_auth(&provider.key)
        .timeout(Duration::from_secs(25)).send().map_err(|_|Failure::new("Could not query the accepted image task."))?;
    let (body,request_id) = json_response(response,&provider.key,JSON_BYTES)?;
    match body["status"].as_str().unwrap_or("unknown") {
        "completed" | "succeeded" | "success" => {
            let entries = body.get("outputs").and_then(Value::as_array).filter(|v|!v.is_empty())
                .or_else(||body.get("data").and_then(Value::as_array)).ok_or_else(||Failure::new("Completed image task has no outputs yet."))?;
            if entries.is_empty() || entries.len()>10 { return Err(Failure::new("Completed image output count is invalid.")); }
            let mut outputs = Vec::new();
            let mut preview = String::new();
            let mut indices = std::collections::BTreeSet::new();
            for (position,entry) in entries.iter().enumerate() {
                let output_index = entry["index"].as_u64().unwrap_or(position as u64);
                if output_index>1000 || !indices.insert(output_index) { return Err(Failure::new("Invalid image output index.")); }
                let bytes = download_output(&client,&provider,task,output_index)?;
                if let Some(expected) = entry["sha256"].as_str().filter(|v|!v.is_empty()) {
                    if !expected.eq_ignore_ascii_case(&hash(&bytes)) { return Err(Failure::new("Image output SHA-256 does not match its task.")); }
                }
                let (saved_preview,output) = save_downloaded_output(input,index,position,&bytes)?;
                if position==0 { preview=saved_preview; }
                outputs.push(output);
            }
            // Both outputs and data describe the same files. Save only one set.
            let multiple=outputs.len()!=1;
            let saved=SavedResult {path:PathBuf::from(&outputs[0].path),preview,request_id,
                warning:multiple.then(||"The provider returned more than the requested one image; all original outputs were saved without another POST.".into()),
                additional_paths:outputs.iter().skip(1).map(|o|o.path.clone()).collect(),outputs,count_mismatch:multiple};
            Ok(Refresh::Completed(saved))
        }
        "failed" | "cancelled" | "canceled" => {
            let code = safe_error_code(&body,&provider.key).map(|c|format!(" Code: {c}.")).unwrap_or_default();
            Ok(Refresh::Failed(format!("Oceanway image task failed.{code} Task: {}. No new generation was submitted.",task.id)))
        }
        _ => Ok(Refresh::Pending),
    }
}

fn save_downloaded_output(input: &Input,index: usize,position: usize,bytes: &[u8]) -> Result<(String,ImageOutput),Failure> {
    let (decoded,format)=decode_image(bytes)?;
    let extension=format_info(format).1;
    let filename=if position==0 {format!("{index}.{extension}")} else {format!("{index}-extra-{}.{extension}",position+1)};
    let path=input.directory.join(filename);
    verify_directory(&input.directory)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            // A crash can occur after the file is durable but before its manifest.
            // Recover those exact bytes without overwriting or duplicating output.
            if metadata.file_type().is_symlink() || !metadata.is_file()
                || fs::canonicalize(&path).ok().as_ref()!=Some(&path) || metadata.len()!=bytes.len() as u64 {
                return Err(Failure::new("Existing image output does not match the original task."));
            }
            let saved=bounded_read(File::open(&path).map_err(|_|Failure::new("Could not read the saved image output."))?,IMAGE_BYTES)?;
            if hash(&saved)!=hash(bytes) {return Err(Failure::new("Existing image output was changed; it was preserved."));}
            Ok((String::new(),ImageOutput {path:path.to_string_lossy().into_owned(),width:decoded.width(),height:decoded.height(),sha256:hash(bytes),bytes:bytes.len() as u64}))
        }
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => {
            let (_,preview,output)=save_image_bytes(input,index,(position>0).then_some(position+1),bytes)?;
            Ok((preview,output))
        }
        Err(_) => Err(Failure::new("Image output location is unavailable.")),
    }
}

fn download_output(client: &Client, provider: &SavedProvider, task: &RemoteTask, index: u64) -> Result<Vec<u8>,Failure> {
    let response = client.get(format!("{}/images/{}/outputs/{index}/content",task.query_base,task.id))
        .bearer_auth(&provider.key).send().map_err(|_|Failure::new("Could not download the original image output."))?;
    if response.status().is_redirection() {
        let location = response.headers().get(reqwest::header::LOCATION).and_then(|v|v.to_str().ok())
            .ok_or_else(||Failure::new("Image download redirect has no destination."))?;
        // The separate download client never receives provider credentials.
        return download_image(location,Instant::now());
    }
    if !response.status().is_success() {
        return Err(json_response(response,&provider.key,16*1024).err().unwrap_or_else(||Failure::new("Image download failed.")));
    }
    let bytes = bounded_read(response,IMAGE_BYTES)?;
    decode_image(&bytes)?;
    Ok(bytes)
}
