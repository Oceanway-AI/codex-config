//! Durable video jobs over the customer's Oceanway API. Discovery and polling
//! never submit a generation request. A lost POST response is never retried.
use super::provider;
use reqwest::blocking::{Client, Response};
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const JSON_LIMIT: u64 = 4 * 1024 * 1024;
const VIDEO_LIMIT: u64 = 1024 * 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateVideoRequest {
    /// Absolute existing working directory; videos are saved in output/videos.
    pub workspace_directory: String,
    /// Describe the requested video and how each ordered reference should be used.
    pub prompt: String,
    /// Optional permitted model; get_video_models lists actual models and parameter ranges.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    #[serde(default)]
    pub generate_audio: Option<bool>,
    /// Ordered HTTPS image references. Preserve order and use real original files' URLs.
    #[serde(default)]
    pub reference_images: Vec<String>,
    /// Ordered HTTPS video references. Do not replace a reference with a description.
    #[serde(default)]
    pub reference_videos: Vec<String>,
    /// Ordered HTTPS audio references, alongside at least one image or video.
    #[serde(default)]
    pub reference_audios: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct VideoJob {
    pub id: String,
    pub status: String,
    pub model: String,
    pub task_id: Option<String>,
    pub progress: Option<f64>,
    pub output_path: Option<String>,
    pub error: Option<String>,
    pub reference_counts: [usize; 3],
}

#[derive(Deserialize, Serialize)]
struct Stored {
    job: VideoJob,
    request: GenerateVideoRequest,
    credential: String,
}

fn client() -> Result<Client, String> {
    Client::builder().connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none())
        .build().map_err(|_| "Could not initialize the Oceanway video connection.".into())
}

fn json_response(response: Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        // Do not relay a gateway body which can contain credentials or signed URLs.
        return Err(format!("Oceanway video API returned HTTP {}. Check the model, parameters and account status.", status.as_u16()));
    }
    let mut data = Vec::new();
    response.take(JSON_LIMIT + 1).read_to_end(&mut data)
        .map_err(|_| "Could not read the Oceanway video response.")?;
    if data.len() as u64 > JSON_LIMIT { return Err("Oceanway video response is too large.".into()); }
    serde_json::from_slice(&data).map_err(|_| "Oceanway did not return a valid video API response.".into())
}

pub fn is_video_model(id: &str) -> bool { id.to_ascii_lowercase().starts_with("seedance-") }

fn catalog(base: &str, key: &str) -> Result<Vec<Value>, String> {
    let client = client()?;
    let available = json_response(client.get(format!("{base}/models")).bearer_auth(key)
        .send().map_err(|_| "Could not load the models available to this Key.")?)?;
    let mut origin = reqwest::Url::parse(base).map_err(|_| "Invalid Oceanway endpoint.")?;
    origin.set_path("/api/platform/v1/public/offering");
    let offering = json_response(client.get(origin).send()
        .map_err(|_| "Could not load Oceanway video capabilities.")?)?;
    let ids = available["data"].as_array().ok_or("Invalid Oceanway model list.")?;
    let models = offering.pointer("/data/models").and_then(Value::as_array)
        .ok_or("Invalid Oceanway public capabilities.")?;
    Ok(models.iter().filter(|m| {
        let name = m["model_name"].as_str().unwrap_or("");
        is_video_model(name) && ids.iter().any(|id| id["id"] == name)
            && m["invocation_status"] == "available"
    }).map(|m| json!({"model":m["model_name"], "profile":m["media_generation_profile"]})).collect())
}

pub fn models(home: &Path) -> Result<Value, String> {
    let provider = provider::read_record(home)?;
    Ok(json!({"models":catalog(&provider.api_base, &provider.api_key)?,
        "reference_transport":"https_urls", "local_file_upload_available":false,
        "first_last_frame_available":false,
        "instructions":"Preserve reference order. Local attachment upload is pending platform support; never invent a URL or substitute text for a missing attachment."}))
}

fn reference_url(value: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(value).map_err(|_| "Video references require real HTTPS URLs. Local attachment upload is not available yet.")?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty()
        || url.password().is_some() || url.fragment().is_some() {
        return Err("Video references require real HTTPS URLs. Local attachment upload is not available yet.".into());
    }
    Ok(())
}

fn validate_input(request: &GenerateVideoRequest) -> Result<(), String> {
    if request.prompt.trim().chars().count() < 10 || request.prompt.len() > 32_000 {
        return Err("Describe the video in at least 10 characters (maximum 32000 bytes).".into());
    }
    for url in request.reference_images.iter().chain(&request.reference_videos).chain(&request.reference_audios) {
        reference_url(url)?;
    }
    if !request.reference_audios.is_empty() && request.reference_images.is_empty() && request.reference_videos.is_empty() {
        return Err("An audio reference needs at least one image or video reference.".into());
    }
    Ok(())
}

fn conforms(value: &Value, schema: &Value) -> bool {
    if let Some(values) = schema["enum"].as_array() {
        if !values.contains(value) { return false; }
    }
    match schema["type"].as_str() {
        Some("integer") => value.as_u64().is_some_and(|n| {
            n >= schema["minimum"].as_u64().unwrap_or(0)
                && n <= schema["maximum"].as_u64().unwrap_or(u64::MAX)
        }),
        Some("string") => value.is_string(),
        Some("boolean") => value.is_boolean(),
        _ => false,
    }
}

fn payload(request: &GenerateVideoRequest, models: &[Value]) -> Result<Value, String> {
    validate_input(request)?;
    let model = if request.model.is_empty() {
        models.iter().find(|m| m["model"] == "seedance-2.0").or_else(|| models.first())
    } else { models.iter().find(|m| m["model"] == request.model) }
        .ok_or("This Key has no matching video model. Use get_video_models to see available models.")?;
    let profile = &model["profile"];
    let variants = profile["variants"].as_array().ok_or("Video capability variants are missing.")?;
    let mut ordered: Vec<_> = variants.iter().collect();
    ordered.sort_by_key(|variant| variant["id"] != profile["default_variant"]);
    for variant in ordered {
        if variant["method"] != "POST" || variant["path"] != "/videos" || variant["protocol"] != "openai-video" { continue; }
        let mut specs = variant["defaults"].as_object().cloned().unwrap_or_default();
        if let Some(n) = request.duration { specs.insert("duration".into(), json!(n)); }
        if let Some(v) = &request.resolution { specs.insert("resolution".into(), json!(v)); }
        if let Some(v) = &request.aspect_ratio { specs.insert("aspect_ratio".into(), json!(v)); }
        if let Some(v) = request.generate_audio { specs.insert("audio".into(), json!(v)); }
        let properties = &variant["specifications_schema"]["properties"];
        if specs.iter().any(|(key,v)| !conforms(v, &properties[key])) { continue; }
        let mut body = json!({"model":model["model"], "prompt":request.prompt});
        let mut compatible = true;
        for (name,value) in specs {
            if let Some(binding) = variant["parameter_bindings"][&name].as_str() {
                body[binding] = value;
            } else { compatible = false; }
        }
        let refs = [(&request.reference_images, "reference_image"),
            (&request.reference_videos, "reference_video"), (&request.reference_audios, "reference_audio")];
        let count = refs.iter().map(|(refs,_)| refs.len()).sum::<usize>();
        if count as u64 > variant["max_reference_items"].as_u64()
            .or_else(|| profile["max_reference_items"].as_u64()).unwrap_or(0) { continue; }
        for (urls, role) in refs {
            if urls.is_empty() { continue; }
            let input = variant["native_reference_inputs"].as_array().and_then(|inputs| inputs.iter().find(|input|
                input["transport"] == "url-array" && input["roles"].as_array().is_some_and(|roles|roles.iter().any(|r|r==role))));
            if let Some(input) = input.filter(|i| urls.len() as u64 <= i["max_items"].as_u64().unwrap_or(0)) {
                if let Some(field) = input["field"].as_str() { body[field] = json!(urls); }
                else { compatible = false; }
            } else { compatible = false; }
        }
        if compatible { return Ok(body); }
    }
    Err("The requested duration, resolution or references do not match this model's current capabilities. Call get_video_models; do not silently change the user's request.".into())
}

fn private_file(path: &Path, new: bool) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if new { options.create_new(true); } else { options.create(true); }
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    options.open(path).map_err(|_| "Could not open the local video job record.".into())
}

fn root(home: &Path) -> Result<PathBuf, String> {
    let home = fs::canonicalize(home).map_err(|_| "Oceanway application data is unavailable.")?;
    let root = home.join("video-jobs");
    fs::create_dir_all(&root).map_err(|_| "Could not create the video job directory.")?;
    if fs::symlink_metadata(&root).map_err(|_| "Video job directory is unavailable.")?.file_type().is_symlink()
        || fs::canonicalize(&root).ok().as_ref() != Some(&root) {
        return Err("Video job directory must be inside Oceanway application data.".into());
    }
    Ok(root)
}

fn job_path(home: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) || !id.starts_with("video-") { return Err("Invalid video job ID.".into()); }
    let path = root(home)?.join(id);
    if fs::canonicalize(&path).ok().as_ref() != Some(&path) { return Err("Video job was not found.".into()); }
    Ok(path)
}

fn valid_id(id: &str) -> bool { !id.is_empty() && id.len() <= 180 && id.bytes().all(|b|b.is_ascii_alphanumeric() || b == b'-' || b == b'_') }

fn load(path: &Path) -> Result<Stored, String> {
    let bytes = fs::read(path.join("job.json")).map_err(|_| "Could not read the saved video job.")?;
    if bytes.len() as u64 > JSON_LIMIT { return Err("Video job record is too large.".into()); }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid saved video job.".into())
}

fn save(path: &Path, stored: &Stored) -> Result<(), String> {
    let temporary = path.join(format!("state-{}-{}.tmp", std::process::id(), SEQUENCE.fetch_add(1,Ordering::Relaxed)));
    let result = (|| {
        let mut file = private_file(&temporary,true)?;
        serde_json::to_writer(&mut file,stored).map_err(|_| "Could not write the video job.")?;
        file.sync_all().map_err(|_| "Could not persist the video job.")?;
        drop(file);
        fs::rename(&temporary,path.join("job.json")).map_err(|_| "Could not save the video job.")?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(temporary); }
    result
}

/// The lock is held through capability checks and the POST receipt. OS file
/// locking also prevents two Agent processes from polling/saving the same job.
pub struct Submission { path: PathBuf, stored: Stored, _lock: File }

pub fn prepare(home: &Path, mut request: GenerateVideoRequest) -> Result<(VideoJob, Submission), String> {
    validate_input(&request)?;
    let workspace = Path::new(&request.workspace_directory);
    if !workspace.is_absolute() || !workspace.is_dir() { return Err("workspace_directory must be an existing absolute directory.".into()); }
    let workspace = fs::canonicalize(workspace).map_err(|_| "Video workspace is unavailable.")?;
    request.workspace_directory = workspace.to_string_lossy().into_owned();
    let key = provider::read_api_key(home)?;
    let id = format!("video-{}-{}-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(), std::process::id(), SEQUENCE.fetch_add(1,Ordering::Relaxed));
    let path = root(home)?.join(&id);
    fs::create_dir(&path).map_err(|_| "Could not create a video job.")?;
    let lock = private_file(&path.join("lock"),true)?;
    lock.try_lock().map_err(|_| "Video job is busy.")?;
    let job = VideoJob { id, status:"preparing".into(), model:request.model.clone(), task_id:None, progress:None, output_path:None, error:None,
        reference_counts:[request.reference_images.len(),request.reference_videos.len(),request.reference_audios.len()] };
    let stored = Stored {job:job.clone(), request, credential:format!("{:x}",Sha256::digest(key.as_bytes()))};
    save(&path,&stored)?;
    Ok((job,Submission {path,stored,_lock:lock}))
}

impl Submission {
    pub fn run(mut self, home: &Path, cancelled: impl Fn() -> bool) {
        let outcome = (|| {
            let provider = provider::read_record(home)?;
            if self.stored.credential != format!("{:x}",Sha256::digest(provider.api_key.as_bytes())) { return Err("Oceanway Key changed before video submission.".into()); }
            let body = payload(&self.stored.request, &catalog(&provider.api_base, &provider.api_key)?)?;
            if cancelled() { return Err("Video request was stopped before submission.".into()); }
            self.stored.job.model = body["model"].as_str().unwrap_or("").into();
            self.stored.job.status = "submitting".into();
            save(&self.path,&self.stored)?;
            let response = client()?.post(format!("{}/videos",provider.api_base)).bearer_auth(&provider.api_key)
                .json(&body).send().map_err(|_| "Video submission result is unknown. Do not resubmit automatically; check the account task history.")?;
            if response.status().is_client_error() { self.stored.job.status = "failed".into(); }
            let value = json_response(response)?;
            let id = value["id"].as_str().or_else(||value["task_id"].as_str()).filter(|s|valid_id(s))
                .ok_or("Video submission returned no valid task ID. Do not resubmit automatically.")?;
            self.stored.job.task_id = Some(id.into());
            self.stored.job.status = "queued".into();
            save(&self.path,&self.stored)
        })();
        if let Err(error) = outcome {
            self.stored.job.status = if self.stored.job.status == "submitting" {"outcome_unknown"} else {"failed"}.into();
            self.stored.job.error = Some(error);
            let _ = save(&self.path,&self.stored);
        }
    }
}

pub fn status(home: &Path, id: &str) -> Result<VideoJob, String> { Ok(load(&job_path(home,id)?)?.job) }

fn remote_status(value: &Value) -> Result<&str,String> {
    match value["status"].as_str() {
        Some("queued" | "pending") => Ok("queued"),
        Some("in_progress" | "running" | "processing") => Ok("in_progress"),
        Some("completed" | "succeeded") => Ok("completed"),
        Some("failed" | "error") => Ok("failed"),
        Some("cancelled" | "canceled") => Ok("cancelled"),
        _ => Err("Oceanway returned an unrecognized video task status.".into()),
    }
}

/// Refresh and download run in a worker; get_video_job can return the durable
/// state immediately even when downloading a large completed video.
pub fn refresh(home: &Path, id: &str) -> Result<(), String> {
    let path = job_path(home,id)?;
    let lock = private_file(&path.join("lock"),false)?;
    if lock.try_lock().is_err() { return Ok(()); }
    let mut stored = load(&path)?;
    if stored.job.output_path.is_some() || matches!(stored.job.status.as_str(),"failed"|"outcome_unknown"|"interrupted"|"cancelled") { return Ok(()); }
    let task = match &stored.job.task_id {
        Some(task) if valid_id(task) => task.clone(),
        _ => {
            stored.job.status = if stored.job.status == "submitting" {"outcome_unknown"} else {"interrupted"}.into();
            stored.job.error = Some("The Agent stopped before a task receipt was saved. Polling will not create a new paid request.".into());
            return save(&path,&stored);
        }
    };
    let result = (|| {
        let provider = provider::read_record(home)?;
        if stored.credential != format!("{:x}",Sha256::digest(provider.api_key.as_bytes())) { return Err("Restore the original Oceanway Key to query this video job.".into()); }
        let value = json_response(client()?.get(format!("{}/videos/{task}",provider.api_base)).bearer_auth(&provider.api_key)
            .send().map_err(|_| "Could not query the video task. Call get_video_job again; no generation request will be repeated.")?)?;
        stored.job.status = remote_status(&value)?.into();
        stored.job.progress = value["progress"].as_f64();
        stored.job.error = None;
        if stored.job.status == "failed" { stored.job.error = Some("Oceanway reports that video generation failed. Check this task in the account console.".into()); }
        if stored.job.status == "completed" {
            stored.job.status = "downloading".into();
            save(&path,&stored)?;
            let output = download(&provider.api_base,&provider.api_key,&task,&stored.request.workspace_directory,&stored.job.id)?;
            stored.job.output_path = Some(output.to_string_lossy().into_owned());
            stored.job.status = "completed".into();
        }
        Ok::<_,String>(())
    })();
    if let Err(error) = result { stored.job.error = Some(error); }
    save(&path,&stored)
}

fn download(base: &str,key:&str,task:&str,workspace:&str,id:&str) -> Result<PathBuf,String> {
    let client = client()?;
    let mut response = client.get(format!("{base}/videos/{task}/content")).bearer_auth(key)
        .timeout(Duration::from_secs(240)).send().map_err(|_| "Could not download the completed video. Query this job again to retry the download.")?;
    // Signed CDN redirects receive no customer authorization header.
    for _ in 0..3 {
        if !response.status().is_redirection() { break; }
        let url = response.headers().get(reqwest::header::LOCATION).and_then(|v|v.to_str().ok()).ok_or("Video download redirect has no URL.")?.to_owned();
        reference_url(&url)?;
        response = client.get(url).timeout(Duration::from_secs(240)).send().map_err(|_| "Could not download the video asset.")?;
    }
    if !response.status().is_success() {return Err(format!("Video content returned HTTP {}. Query this job to retry the download.",response.status().as_u16()));}
    if response.content_length().is_some_and(|n|n>VIDEO_LIMIT) { return Err("The video exceeds the 1 GB local download limit.".into()); }
    let expected_length = response.content_length();
    let workspace = fs::canonicalize(workspace).map_err(|_| "The video output workspace is unavailable.")?;
    let output = workspace.join("output").join("videos");
    fs::create_dir_all(&output).map_err(|_| "Could not create the video output directory.")?;
    if fs::canonicalize(&output).ok().as_ref()!=Some(&output) {return Err("Video output must remain inside the workspace.".into());}
    let final_path = output.join(format!("{id}.mp4"));
    if final_path.exists() { return Err("Video output already exists; it will not be overwritten.".into()); }
    let temporary = output.join(format!("{id}-{}.part",SEQUENCE.fetch_add(1,Ordering::Relaxed)));
    let result = (|| {
        let mut file = private_file(&temporary,true)?;
        let mut first = [0;12];
        response.read_exact(&mut first).map_err(|_| "Video content was incomplete.")?;
        if &first[4..8] != b"ftyp" {return Err("The content endpoint did not return an MP4 video.".into());}
        file.write_all(&first).map_err(|_| "Could not save video bytes.")?;
        let length = std::io::copy(&mut response.take(VIDEO_LIMIT), &mut file).map_err(|_| "Video download was interrupted; query this job to retry the download.")?+12;
        if length>VIDEO_LIMIT || length<128 {return Err("Video content has an invalid size.".into());}
        if expected_length.is_some_and(|n| n != length) { return Err("Video download was incomplete; query this job to retry the download.".into()); }
        file.sync_all().map_err(|_| "Could not persist the downloaded video.")?;
        drop(file);
        fs::rename(&temporary,&final_path).map_err(|_| "Could not finish saving the video.")?;
        Ok(final_path)
    })();
    if result.is_err() {let _=fs::remove_file(temporary);}
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc,Mutex};

    fn profile() -> Value {
        json!({"default_variant":"default","max_reference_items":12,"variants":[{
            "id":"default","method":"POST","path":"/videos","protocol":"openai-video",
            "defaults":{"duration":5,"resolution":"720p","aspect_ratio":"16:9"},
            "specifications_schema":{"properties":{"duration":{"type":"integer","minimum":4,"maximum":15},
                "resolution":{"type":"string","enum":["480p","720p"]},"aspect_ratio":{"type":"string","enum":["16:9","9:16"]},"audio":{"type":"boolean"}}},
            "parameter_bindings":{"duration":"duration","resolution":"resolution","aspect_ratio":"aspect_ratio","audio":"generate_audio"},
            "native_reference_inputs":[
                {"field":"images","transport":"url-array","roles":["reference_image"],"max_items":9},
                {"field":"reference_videos","transport":"url-array","roles":["reference_video"],"max_items":3},
                {"field":"reference_audios","transport":"url-array","roles":["reference_audio"],"max_items":3}]
        }]})
    }
    fn request() -> GenerateVideoRequest {
        serde_json::from_value(json!({"workspace_directory":"/unused","prompt":"A blue sphere moves slowly across a plain white surface."})).unwrap()
    }
    fn models_fixture() -> Vec<Value> {vec![json!({"model":"seedance-2.0","profile":profile()})]}

    #[test]
    fn multimodal_references_and_audio_keep_real_wire_names_and_order() {
        let mut req=request();
        req.reference_images=vec!["https://assets.example/second.png".into(),"https://assets.example/first.png".into()];
        req.reference_videos=vec!["https://assets.example/movement.mp4".into()];
        req.reference_audios=vec!["https://assets.example/rhythm.wav".into()];
        req.generate_audio=Some(true);
        let body=payload(&req,&models_fixture()).unwrap();
        assert_eq!(body["images"],json!(req.reference_images));
        assert_eq!(body["reference_videos"],json!(req.reference_videos));
        assert_eq!(body["reference_audios"],json!(req.reference_audios));
        assert_eq!(body["generate_audio"],true);
        assert!(body.get("audio").is_none());
        req.resolution=Some("1080p".into());assert!(payload(&req,&models_fixture()).is_err());
        req.resolution=None;req.duration=Some(30);assert!(payload(&req,&models_fixture()).is_err());
        req.duration=None;req.reference_images=vec!["https://assets.example/image.png".into();10];assert!(payload(&req,&models_fixture()).is_err());
    }
    #[test]
    fn local_attachments_and_unpublished_roles_are_not_silently_dropped() {
        let mut req=request();
        for path in ["C:\\images\\reference.png","/tmp/reference.mp4","data:image/png;base64,AAA","file:///tmp/audio.wav"] {
            req.reference_images=vec![path.into()];
            assert!(validate_input(&req).unwrap_err().contains("Local attachment upload"));
        }
        assert!(serde_json::from_value::<GenerateVideoRequest>(json!({"workspace_directory":"/tmp","prompt":"video prompt","first_image_url":"https://example.com/a.png"})).is_err());
    }

    struct Fixture {home:PathBuf,base:String,seen:Arc<Mutex<Vec<String>>>,stop:Arc<std::sync::atomic::AtomicBool>}
    impl Fixture {
        fn new(post_status:&'static str,post_body:&'static str) -> Self {
            let home=std::env::temp_dir().join(format!("oceanway-video-test-{}-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),SEQUENCE.fetch_add(1,Ordering::Relaxed)));
            fs::create_dir(&home).unwrap();
            let listener=TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();
            let base=format!("http://{}/v1",listener.local_addr().unwrap());
            provider::fixture(&home,&base,"fixture-video-key","fixture-image");
            let seen=Arc::new(Mutex::new(Vec::new()));let requests=seen.clone();
            let stop=Arc::new(std::sync::atomic::AtomicBool::new(false));let stopped=stop.clone();
            std::thread::spawn(move || while !stopped.load(Ordering::SeqCst) {
                let Ok((mut socket,_))=listener.accept() else {std::thread::sleep(Duration::from_millis(5));continue;};
                socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut bytes=Vec::new();let mut buffer=[0;4096];
                let header_end=loop {let n=socket.read(&mut buffer).unwrap();if n==0 {break 0;} bytes.extend_from_slice(&buffer[..n]);if let Some(p)=bytes.windows(4).position(|p|p==b"\r\n\r\n") {break p+4;}};
                if header_end==0 {continue;}
                let headers=String::from_utf8_lossy(&bytes[..header_end]).into_owned();
                let length=headers.lines().find_map(|line|line.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|n|n.parse::<usize>().ok())).unwrap_or(0);
                while bytes.len()<header_end+length {let n=socket.read(&mut buffer).unwrap();if n==0 {break;}bytes.extend_from_slice(&buffer[..n]);}
                let line=headers.lines().next().unwrap().to_owned();requests.lock().unwrap().push(line.clone());
                let (status,content,body)=if line.starts_with("GET /v1/models ") {("200 OK","application/json",json!({"data":[{"id":"seedance-2.0"}]}).to_string().into_bytes())}
                    else if line.contains("/public/offering ") {("200 OK","application/json",json!({"data":{"models":[{"model_name":"seedance-2.0","invocation_status":"available","media_generation_profile":profile()}]}}).to_string().into_bytes())}
                    else if line.starts_with("POST /v1/videos ") {(post_status,"application/json",post_body.as_bytes().to_vec())}
                    else if line.contains("/content ") {let mut mp4=vec![0;256];mp4[4..8].copy_from_slice(b"ftyp");("200 OK","video/mp4",mp4)}
                    else {("200 OK","application/json",br#"{"id":"task_fixture","status":"completed","progress":100}"#.to_vec())};
                write!(socket,"HTTP/1.1 {status}\r\nContent-Type: {content}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();socket.write_all(&body).unwrap();
            });
            Self {home,base,seen,stop}
        }
        fn request(&self)->GenerateVideoRequest {let mut req=request();req.workspace_directory=self.home.to_string_lossy().into_owned();req}
    }
    impl Drop for Fixture {fn drop(&mut self){self.stop.store(true,Ordering::SeqCst);let _=fs::remove_dir_all(&self.home);}}

    #[test]
    fn saved_task_resumes_download_without_another_paid_post_or_saved_key() {
        let fixture=Fixture::new("200 OK",r#"{"id":"task_fixture","status":"queued"}"#);
        let (job,submission)=prepare(&fixture.home,fixture.request()).unwrap();
        submission.run(&fixture.home,||false);
        assert_eq!(status(&fixture.home,&job.id).unwrap().task_id.as_deref(),Some("task_fixture"));
        refresh(&fixture.home,&job.id).unwrap();
        let done=status(&fixture.home,&job.id).unwrap();assert_eq!(done.status,"completed");
        assert_eq!(fs::metadata(done.output_path.unwrap()).unwrap().len(),256);
        refresh(&fixture.home,&job.id).unwrap();
        assert_eq!(fixture.seen.lock().unwrap().iter().filter(|s|s.starts_with("POST ")).count(),1);
        let saved=fs::read_to_string(job_path(&fixture.home,&job.id).unwrap().join("job.json")).unwrap();assert!(!saved.contains("fixture-video-key"));
    }
    #[test]
    fn uncertain_receipt_is_never_reposted_and_changed_credentials_block_submission() {
        let fixture=Fixture::new("502 Bad Gateway",r#"{"error":"unavailable"}"#);
        let (job,submission)=prepare(&fixture.home,fixture.request()).unwrap();submission.run(&fixture.home,||false);
        assert_eq!(status(&fixture.home,&job.id).unwrap().status,"outcome_unknown");
        refresh(&fixture.home,&job.id).unwrap();
        let (second,submission)=prepare(&fixture.home,fixture.request()).unwrap();
        provider::fixture(&fixture.home,&fixture.base,"changed-key","fixture-image");submission.run(&fixture.home,||false);
        assert_eq!(status(&fixture.home,&second.id).unwrap().status,"failed");
        assert_eq!(fixture.seen.lock().unwrap().iter().filter(|s|s.starts_with("POST ")).count(),1);
    }
    #[test]
    fn recovery_never_submits_work_whose_agent_was_interrupted() {
        let fixture=Fixture::new("200 OK",r#"{"id":"unused"}"#);
        let (job,submission)=prepare(&fixture.home,fixture.request()).unwrap();
        drop(submission);refresh(&fixture.home,&job.id).unwrap();
        assert_eq!(status(&fixture.home,&job.id).unwrap().status,"interrupted");assert!(fixture.seen.lock().unwrap().is_empty());
    }
}
