//! Mock-only tests: no real provider, external download, or saved user credential.
use super::*;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize};

const MOCK_KEY: &str = "mock-only-credential";
// Production has exactly two workers per process, including across service
// instances. Tests with gated requests must not compete for those same workers.
static WORKER_TEST: Mutex<()> = Mutex::new(());

struct TempHome(PathBuf);

impl TempHome {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "oceanway-image-unit-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct Captured {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn json(body: Value) -> Self {
        Self {
            status: 200,
            headers: vec![
                ("Content-Type".into(), "application/json".into()),
                ("x-request-id".into(), "mock-request-1".into()),
            ],
            body: serde_json::to_vec(&body).unwrap(),
        }
    }
}

struct Mock {
    base: String,
    address: SocketAddr,
    requests: Arc<Mutex<Vec<Captured>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Mock {
    fn new(handler: impl Fn(usize, &Captured) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let recorded = Arc::clone(&requests);
        let stopping = Arc::clone(&stop);
        let handler = Arc::new(handler);
        let handle = thread::spawn(move || {
            let mut connections = Vec::new();
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let recorded = Arc::clone(&recorded);
                        let handler = Arc::clone(&handler);
                        connections.push(thread::spawn(move || {
                            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                            stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
                            let request = capture(&mut stream);
                            let number = {
                                let mut requests = recorded.lock().unwrap();
                                requests.push(request.clone());
                                requests.len()
                            };
                            let reply = handler(number, &request);
                            write!(stream, "HTTP/1.1 {} Mock\r\nContent-Length: {}\r\nConnection: close\r\n",
                                reply.status, reply.body.len()).unwrap();
                            for (name, value) in reply.headers {
                                write!(stream, "{name}: {value}\r\n").unwrap();
                            }
                            stream.write_all(b"\r\n").unwrap();
                            stream.write_all(&reply.body).unwrap();
                        }));
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(err) => panic!("mock listener: {err}"),
                }
            }
            for connection in connections {
                connection.join().unwrap();
            }
        });
        Self {
            base: format!("http://{address}/v1"),
            address,
            requests,
            stop,
            handle: Some(handle),
        }
    }

    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let result = handle.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn capture(stream: &mut TcpStream) -> Captured {
    // The listener polls nonblocking; accepted connections must read complete HTTP bodies.
    stream.set_nonblocking(false).unwrap();
    let mut bytes = Vec::new();
    let boundary = loop {
        let mut buffer = [0u8; 4096];
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "unexpected end of request");
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 4 * 1024 * 1024, "test request too large");
        if let Some(boundary) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break boundary + 4;
        }
    };
    let header = String::from_utf8(bytes[..boundary].to_vec()).unwrap();
    let mut lines = header.split("\r\n");
    let mut start = lines.next().unwrap().split_whitespace();
    let method = start.next().unwrap().to_string();
    let path = start.next().unwrap().to_string();
    let headers: BTreeMap<_, _> = lines.filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_string())).collect();
    assert!(!headers.contains_key("transfer-encoding"), "fixture expects known-length multipart");
    let length = headers.get("content-length").map(|s| s.parse::<usize>().unwrap()).unwrap_or(0);
    assert!(length < 4 * 1024 * 1024);
    while bytes.len() - boundary < length {
        let mut buffer = [0u8; 4096];
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
    }
    Captured { method, path, headers, body: bytes[boundary..boundary + length].to_vec() }
}

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        let guard = self.open.lock().unwrap();
        let (guard, timeout) = self.changed.wait_timeout_while(
            guard, Duration::from_secs(5), |open| !*open,
        ).unwrap();
        assert!(*guard && !timeout.timed_out(), "test did not release request gate");
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::new_rgb8(width, height).write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn success() -> Value {
    json!({ "data": [{ "b64_json": STANDARD.encode(png(320, 160)) }] })
}

fn request(count: usize) -> ImageTestRequest {
    ImageTestRequest {
        model: "gpt-image-2".into(),
        prompt: "A mock-only image.".into(),
        prompts: Vec::new(),
        count,
        reference_paths: Vec::new(),
        size: "1024x1024".into(),
    }
}

fn input(home: &TempHome, mock: &Mock, count: usize) -> (String, Input) {
    let (id, directory) = create_directory(&home.0).unwrap();
    (id, Input {
        request: request(count),
        provider: SavedProvider {
            home: home.0.clone(),
            config: "mock".into(),
            api_base: mock.base.clone(),
            key: MOCK_KEY.into(),
            local_mock: true,
        },
        references: Vec::new(),
        directory,
    })
}

fn wait_until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !check() {
        assert!(Instant::now() < deadline, "mock test deadline expired");
        thread::sleep(Duration::from_millis(5));
    }
}

fn finished(state: &ImageJobs, id: &str) -> ImageJob {
    wait_until(|| {
        let registry = lock(&state.shared).unwrap();
        !find_job(&registry, id).unwrap().running()
    });
    find_job(&lock(&state.shared).unwrap(), id).unwrap().snapshot()
}

fn retry_mock(state: &ImageJobs, id: &str) {
    let mut registry = lock(&state.shared).unwrap();
    ensure_workers(&state.shared, &mut registry).unwrap();
    find_job_mut(&mut registry, id).unwrap().retry().unwrap();
    worker_pool().wake.notify_all();
}

#[test]
fn normalization_validation_and_camel_case_contract() {
    assert_eq!(normalize_api_base(" https://example.com/// ").unwrap(), "https://example.com/v1");
    assert_eq!(normalize_api_base("https://example.com/api/v1///").unwrap(), "https://example.com/api/v1");
    assert_eq!(normalize_api_base("https://example.com/api").unwrap(), "https://example.com/api");
    assert_eq!(normalize_api_base("https://example.com/custom/prefix///").unwrap(), "https://example.com/custom/prefix");
    assert_eq!(normalize_api_base("https://example.com/v1").unwrap(), "https://example.com/v1");
    for base in ["https://user:password@example.com", "https://example.com?q=1", "file:///tmp/x"] {
        assert!(normalize_api_base(base).is_err());
    }
    assert!(validate_request(request(0)).is_err());
    assert!(validate_request(request(usize::MAX)).is_ok());
    assert!(validate_model("  ").is_err());
    assert!(validate_model("https://example.com/model").is_err());
    assert!(validate_model("../private").is_err());
    let parsed: ImageTestRequest = serde_json::from_value(json!({
        "prompt": "test", "count": 1, "referencePaths": [], "size": ""
    })).unwrap();
    assert_eq!(parsed.model, "gpt-image-2");
    assert_eq!(validate_request(parsed).unwrap().size, "1024x1024");
    let omitted: ImageTestRequest = serde_json::from_value(json!({
        "prompt": "test", "count": 1
    })).unwrap();
    assert_eq!(omitted.size, "1024x1024");
    let blank: ImageTestRequest = serde_json::from_value(json!({
        "model": "", "prompt": "test", "count": 1
    })).unwrap();
    assert!(validate_request(blank).is_err());
    for invalid in [json!(-1), json!(1.5), json!("2")] {
        assert!(serde_json::from_value::<ImageTestRequest>(json!({
            "prompt": "test", "count": invalid
        })).is_err());
    }
    let item = serde_json::to_value(ImageItem::new(1, "queued")).unwrap();
    assert_eq!(item, json!({ "index": 1, "status": "queued" }));
}

#[test]
fn default_size_and_custom_prefix_reach_generation_and_edit_requests() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let (_, mut input) = input(&home, &mock, 2);
    input.provider.api_base = normalize_api_base(&format!("http://{}/custom/images-api/", mock.address)).unwrap();
    input.request.size = " ".into();
    input.request = validate_request(input.request).unwrap();
    execute(&input, 1, Instant::now()).unwrap();
    input.references.push(Reference {
        bytes: png(1, 1).into(),
        mime: "image/png",
        extension: "png",
    });
    execute(&input, 2, Instant::now()).unwrap();
    let captured = mock.requests.lock().unwrap();
    assert_eq!(captured[0].path, "/custom/images-api/images/generations");
    assert_eq!(serde_json::from_slice::<Value>(&captured[0].body).unwrap()["size"], "1024x1024");
    assert_eq!(captured[1].path, "/custom/images-api/images/edits");
    assert!(String::from_utf8_lossy(&captured[1].body).contains("name=\"size\"\r\n\r\n1024x1024\r\n"));
}

#[test]
fn manifest_persists_progress_without_keys_or_thumbnails_and_does_not_auto_resume() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let state = ImageJobs::default();
    let (id, mut input) = input(&home, &mock, 2);
    input.request.prompt = "A saved prompt".into();
    input.request.reference_paths = vec!["mock-reference.png".into()];
    let directory = input.directory.clone();
    submit(&state.shared, id.clone(), input).unwrap();
    let done = finished(&state, &id);
    assert_eq!(done.completed, 2);
    drop(state);
    let bytes = fs::read(directory.join("manifest.json")).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(!text.contains(MOCK_KEY));
    assert!(!text.contains("previewDataUrl"));
    assert!(!text.contains("apiBase"));
    assert!(!text.contains("\"config\""));
    let manifest: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(manifest["schemaVersion"], 2);
    assert_eq!(manifest["requiresExplicitResume"], true);
    assert_eq!(manifest["count"], 2);
    assert_eq!(manifest["model"], "gpt-image-2");
    assert_eq!(manifest["size"], "1024x1024");
    assert_eq!(manifest["prompt"], "A saved prompt");
    assert_eq!(manifest["referencePaths"][0], "mock-reference.png");
    assert_eq!(manifest["job"]["status"], "completed");
    assert_eq!(manifest["job"]["completed"], 2);
    assert_eq!(manifest["job"]["items"][0]["requestId"], "mock-request-1");
    assert!(Path::new(manifest["job"]["items"][0]["path"].as_str().unwrap()).is_file());
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 3, "two images plus atomic manifest");
    let reopened = ImageJobs::default();
    assert!(lock(&reopened.shared).unwrap().jobs.is_empty());
    assert_eq!(mock.count(), 2);
}

#[test]
fn credentials_in_inputs_are_rejected_before_manifest_or_http_and_redaction_is_defense_in_depth() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("credential-bearing inputs must not POST"));
    let state = ImageJobs::default();
    for field in ["model", "prompt", "prompts", "referencePaths", "size"] {
        let (id, mut input) = input(&home, &mock, 1);
        match field {
            "model" => input.request.model = MOCK_KEY.into(),
            "prompt" => input.request.prompt = format!("accidental {MOCK_KEY} paste"),
            "prompts" => input.request.prompts = vec![format!("accidental {MOCK_KEY} paste")],
            "referencePaths" => input.request.reference_paths.push(format!("C:/test/{MOCK_KEY}.png")),
            "size" => input.request.size = MOCK_KEY.into(),
            _ => unreachable!(),
        }
        let directory = input.directory.clone();
        let error = submit(&state.shared, id, input).err().unwrap();
        assert!(!error.contains(MOCK_KEY));
        assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    }
    assert!(ensure_no_credential_in_values([MOCK_KEY], MOCK_KEY).is_err());
    assert!(lock(&state.shared).unwrap().jobs.is_empty());
    assert_eq!(mock.count(), 0);
    let (id, mut input) = input(&home, &mock, 1);
    input.request.prompt = format!("accidental {MOCK_KEY} paste");
    input.request.model = MOCK_KEY.into();
    input.request.reference_paths = vec![format!("reference-{MOCK_KEY}.png")];
    let job = StoredJob::new(id, input);
    persist_manifest(&job).unwrap();
    let text = fs::read_to_string(job.input.directory.join("manifest.json")).unwrap();
    assert!(!text.contains(MOCK_KEY));
    assert!(text.contains("[REDACTED]"));
}

#[test]
fn manifest_write_failure_prevents_submission_and_keeps_previous_record() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("failed manifest must prevent the POST"));
    let state = ImageJobs::default();
    let (id, input) = input(&home, &mock, 1);
    let directory = input.directory.clone();
    // A non-file destination makes the atomic rename fail on every platform.
    fs::create_dir(directory.join("manifest.json")).unwrap();
    fs::write(directory.join("manifest.json").join("previous"), b"preserve me").unwrap();
    assert!(submit(&state.shared, id, input).is_err());
    assert_eq!(mock.count(), 0);
    assert!(lock(&state.shared).unwrap().jobs.is_empty());
    assert_eq!(fs::read(directory.join("manifest.json").join("previous")).unwrap(), b"preserve me");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1, "failed temporary record was removed");
}

#[test]
fn running_and_cancelled_progress_are_written_without_allocating_the_queue() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("state-only test must not POST"));
    let (id, input) = input(&home, &mock, usize::MAX);
    let mut job = StoredJob::new(id, input);
    assert_eq!(job.claim(), Some(1));
    persist_manifest(&job).unwrap();
    let path = job.input.directory.join("manifest.json");
    let running: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(running["job"]["items"][0]["status"], "running");
    assert_eq!(running["job"]["items"].as_array().unwrap().len(), 3);
    job.cancel();
    persist_manifest(&job).unwrap();
    let cancelled: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(cancelled["job"]["cancelled"].as_u64(), Some((usize::MAX - 1) as u64));
    assert_eq!(cancelled["job"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(mock.count(), 0);
}

#[test]
fn changed_provider_guard_prevents_post_stops_queue_and_preserves_success() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let (id, input) = input(&home, &mock, 3);
    let mut job = StoredJob::new(id, input);
    let first = job.claim().unwrap();
    let saved = execute(&job.input, first, Instant::now()).unwrap();
    let first_path = saved.path.clone();
    finish_item(&mut job, first, Ok(saved), 1);
    let second = job.claim().unwrap();
    let guard_error = post_image_with_guard(&job.input, || {
        provider_guard_result(Err("Saved provider or credentials changed.".into()))
    }).err().unwrap();
    assert!(guard_error.stop_job);
    assert!(guard_error.message.contains("no POST was sent"));
    finish_item(&mut job, second, Err(guard_error), 1);
    persist_manifest(&job).unwrap();
    let snapshot = job.snapshot();
    assert_eq!(snapshot.status, "cancelled");
    assert_eq!((snapshot.completed, snapshot.failed, snapshot.cancelled), (1, 1, 1));
    assert_eq!(snapshot.items[0].status, "succeeded");
    assert!(first_path.is_file());
    assert!(job.claim().is_none());
    assert_eq!(mock.count(), 1);
    assert!(provider_guard_result(Err("Saved config is unreadable.".into())).err().unwrap().stop_job);
}

#[test]
fn provider_requires_valid_saved_root_oceanway_config() {
    let home = TempHome::new();
    assert!(load_provider_in_home(&home.0).is_err());
    for config in [
        "[nested]\nmodel_provider = 'OceanWay'\n",
        "model_provider = 'other'\n[model_providers.OceanWay]\n",
        "model_provider = 'OceanWay'\n",
        "model_provider = 'OceanWay'\n[model_providers.OceanWay]\nbase_url = 'http://example.com'\n",
        "not valid TOML",
    ] {
        fs::write(home.0.join("config.toml"), config).unwrap();
        assert!(load_provider_in_home(&home.0).is_err());
    }
    fs::write(home.0.join("config.toml"), format!(
        "model_provider = \"OceanWay\"\n[model_providers.OceanWay]\n\
         base_url = \"https://example.com/\"\nexperimental_bearer_token = \"{MOCK_KEY}\"\n"
    )).unwrap();
    let provider = load_provider_in_home(&home.0).unwrap();
    assert_eq!(provider.api_base, "https://example.com/v1");
    assert_eq!(provider.key, MOCK_KEY);
}

#[test]
fn metadata_probe_only_gets_models_and_does_not_claim_verified_generation() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(json!({
        "data": [
            { "id": "gpt-image-2" },
            { "id": "gpt-image-2" },
            { "id": "some-other-model" },
            { "id": MOCK_KEY },
            { "id": "https://private.example/image?signature=secret" }
        ]
    })));
    let (_, input) = input(&home, &mock, 1);
    let result = probe_capabilities(&input.provider, "gpt-image-2".into());
    assert!(result.available);
    assert_eq!(result.models, vec!["gpt-image-2", "some-other-model"]);
    assert!(result.message.contains("not verified"));
    assert_eq!(mock.count(), 1);
    let captured = mock.requests.lock().unwrap()[0].clone();
    assert_eq!(captured.method, "GET");
    assert_eq!(captured.path, "/v1/models");
    assert!(captured.body.is_empty());
    assert!(!serde_json::to_string(&result).unwrap().contains(MOCK_KEY));
}

#[test]
fn provider_snapshot_comparison_detects_changed_configuration_endpoint_home_or_credentials() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("comparison must not send HTTP"));
    let (_, first) = input(&home, &mock, 1);
    let (_, mut second) = input(&home, &mock, 1);
    assert!(compare_provider(&first.provider, &second.provider).is_ok());
    second.provider.key = "different-mock-key".into();
    assert!(compare_provider(&first.provider, &second.provider).is_err());
    second.provider.key = first.provider.key.clone();
    second.provider.config.push_str("\n# Changed");
    assert!(compare_provider(&first.provider, &second.provider).is_err());
    second.provider.config = first.provider.config.clone();
    second.provider.api_base.push_str("/changed");
    assert!(compare_provider(&first.provider, &second.provider).is_err());
    second.provider.api_base = first.provider.api_base.clone();
    second.provider.home = home.0.join("other");
    assert!(compare_provider(&first.provider, &second.provider).is_err());
    assert_eq!(mock.count(), 0);
}

#[test]
fn generation_posts_one_and_saves_valid_thumbnail_without_overwrite() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let (_, input) = input(&home, &mock, 12);
    let result = execute(&input, 1, Instant::now()).unwrap();
    assert_eq!(result.path.file_name().unwrap(), "1.png");
    assert_eq!(result.request_id.as_deref(), Some("mock-request-1"));
    let (full, _) = decode_image(&fs::read(&result.path).unwrap()).unwrap();
    assert_eq!((full.width(), full.height()), (320, 160));
    let thumbnail = result.preview.strip_prefix("data:image/png;base64,").unwrap();
    let (preview, _) = decode_image(&STANDARD.decode(thumbnail).unwrap()).unwrap();
    assert_eq!((preview.width(), preview.height()), (256, 128));
    let captured = mock.requests.lock().unwrap()[0].clone();
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/v1/images/generations");
    assert_eq!(captured.headers["authorization"], format!("Bearer {MOCK_KEY}"));
    let body: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(body["n"], 1);
    assert_eq!(body["model"], "gpt-image-2");
    assert_eq!(body["size"], "1024x1024");
    assert!(body.get("quality").is_none());
    assert!(body.get("referencePaths").is_none());
    let original = fs::read(&result.path).unwrap();
    assert!(execute(&input, 1, Instant::now()).is_err());
    assert_eq!(fs::read(&result.path).unwrap(), original);
    assert_eq!(mock.count(), 1, "existing output must be detected before another paid POST");
}

#[test]
fn edits_use_ordered_image_array_mime_and_snapshot_reference_bytes() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let (_, mut input) = input(&home, &mock, 2);
    let first = home.0.join("source-one.unknown");
    let second = home.0.join("source-two.jpeg");
    fs::write(&first, png(1, 1)).unwrap();
    fs::write(&second, png(2, 2)).unwrap();
    input.request.reference_paths = vec![
        first.to_string_lossy().into_owned(), second.to_string_lossy().into_owned(),
    ];
    input.references = load_references(&input.request.reference_paths).unwrap();
    fs::write(&first, b"changed after the validated snapshot").unwrap();
    execute(&input, 1, Instant::now()).unwrap();
    let captured = mock.requests.lock().unwrap()[0].clone();
    assert_eq!(captured.path, "/v1/images/edits");
    assert!(captured.headers["content-type"].starts_with("multipart/form-data; boundary="));
    let body = String::from_utf8_lossy(&captured.body);
    assert_eq!(body.matches("name=\"image[]\"").count(), 2);
    assert_eq!(body.matches("Content-Type: image/png").count()
        + body.matches("content-type: image/png").count(), 2);
    assert!(body.find("reference-1.png").unwrap() < body.find("reference-2.png").unwrap());
    assert!(body.contains("name=\"n\"\r\n\r\n1\r\n"));
    assert!(!body.contains("quality"));
    assert!(!body.contains("source-one"));
    let first_bytes = png(1, 1);
    let second_bytes = png(2, 2);
    let position = |needle: &[u8]| captured.body.windows(needle.len())
        .position(|part| part == needle).unwrap();
    assert!(position(&first_bytes) < position(&second_bytes));
}

#[test]
fn invalid_references_fail_before_post_and_have_byte_dimension_limits() {
    let home = TempHome::new();
    let invalid = home.0.join("invalid.png");
    fs::write(&invalid, b"<html>not an image</html>").unwrap();
    assert!(load_references(&[invalid.to_string_lossy().into_owned()]).is_err());
    let large = home.0.join("large.png");
    File::create(&large).unwrap().set_len(IMAGE_BYTES as u64 + 1).unwrap();
    assert!(load_references(&[large.to_string_lossy().into_owned()]).is_err());
    assert!(load_references(&[home.0.to_string_lossy().into_owned()]).is_err());
    assert!(decode_image(&png(MAX_DIMENSION + 1, 1)).is_err());
    assert!(bounded_read(Cursor::new(b"12345"), 4).is_err());
    assert_eq!(bounded_read(Cursor::new(b"1234"), 4).unwrap(), b"1234");
}

#[test]
fn invalid_response_count_base64_and_image_never_succeed_or_save() {
    let cases = vec![
        json!({}),
        json!({ "data": [] }),
        json!({ "data": [{}, {}] }),
        json!({ "data": [{ "b64_json": "!bad base64!" }] }),
        json!({ "data": [{ "b64_json": STANDARD.encode(b"<html>bad</html>") }] }),
        json!({ "data": [{ "url": "http://127.0.0.1/private" }] }),
        json!({ "data": [{ "url": "https://127.0.0.1/private?signature=hidden" }] }),
    ];
    for body in cases {
        let home = TempHome::new();
        let mock = Mock::new(move |_, _| Reply::json(body.clone()));
        let (_, input) = input(&home, &mock, 1);
        let error = execute(&input, 1, Instant::now()).err().unwrap();
        assert!(!error.message.contains(MOCK_KEY));
        assert!(!error.message.contains("hidden"));
        assert_eq!(error.request_id.as_deref(), Some("mock-request-1"));
        assert_eq!(fs::read_dir(&input.directory).unwrap().count(), 0);
        assert_eq!(mock.count(), 1);
    }
}

#[test]
fn valid_base64_wins_over_url_and_accompanying_error_does_not_discard_image() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(json!({
        "error": { "message": format!("{MOCK_KEY} signature=withheld") },
        "data": [{
            "b64_json": STANDARD.encode(png(2, 2)),
            "url": "https://127.0.0.1/never-download-this"
        }]
    })));
    let (_, input) = input(&home, &mock, 1);
    let result = execute(&input, 1, Instant::now()).unwrap();
    assert!(result.path.is_file());
    assert!(result.warning.as_ref().unwrap().contains("valid images were retained"));
    assert!(!result.warning.as_ref().unwrap().contains(MOCK_KEY));
    assert!(!result.warning.as_ref().unwrap().contains("signature"));
    assert_eq!(mock.count(), 1);
    let entry = json!({ "b64_json": STANDARD.encode(png(1, 1)), "url": "https://example.com/x" });
    assert!(response_entry_with_download(&entry, Instant::now(), |_, _| {
        panic!("valid base64 must not initiate a download")
    }).is_ok());
}

#[test]
fn invalid_base64_falls_back_to_a_clean_download_without_another_post() {
    for encoded in ["!not-base64!".to_string(), STANDARD.encode(b"not an image")] {
        let mock = Mock::new(|_, request| {
            assert_eq!(request.method, "GET");
            assert!(!request.headers.contains_key("authorization"));
            Reply {
                status: 200,
                headers: vec![("Content-Type".into(), "image/png".into())],
                body: png(2, 2),
            }
        });
        let client = client_builder(Duration::from_secs(3)).build().unwrap();
        let entry = json!({
            "b64_json": encoded,
            "url": format!("http://{}/mock-image", mock.address)
        });
        let bytes = response_entry_with_download(&entry, Instant::now(), |raw, _| {
            // Only the local mock bypasses the production HTTPS/public-DNS policy.
            download_from(&client, Url::parse(raw).unwrap(), &[mock.address])
        }).unwrap();
        assert_eq!(decode_image(&bytes).unwrap().0.width(), 2);
        assert_eq!(mock.count(), 1);
    }
    let unsafe_entry = json!({
        "b64_json": "!invalid!",
        "url": "https://127.0.0.1/blocked?signature=hidden"
    });
    let error = response_entry_image(&unsafe_entry, Instant::now()).err().unwrap();
    assert!(!error.message.contains("hidden"));
}

#[test]
fn multiple_results_preserve_every_valid_image_and_never_retry_a_successful_index() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(json!({
        "data": [
            { "b64_json": "!invalid!" },
            { "b64_json": STANDARD.encode(png(4, 4)) },
            { "b64_json": STANDARD.encode(png(5, 5)) }
        ]
    })));
    let state = ImageJobs::default();
    let (id, input) = input(&home, &mock, 1);
    let directory = input.directory.clone();
    submit(&state.shared, id.clone(), input).unwrap();
    let done = finished(&state, &id);
    assert_eq!((done.completed, done.failed), (1, 0));
    assert_eq!(done.status, "completed_with_warnings");
    assert!(done.count_mismatch);
    let item = &done.items[0];
    assert_eq!(item.status, "succeeded");
    assert!(item.warning.as_ref().unwrap().contains("provider returned 3 entries"));
    assert!(item.warning.as_ref().unwrap().contains("Saved 2 valid image(s)"));
    assert!(done.message.contains("response warnings"));
    assert_eq!(Path::new(item.path.as_ref().unwrap()).file_name().unwrap(), "1.png");
    assert_eq!(item.additional_paths.len(), 1);
    let extra = Path::new(&item.additional_paths[0]);
    assert_eq!(extra.file_name().unwrap(), "1-extra-3.png");
    assert_eq!(decode_image(&fs::read(extra).unwrap()).unwrap().0.width(), 5);
    let manifest: Value = serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["job"]["items"][0]["additionalPaths"][0], item.additional_paths[0]);
    assert!(find_job_mut(&mut lock(&state.shared).unwrap(), &id).unwrap().retry().is_err());
    assert_eq!(mock.count(), 1);
}

#[test]
fn existing_extra_names_do_not_discard_images_or_overwrite_existing_files() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(json!({
        "data": [
            { "b64_json": STANDARD.encode(png(1, 1)) },
            { "b64_json": STANDARD.encode(png(2, 2)) }
        ]
    })));
    let (_, input) = input(&home, &mock, 1);
    let extra = input.directory.join("1-extra-2.png");
    fs::write(&extra, b"existing result").unwrap();
    let result = execute(&input, 1, Instant::now()).unwrap();
    assert!(result.path.is_file());
    assert_eq!(result.additional_paths.len(), 1);
    assert_ne!(Path::new(&result.additional_paths[0]), extra.as_path());
    assert!(Path::new(&result.additional_paths[0]).is_file());
    assert!(result.warning.as_ref().unwrap().contains("Saved 2 valid image(s)"));
    assert_eq!(fs::read(extra).unwrap(), b"existing result");
    assert_eq!(mock.count(), 1);
}

#[test]
fn completed_jobs_release_reference_bytes_and_secrets_but_failed_jobs_keep_retry_inputs() {
    let _serial = WORKER_TEST.lock().unwrap();
    for succeed in [true, false] {
        let home = TempHome::new();
        let mock = Mock::new(move |_, _| Reply::json(
            if succeed { success() } else { json!({ "data": [] }) }
        ));
        let state = ImageJobs::default();
        let (id, mut input) = input(&home, &mock, 1);
        let bytes: Arc<[u8]> = png(2, 2).into();
        let weak_bytes = Arc::downgrade(&bytes);
        input.references.push(Reference { bytes, mime: "image/png", extension: "png" });
        input.request.reference_paths.push("mock-reference.png".into());
        submit(&state.shared, id.clone(), input).unwrap();
        let done = finished(&state, &id);
        assert_eq!(done.mode, "edit");
        let registry = lock(&state.shared).unwrap();
        let job = find_job(&registry, &id).unwrap();
        if succeed {
            assert!(job.input.references.is_empty());
            assert!(job.input.provider.key.is_empty());
            assert!(job.input.provider.config.is_empty());
            assert!(weak_bytes.upgrade().is_none());
            assert!(recorded_path(&job.input.directory, &done.items[0]).is_ok());
        } else {
            assert_eq!(job.input.references.len(), 1);
            assert_eq!(job.input.provider.key, MOCK_KEY);
            assert!(weak_bytes.upgrade().is_some());
        }
    }
}

#[test]
fn provider_failures_are_redacted_and_not_retried_or_redirected() {
    for status in [301, 302, 307, 308, 400, 401, 403, 404, 413, 429, 500, 503] {
        let home = TempHome::new();
        let mock = Mock::new(move |_, _| Reply {
            status,
            headers: vec![
                ("Location".into(), "http://127.0.0.1:1/leak?signature=private".into()),
                ("x-request-id".into(), MOCK_KEY.into()),
            ],
            body: format!("Authorization: {MOCK_KEY}; signature=private").into_bytes(),
        });
        let (_, input) = input(&home, &mock, 1);
        let error = execute(&input, 1, Instant::now()).err().unwrap();
        assert!(error.message.contains(&format!("HTTP {status}")));
        assert!(!error.message.contains(MOCK_KEY));
        assert!(!error.message.contains("signature"));
        assert!(error.request_id.is_none());
        assert_eq!(mock.count(), 1);
    }
}

#[test]
fn transport_timeout_reports_unknown_billing_without_raw_url() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline);
                    thread::sleep(Duration::from_millis(5));
                }
                Err(err) => panic!("timeout mock accept: {err}"),
            }
        };
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = capture(&mut stream);
        thread::sleep(Duration::from_millis(750));
    });
    let client = client_builder(Duration::from_millis(500)).build().unwrap();
    let error = client.post(format!("http://{address}/?signature=secret"))
        .body("mock-only").send().err().unwrap();
    assert!(error.is_timeout());
    let failure = network_failure(&error);
    assert!(failure.message.contains("billing is unknown"));
    assert!(!failure.message.contains("signature"));
    server.join().unwrap();
}

#[test]
fn strict_download_policy_blocks_private_and_special_targets_without_network() {
    for url in [
        "http://example.com/image.png",
        "file:///private/image.png",
        "https://user:secret@example.com/image",
        "https://example.com:8443/image",
        "https://example.com/image#fragment",
        "https://localhost/image",
        "https://example.local/image",
        "https://service.internal/image",
        "https://x.home.arpa/image",
        "https://127.0.0.1/image",
        "https://2130706433/image",
        "https://10.0.0.1/image",
        "https://100.64.0.1/image",
        "https://169.254.169.254/image",
        "https://172.16.0.1/image",
        "https://192.168.1.1/image",
        "https://198.18.0.1/image",
        "https://224.0.0.1/image",
        "https://[::1]/image",
        "https://[::ffff:127.0.0.1]/image",
        "https://[fc00::1]/image",
        "https://[fe80::1]/image",
        "https://[2002:7f00:1::]/image",
        "https://[64:ff9b::7f00:1]/image",
    ] {
        assert!(download_url(url).is_err(), "{url}");
    }
    assert!(download_url("https://cdn.example.com/image?signature=permitted-but-never-echoed").is_ok());
    assert!(public_ip("8.8.8.8".parse().unwrap()));
    assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
    assert!(!public_ip("2001:db8::1".parse().unwrap()));
    assert!(remaining(Instant::now() - TIMEOUT - Duration::from_secs(1)).is_err());
}

#[test]
fn downloads_use_clean_client_validate_mime_and_refuse_redirects() {
    for (status, mime, good) in [(200, "image/png", true), (200, "image/jpeg", false), (302, "image/png", false)] {
        let mock = Mock::new(move |_, request| {
            assert!(!request.headers.contains_key("authorization"));
            assert!(!request.headers.contains_key("cookie"));
            assert!(!request.headers.contains_key("referer"));
            Reply {
                status,
                headers: vec![
                    ("Content-Type".into(), mime.into()),
                    ("Location".into(), "http://127.0.0.1:1/private".into()),
                ],
                body: png(2, 2),
            }
        });
        // Inject a loopback transport below the production HTTPS/DNS policy.
        let client = client_builder(Duration::from_secs(3)).build().unwrap();
        let result = download_from(
            &client,
            Url::parse(&format!("http://{}/image", mock.address)).unwrap(),
            &[mock.address],
        );
        assert_eq!(result.is_ok(), good);
        assert_eq!(mock.count(), 1);
    }
}

#[test]
fn huge_count_is_lazy_cancel_finishes_inflight_and_preserves_indices() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let gate = Arc::new(Gate::default());
    let handler_gate = Arc::clone(&gate);
    let mock = Mock::new(move |_, _| {
        handler_gate.wait();
        Reply::json(success())
    });
    let state = ImageJobs::default();
    let (id, input) = input(&home, &mock, usize::MAX);
    let initial = submit(&state.shared, id.clone(), input).unwrap();
    assert_eq!(initial.total, usize::MAX);
    assert_eq!(initial.items.len(), 2);
    wait_until(|| mock.count() == 2);
    {
        let mut registry = lock(&state.shared).unwrap();
        assert_eq!(registry.workers, 2);
        let job = find_job_mut(&mut registry, &id).unwrap();
        assert_eq!(job.active, 2);
        assert!(job.retry().is_err());
        job.cancel();
        let snapshot = job.snapshot();
        assert_eq!(snapshot.status, "running");
        assert_eq!(snapshot.cancelled, usize::MAX - 2);
        assert_eq!(job.items.len(), 2);
    }
    gate.release();
    let done = finished(&state, &id);
    assert_eq!(done.status, "cancelled");
    assert_eq!(done.completed, 2);
    assert_eq!(done.failed, 0);
    assert_eq!(done.cancelled, usize::MAX - 2);
    assert_eq!(done.items.iter().map(|item| item.index).collect::<Vec<_>>(), vec![1, 2]);
    assert!(done.items.iter().all(|item| item.status == "succeeded"));
    assert_eq!(mock.count(), 2);
}

#[test]
fn partial_failure_retry_sends_only_failed_indices_and_preserves_successes() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let mock = Mock::new(|number, _| {
        if number == 2 {
            Reply::json(json!({ "data": [] }))
        } else {
            Reply::json(success())
        }
    });
    let state = ImageJobs::default();
    let (id, input) = input(&home, &mock, 4);
    submit(&state.shared, id.clone(), input).unwrap();
    let first = finished(&state, &id);
    assert_eq!(first.status, "partial");
    assert_eq!((first.completed, first.failed), (3, 1));
    let previous: Vec<_> = first.items.iter().filter(|item| item.status == "succeeded")
        .map(|item| (item.index, item.path.clone(), item.sha256.clone())).collect();
    retry_mock(&state, &id);
    let second = finished(&state, &id);
    assert_eq!(second.status, "completed");
    assert_eq!((second.completed, second.failed), (4, 0));
    assert_eq!(mock.count(), 5);
    for (index, path, sha256) in previous {
        let item = second.items.iter().find(|item| item.index == index).unwrap();
        assert_eq!(item.path, path);
        assert_eq!(item.sha256, sha256);
    }
    assert!(find_job_mut(&mut lock(&state.shared).unwrap(), &id).unwrap().retry().is_err());
    let serialized = serde_json::to_string(&second).unwrap();
    assert!(!serialized.contains(MOCK_KEY));
    assert!(!serialized.contains("apiBase"));
}

#[test]
fn all_failed_job_retries_explicitly_and_counts_are_consistent() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(json!({ "data": [] })));
    let state = ImageJobs::default();
    let (id, input) = input(&home, &mock, 3);
    submit(&state.shared, id.clone(), input).unwrap();
    let first = finished(&state, &id);
    assert_eq!(first.status, "failed");
    assert_eq!((first.completed, first.failed, first.cancelled), (0, 3, 0));
    assert_eq!(mock.count(), 3);
    retry_mock(&state, &id);
    let second = finished(&state, &id);
    assert_eq!(second.status, "failed");
    assert_eq!((second.completed, second.failed, second.cancelled), (0, 3, 0));
    assert_eq!(mock.count(), 6);
}

#[test]
fn cancellation_retry_resumes_only_unfinished_queue() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let gate = Arc::new(Gate::default());
    let handler_gate = Arc::clone(&gate);
    let mock = Mock::new(move |_, _| {
        handler_gate.wait();
        Reply::json(success())
    });
    let state = ImageJobs::default();
    let (id, input) = input(&home, &mock, 5);
    submit(&state.shared, id.clone(), input).unwrap();
    wait_until(|| mock.count() == 2);
    find_job_mut(&mut lock(&state.shared).unwrap(), &id).unwrap().cancel();
    gate.release();
    let cancelled = finished(&state, &id);
    assert_eq!((cancelled.completed, cancelled.cancelled), (2, 3));
    retry_mock(&state, &id);
    let done = finished(&state, &id);
    assert_eq!((done.completed, done.cancelled, done.failed), (5, 0, 0));
    assert_eq!(mock.count(), 5);
}

#[test]
fn worker_limit_is_global_across_multiple_jobs() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let handler_active = Arc::clone(&active);
    let handler_maximum = Arc::clone(&maximum);
    let gate = Arc::new(Gate::default());
    let handler_gate = Arc::clone(&gate);
    let mock = Mock::new(move |_, _| {
        let active = handler_active.fetch_add(1, Ordering::SeqCst) + 1;
        handler_maximum.fetch_max(active, Ordering::SeqCst);
        handler_gate.wait();
        handler_active.fetch_sub(1, Ordering::SeqCst);
        Reply::json(success())
    });
    let state = ImageJobs::default();
    let (first, first_input) = input(&home, &mock, 3);
    let (second, second_input) = input(&home, &mock, 3);
    submit(&state.shared, first.clone(), first_input).unwrap();
    submit(&state.shared, second.clone(), second_input).unwrap();
    wait_until(|| mock.count() == 2 && maximum.load(Ordering::SeqCst) == 2);
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    gate.release();
    assert_eq!(finished(&state, &first).completed, 3);
    assert_eq!(finished(&state, &second).completed, 3);
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    assert_eq!(mock.count(), 6);
}

#[test]
fn recorded_paths_are_contained_and_unknown_indices_cannot_be_opened() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let (_, input) = input(&home, &mock, 1);
    let result = execute(&input, 1, Instant::now()).unwrap();
    let mut item = ImageItem::new(1, "succeeded");
    item.path = Some(result.path.to_string_lossy().into_owned());
    assert_eq!(recorded_path(&input.directory, &item).unwrap(), result.path);
    item.index = 2;
    assert!(recorded_path(&input.directory, &item).is_err());
    item.index = 1;
    item.status = "failed".into();
    assert!(recorded_path(&input.directory, &item).is_err());
    item.status = "succeeded".into();
    let external = home.0.join("1.png");
    fs::write(&external, png(1, 1)).unwrap();
    item.path = Some(external.to_string_lossy().into_owned());
    assert!(recorded_path(&input.directory, &item).is_err());
    item.path = Some(input.directory.join("..").join("1.png").to_string_lossy().into_owned());
    assert!(recorded_path(&input.directory, &item).is_err());
    let registry = Registry::default();
    assert!(find_job(&registry, "../../outside").is_err());
    let (another_id, another_dir) = create_directory(&home.0).unwrap();
    assert_ne!(another_dir, input.directory);
    assert!(!another_id.contains('/'));
}

#[cfg(unix)]
#[test]
fn symlink_output_roots_and_results_are_rejected() {
    use std::os::unix::fs::symlink;
    let home = TempHome::new();
    let outside = TempHome::new();
    symlink(&outside.0, home.0.join("oceanway-image-tests")).unwrap();
    assert!(create_directory(&home.0).is_err());
    fs::remove_file(home.0.join("oceanway-image-tests")).unwrap();
    let (_, directory) = create_directory(&home.0).unwrap();
    let target = outside.0.join("1.png");
    fs::write(&target, png(1, 1)).unwrap();
    let link = directory.join("1.png");
    symlink(target, &link).unwrap();
    let mut item = ImageItem::new(1, "succeeded");
    item.path = Some(link.to_string_lossy().into_owned());
    assert!(recorded_path(&directory, &item).is_err());
}

fn record_without_workers(state: &ImageJobs, id: String, input: Input, leased: bool) {
    let mut job = StoredJob::new(id.clone(), input);
    job.leased = leased;
    job.renew_lease();
    persist_manifest(&job).unwrap();
    register_manifest(&job).unwrap();
    let mut registry = lock(&state.shared).unwrap();
    registry.home = Some(job.input.provider.home.clone());
    registry.jobs.insert(id, job);
}

fn save_provider_fixture(home: &Path, key: &str) {
    fs::write(home.join("config.toml"), format!(
        "model_provider = \"OceanWay\"\n[model_providers.OceanWay]\n\
         base_url = \"https://example.invalid/v1\"\nexperimental_bearer_token = \"{key}\"\n"
    )).unwrap();
}

#[test]
fn per_slot_prompts_default_validate_and_reach_generation_and_edit_in_order() {
    let parsed: ImageTestRequest = serde_json::from_value(json!({
        "count": 2, "prompts": ["first subject", "second subject"]
    })).unwrap();
    assert!(parsed.prompt.is_empty());
    assert_eq!(validate_request(parsed).unwrap().prompts.len(), 2);
    for prompts in [vec!["one"], vec!["one", ""], vec!["one", "two", "three"]] {
        let mut invalid = request(2);
        invalid.prompts = prompts.into_iter().map(String::from).collect();
        assert!(validate_request(invalid).is_err());
    }
    let legacy = validate_request(request(usize::MAX)).unwrap();
    assert!(legacy.prompts.is_empty(), "shared prompt never allocates count slots");
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let (_, mut input) = input(&home, &mock, 3);
    input.request.prompt = "shared fallback not used".into();
    input.request.prompts = vec!["first subject".into(), "second subject".into(), "edit subject".into()];
    execute(&input, 1, Instant::now()).unwrap();
    execute(&input, 2, Instant::now()).unwrap();
    input.references.push(Reference { bytes: png(1, 1).into(), mime: "image/png", extension: "png" });
    execute(&input, 3, Instant::now()).unwrap();
    let captured = mock.requests.lock().unwrap();
    for (position, expected) in ["first subject", "second subject"].iter().enumerate() {
        assert_eq!(serde_json::from_slice::<Value>(&captured[position].body).unwrap()["prompt"], *expected);
    }
    let body = String::from_utf8_lossy(&captured[2].body);
    assert!(body.contains("name=\"prompt\"\r\n\r\nedit subject\r\n"));
    assert!(!body.contains("shared fallback"));
}

#[test]
fn task_output_layout_ordered_original_hashes_and_result_metadata_survive_recovery() {
    let home = TempHome::new();
    let workspace = TempHome::new();
    let attachments = TempHome::new();
    let first = attachments.0.join("subject.png");
    let second = attachments.0.join("style.png");
    let original = png(2, 3);
    fs::write(&first, &original).unwrap();
    fs::write(&second, png(5, 7)).unwrap();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let (_, mut input) = input(&home, &mock, 1);
    let (id, directory) = create_workspace_directory(&workspace.0).unwrap();
    input.directory = directory.clone();
    input.request.reference_paths = [&second, &first, &second].into_iter()
        .map(|path| path.to_string_lossy().into_owned()).collect();
    input.references = load_references_in_home(&input.request.reference_paths, &home.0).unwrap();
    let mut job = StoredJob::new(id.clone(), input);
    let slot = job.claim().unwrap();
    let result = execute(&job.input, slot, Instant::now()).unwrap();
    finish_item(&mut job, slot, Ok(result), 1);
    persist_manifest(&job).unwrap();
    register_manifest(&job).unwrap();
    assert_eq!(directory, workspace.0.join("output").join("images").join(&id));
    let reference_paths = job.input.request.reference_paths.clone();
    drop(job);
    let reopened = ImageJobs::new(&home.0).unwrap();
    let saved = reopened.status(&id).unwrap();
    assert_eq!(saved.status, "completed");
    assert_eq!(saved.reference_hashes, vec![hash(&png(5, 7)), hash(&original), hash(&png(5, 7))]);
    assert_eq!(saved.reference_paths, reference_paths);
    assert_eq!(saved.output_directory, directory.to_string_lossy());
    let item = &saved.items[0];
    assert_eq!((item.width, item.height), (Some(320), Some(160)));
    assert_eq!(item.sha256, Some(hash(&png(320, 160))));
    assert_eq!(item.outputs[0].bytes, png(320, 160).len() as u64);
    assert!(item.preview_data_url.is_none());
    assert!(!reopened.has_active_requests());
    assert!(reopened.retry(&id).is_err());
    let registry = lock(&reopened.shared).unwrap();
    let recovered = find_job(&registry, &id).unwrap();
    assert!(recovered.input.provider.key.is_empty());
    assert!(recovered.input.provider.config.is_empty());
    assert_eq!(registry.workers, 0, "recovery never starts the pool");
    assert_eq!(mock.count(), 1);
}

#[test]
fn sha256_is_the_standard_digest_and_extra_outputs_have_individual_evidence() {
    assert_eq!(hash(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(json!({ "data": [
        { "b64_json": STANDARD.encode(png(3, 4)) },
        { "b64_json": STANDARD.encode(png(5, 6)) }
    ] })));
    let (id, input) = input(&home, &mock, 1);
    let mut job = StoredJob::new(id.clone(), input);
    let slot = job.claim().unwrap();
    let result = execute(&job.input, slot, Instant::now()).unwrap();
    assert_eq!(result.outputs.len(), 2);
    assert_eq!(result.outputs[1].sha256, hash(&png(5, 6)));
    assert_eq!((result.outputs[1].width, result.outputs[1].height), (5, 6));
    finish_item(&mut job, slot, Ok(result), 1);
    persist_manifest(&job).unwrap();
    register_manifest(&job).unwrap();
    drop(job);
    let reopened = ImageJobs::new(&home.0).unwrap();
    let snapshot = reopened.status(&id).unwrap();
    assert_eq!(snapshot.status, "completed_with_warnings");
    assert!(snapshot.count_mismatch);
    assert!(snapshot.items[0].count_mismatch);
    assert!(!snapshot.warnings.is_empty());
}

#[test]
fn recovery_marks_running_unknown_and_only_explicit_retry_posts_missing_slots_with_current_key() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let mock = Mock::new(|number, request| {
        let expected = if number == 1 { MOCK_KEY } else { "new-mock-credential" };
        assert_eq!(request.headers["authorization"], format!("Bearer {expected}"));
        Reply::json(success())
    });
    let (id, mut input) = input(&home, &mock, 3);
    input.request.prompts = vec!["retained".into(), "unknown".into(), "queued".into()];
    let mut provider = input.provider.clone();
    provider.key = "new-mock-credential".into();
    let mut job = StoredJob::new(id.clone(), input);
    let first = job.claim().unwrap();
    let result = execute(&job.input, first, Instant::now()).unwrap();
    let saved_path = result.path.clone();
    finish_item(&mut job, first, Ok(result), 1);
    assert_eq!(job.claim(), Some(2));
    job.items.get_mut(&2).unwrap().post_started = true;
    persist_manifest(&job).unwrap();
    register_manifest(&job).unwrap();
    drop(job);
    let service = ImageJobs::new(&home.0).unwrap();
    let recovered = service.status(&id).unwrap();
    assert_eq!(recovered.status, "interrupted");
    assert_eq!((recovered.completed, recovered.failed, recovered.outcome_unknown), (1, 1, 1));
    assert_eq!(recovered.items[1].status, "uncertain");
    assert!(!service.has_active_requests());
    thread::sleep(Duration::from_millis(150));
    assert_eq!(mock.count(), 1, "status/recovery must not send HTTP");
    service.retry_with_loader(&id, |_| Ok(provider)).unwrap();
    let done = finished(&service, &id);
    assert_eq!((done.completed, done.failed), (3, 0));
    assert_eq!(done.status, "completed");
    assert_eq!(Path::new(done.items[0].path.as_ref().unwrap()), saved_path);
    assert_eq!(mock.count(), 3);
    let captured = mock.requests.lock().unwrap();
    let mut retried: Vec<_> = captured[1..].iter().map(|request|
        serde_json::from_slice::<Value>(&request.body).unwrap()["prompt"].as_str().unwrap().to_owned()).collect();
    retried.sort();
    assert_eq!(retried, vec!["queued", "unknown"]);
}

#[test]
fn recovery_hash_failure_blocks_reposting_even_after_output_is_removed() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let (id, input) = input(&home, &mock, 1);
    let mut job = StoredJob::new(id.clone(), input);
    let slot = job.claim().unwrap();
    let result = execute(&job.input, slot, Instant::now()).unwrap();
    let output = result.path.clone();
    finish_item(&mut job, slot, Ok(result), 1);
    persist_manifest(&job).unwrap();
    register_manifest(&job).unwrap();
    fs::write(&output, png(1, 1)).unwrap();
    drop(job);
    let reopened = ImageJobs::new(&home.0).unwrap();
    let snapshot = reopened.status(&id).unwrap();
    assert_eq!((snapshot.completed, snapshot.failed), (0, 1));
    assert_eq!(snapshot.items[0].status, "integrity_failed");
    assert!(snapshot.items[0].retry_blocked);
    fs::remove_file(output).unwrap();
    assert!(find_job_mut(&mut lock(&reopened.shared).unwrap(), &id).unwrap().retry().is_err());
    assert_eq!(mock.count(), 1);
}

#[test]
fn status_hash_checks_primary_and_extra_files_without_reporting_clean_completion() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(json!({ "data": [
        { "b64_json": STANDARD.encode(png(1, 1)) },
        { "b64_json": STANDARD.encode(png(2, 2)) }
    ] })));
    let (id, input) = input(&home, &mock, 1);
    let state = ImageJobs::default();
    record_without_workers(&state, id.clone(), input, false);
    {
        let mut registry = lock(&state.shared).unwrap();
        let job = find_job_mut(&mut registry, &id).unwrap();
        let slot = job.claim().unwrap();
        let result = execute(&job.input, slot, Instant::now()).unwrap();
        let extra = result.additional_paths[0].clone();
        finish_item(job, slot, Ok(result), 1);
        persist_manifest(job).unwrap();
        fs::remove_file(extra).unwrap();
    }
    let status = state.status(&id).unwrap();
    assert_eq!(status.status, "paused");
    assert_eq!(status.items[0].status, "integrity_failed");
    assert_eq!(status.completed, 0);
    assert_eq!(mock.count(), 1);
}

#[test]
fn interrupted_slot_with_uncommitted_output_is_preserved_and_never_reposted() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("readback test must not POST"));
    let (id, input) = input(&home, &mock, 1);
    let mut job = StoredJob::new(id.clone(), input);
    job.claim().unwrap();
    persist_manifest(&job).unwrap();
    register_manifest(&job).unwrap();
    let orphan = job.input.directory.join("1.png");
    fs::write(&orphan, png(9, 11)).unwrap();
    drop(job);
    let recovered = ImageJobs::new(&home.0).unwrap();
    let snapshot = recovered.status(&id).unwrap();
    assert_eq!(snapshot.outcome_unknown, 1);
    assert!(snapshot.items[0].retry_blocked);
    assert_eq!((snapshot.items[0].width, snapshot.items[0].height), (Some(9), Some(11)));
    assert_eq!(snapshot.items[0].sha256, Some(hash(&png(9, 11))));
    assert!(find_job_mut(&mut lock(&recovered.shared).unwrap(), &id).unwrap().retry().is_err());
    assert_eq!(fs::read(orphan).unwrap(), png(9, 11));
    assert_eq!(mock.count(), 0);
}

#[test]
fn recovery_only_reads_index_referenced_owned_manifests() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("recovery must not POST"));
    let (id, input) = input(&home, &mock, 1);
    let job = StoredJob::new(id.clone(), input);
    persist_manifest(&job).unwrap();
    let service = ImageJobs::new(&home.0).unwrap();
    assert!(lock(&service.shared).unwrap().jobs.is_empty(), "unindexed output is not discovered");
    assert!(service.status(&id).is_err());
    register_manifest(&job).unwrap();
    let path = job.input.directory.join("manifest.json");
    let original = fs::read(&path).unwrap();
    drop(job);
    let index_path = home.0.join("oceanway-image-jobs").join("index.json");
    let original_index = fs::read(&index_path).unwrap();
    for (key, value) in [
        ("ownershipToken", json!("wrong owner")),
        ("codexHome", json!(home.0.join("another-home"))),
        ("engine", json!("foreign engine")),
        ("outputDirectory", json!(home.0)),
        ("schemaVersion", json!(1)),
    ] {
        let mut manifest: Value = serde_json::from_slice(&original).unwrap();
        manifest[key] = value;
        let invalid = serde_json::to_vec(&manifest).unwrap();
        fs::write(&path, &invalid).unwrap();
        let service = ImageJobs::new(&home.0).unwrap();
        assert!(service.status(&id).is_err(), "{key}");
        assert_eq!(fs::read(&path).unwrap(), invalid, "{key}");
        assert_eq!(fs::read(&index_path).unwrap(), original_index, "{key}");
    }
    fs::write(&path, &original).unwrap();
    let recovered = ImageJobs::new(&home.0).unwrap();
    assert_eq!(recovered.status(&id).unwrap().status, "interrupted");
    assert_eq!(mock.count(), 0);
}

#[test]
fn manifests_and_index_do_not_serialize_credentials_or_config_snapshots() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("record-only test must not POST"));
    let (id, mut input) = input(&home, &mock, 2);
    input.request.prompts = vec!["one".into(), "two".into()];
    input.provider.config = "full-private-config-snapshot".into();
    let job = StoredJob::new(id, input);
    persist_manifest(&job).unwrap();
    register_manifest(&job).unwrap();
    for path in [
        job.input.directory.join("manifest.json"),
        home.0.join("oceanway-image-jobs").join("index.json"),
    ] {
        let text = fs::read_to_string(path).unwrap();
        assert!(!text.contains(MOCK_KEY));
        assert!(!text.contains("full-private-config-snapshot"));
        assert!(!text.contains("previewDataUrl"));
        assert!(!text.contains("apiBase"));
    }
}

#[test]
fn cancellation_after_body_preflight_prevents_post_and_drains_active_accounting() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("cancelled preflight must not POST"));
    let (id, input) = input(&home, &mock, 4);
    let service = ImageJobs::default();
    record_without_workers(&service, id.clone(), input, false);
    let input = {
        let mut registry = lock(&service.shared).unwrap();
        let job = find_job_mut(&mut registry, &id).unwrap();
        assert_eq!(job.claim(), Some(1));
        Arc::clone(&job.input)
    };
    let ready = Arc::new(Gate::default());
    let resume = Arc::new(Gate::default());
    let thread_ready = Arc::clone(&ready);
    let thread_resume = Arc::clone(&resume);
    let shared = Arc::clone(&service.shared);
    let thread_id = id.clone();
    let handle = thread::spawn(move || {
        let outcome = execute_with_guard(&input, 1, Instant::now(), || {
            thread_ready.release();
            thread_resume.wait();
            authorize_post(&shared, &thread_id, 1, &input)
        });
        assert!(outcome.as_ref().err().unwrap().not_sent);
        let mut registry = lock(&shared).unwrap();
        let job = find_job_mut(&mut registry, &thread_id).unwrap();
        finish_item(job, 1, outcome, 1);
        persist_or_stop(job).unwrap();
    });
    ready.wait();
    assert!(service.has_active_requests());
    service.cancel(&id).unwrap();
    resume.release();
    handle.join().unwrap();
    let status = service.status(&id).unwrap();
    assert_eq!((status.completed, status.failed, status.cancelled), (0, 0, 4));
    assert!(!service.has_active_requests());
    assert_eq!(mock.count(), 0);
}

#[test]
fn provider_reload_at_final_guard_uses_explicit_home_and_blocks_changed_config() {
    let home = TempHome::new();
    save_provider_fixture(&home.0, MOCK_KEY);
    let mock = Mock::new(|_, _| panic!("changed provider must not POST"));
    let (id, mut input) = input(&home, &mock, 1);
    input.provider = load_provider_in_home(&home.0).unwrap();
    let service = ImageJobs::default();
    record_without_workers(&service, id.clone(), input, false);
    let input = {
        let mut registry = lock(&service.shared).unwrap();
        let job = find_job_mut(&mut registry, &id).unwrap();
        job.claim().unwrap();
        Arc::clone(&job.input)
    };
    let result = execute_with_guard(&input, 1, Instant::now(), || {
        fs::write(home.0.join("config.toml"), "model_provider = 'other'\n").unwrap();
        authorize_post(&service.shared, &id, 1, &input)
    });
    let failure = result.err().unwrap();
    assert!(failure.stop_job && failure.not_sent);
    assert!(failure.message.contains("no POST was sent"));
    assert!(!failure.message.contains(MOCK_KEY));
    assert_eq!(mock.count(), 0);
    let mut registry = lock(&service.shared).unwrap();
    let job = find_job_mut(&mut registry, &id).unwrap();
    finish_item(job, 1, Err(failure), 1);
    assert_eq!((job.snapshot().failed, job.snapshot().cancelled), (0, 1));
}

#[test]
fn retry_preflight_cannot_undo_a_concurrent_cancel() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("cancel must win retry preflight"));
    let (id, input) = input(&home, &mock, 2);
    let provider = input.provider.clone();
    let service = ImageJobs::default();
    record_without_workers(&service, id.clone(), input, false);
    service.cancel(&id).unwrap();
    let result = service.retry_with_loader(&id, |_| {
        service.cancel(&id).unwrap();
        Ok(provider)
    });
    assert!(result.err().unwrap().contains("changed during retry preflight"));
    assert_eq!(service.status(&id).unwrap().status, "cancelled");
    assert_eq!(mock.count(), 0);
}

#[test]
fn changed_reference_hashes_block_explicit_retry_before_http() {
    let home = TempHome::new();
    let attachments = TempHome::new();
    let path = attachments.0.join("source.png");
    fs::write(&path, png(1, 1)).unwrap();
    let mock = Mock::new(|_, _| panic!("changed references must not POST"));
    let (id, mut input) = input(&home, &mock, 1);
    input.request.reference_paths = vec![path.to_string_lossy().into_owned()];
    input.references = load_references_in_home(&input.request.reference_paths, &home.0).unwrap();
    let provider = input.provider.clone();
    let service = ImageJobs::default();
    record_without_workers(&service, id.clone(), input, false);
    service.cancel(&id).unwrap();
    fs::write(path, png(2, 2)).unwrap();
    assert!(service.retry_with_loader(&id, |_| Ok(provider)).err().unwrap().contains("hashes changed"));
    assert_eq!(mock.count(), 0);
}

#[test]
fn expired_lease_pauses_and_status_renews_without_implicitly_resuming() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("lease state test must not POST"));
    let (id, input) = input(&home, &mock, usize::MAX);
    let service = ImageJobs::default();
    record_without_workers(&service, id.clone(), input, true);
    {
        let mut registry = lock(&service.shared).unwrap();
        let job = find_job_mut(&mut registry, &id).unwrap();
        job.lease_deadline = Some(Instant::now() + Duration::from_secs(1));
    }
    let renewed = service.status(&id).unwrap();
    assert!(renewed.lease_remaining_ms.unwrap() > 119_000);
    assert!(!renewed.paused);
    {
        let mut registry = lock(&service.shared).unwrap();
        let job = find_job_mut(&mut registry, &id).unwrap();
        job.lease_deadline = Some(Instant::now() - Duration::from_millis(1));
        assert!(job.claim().is_none(), "claim itself enforces the lease");
    }
    let paused = service.status(&id).unwrap();
    assert_eq!(paused.status, "paused");
    assert!(paused.leased);
    assert!(paused.lease_remaining_ms.unwrap() > 119_000);
    assert_eq!(paused.items.len(), 2);
    assert_eq!(mock.count(), 0);
}

#[test]
fn workers_pause_expired_queue_without_polls_and_do_not_interrupt_admitted_requests() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let gate = Arc::new(Gate::default());
    let handler_gate = Arc::clone(&gate);
    let mock = Mock::new(move |_, _| { handler_gate.wait(); Reply::json(success()) });
    let (id, input) = input(&home, &mock, 5);
    let service = ImageJobs::default();
    submit_leased(&service.shared, id.clone(), input, true).unwrap();
    wait_until(|| mock.count() == 2);
    {
        let mut registry = lock(&service.shared).unwrap();
        find_job_mut(&mut registry, &id).unwrap().lease_deadline =
            Some(Instant::now() - Duration::from_secs(1));
    }
    gate.release();
    wait_until(|| {
        let registry = lock(&service.shared).unwrap();
        let job = find_job(&registry, &id).unwrap();
        job.paused && job.active == 0
    });
    let status = service.status(&id).unwrap();
    assert_eq!(status.status, "paused");
    assert_eq!((status.completed, status.failed), (2, 0));
    assert_eq!(mock.count(), 2);
    let manifest: Value = serde_json::from_slice(
        &fs::read(Path::new(&status.output_directory).join("manifest.json")).unwrap()
    ).unwrap();
    assert_eq!(manifest["job"]["paused"], true);
}

#[test]
fn cancel_all_stops_future_submissions_but_waits_for_inflight_requests() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let gate = Arc::new(Gate::default());
    let handler_gate = Arc::clone(&gate);
    let mock = Mock::new(move |_, _| { handler_gate.wait(); Reply::json(success()) });
    let (id, first_input) = input(&home, &mock, 5);
    let service = ImageJobs::default();
    submit(&service.shared, id.clone(), first_input).unwrap();
    wait_until(|| mock.count() == 2);
    service.cancel_all();
    assert!(service.has_active_requests());
    let (next_id, next_input) = input(&home, &mock, 1);
    assert!(submit(&service.shared, next_id, next_input).is_err());
    gate.release();
    wait_until(|| !service.has_active_requests());
    let result = service.status(&id).unwrap();
    assert_eq!((result.completed, result.cancelled), (2, 3));
    assert_eq!(mock.count(), 2);
}

#[test]
fn the_two_worker_limit_is_process_global_across_independent_services() {
    let _serial = WORKER_TEST.lock().unwrap();
    let first_home = TempHome::new();
    let second_home = TempHome::new();
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let handler_active = Arc::clone(&active);
    let handler_maximum = Arc::clone(&maximum);
    let gate = Arc::new(Gate::default());
    let handler_gate = Arc::clone(&gate);
    let mock = Mock::new(move |_, _| {
        let count = handler_active.fetch_add(1, Ordering::SeqCst) + 1;
        handler_maximum.fetch_max(count, Ordering::SeqCst);
        handler_gate.wait();
        handler_active.fetch_sub(1, Ordering::SeqCst);
        Reply::json(success())
    });
    let first = ImageJobs::default();
    let second = ImageJobs::default();
    let (first_id, first_input) = input(&first_home, &mock, 3);
    let (second_id, second_input) = input(&second_home, &mock, 3);
    submit(&first.shared, first_id.clone(), first_input).unwrap();
    submit(&second.shared, second_id.clone(), second_input).unwrap();
    wait_until(|| mock.count() == 2);
    thread::sleep(Duration::from_millis(150));
    assert_eq!(mock.count(), 2);
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    gate.release();
    assert_eq!(finished(&first, &first_id).completed, 3);
    assert_eq!(finished(&second, &second_id).completed, 3);
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
}

#[test]
fn preview_memory_and_response_text_are_bounded_per_job() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("preview-only test must not POST"));
    let (id, input) = input(&home, &mock, 12);
    let mut job = StoredJob::new(id, input);
    for index in 1..=12 {
        let mut item = ImageItem::new(index, "succeeded");
        item.preview_data_url = Some(format!("data:image/png;base64,{}", "A".repeat(PREVIEW_BUDGET / 2)));
        job.items.insert(index, item);
    }
    let result = job.snapshot();
    let previews: Vec<_> = result.items.iter().filter_map(|item| item.preview_data_url.as_ref()).collect();
    assert!(previews.len() <= MAX_PREVIEWS);
    assert!(previews.iter().map(|preview| preview.len()).sum::<usize>() <= PREVIEW_BUDGET);
    persist_manifest(&job).unwrap();
    assert!(!fs::read_to_string(job.input.directory.join("manifest.json")).unwrap().contains("previewDataUrl"));
}

#[test]
fn persistence_failure_is_reported_without_claiming_progress_was_saved() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("persistence state test must not POST"));
    let (id, input) = input(&home, &mock, 2);
    let mut job = StoredJob::new(id, input);
    persist_manifest(&job).unwrap();
    let path = job.input.directory.join("manifest.json");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let error = persist_or_stop(&mut job).err().unwrap();
    assert!(error.contains("Could not replace"));
    let status = job.snapshot();
    assert_eq!(status.persistence_error.as_deref(), Some(error.as_str()));
    assert_eq!(status.status, "cancelled");
    assert_eq!(status.cancelled, 2);
    assert!(!status.message.contains("Progress is saved"));
}

#[test]
fn index_write_failure_prevents_any_new_worker_submission() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("failed index registration must not POST"));
    let (id, input) = input(&home, &mock, 1);
    fs::create_dir(home.0.join("oceanway-image-jobs")).unwrap();
    fs::create_dir(home.0.join("oceanway-image-jobs").join("index.json")).unwrap();
    let state = ImageJobs::default();
    assert!(submit(&state.shared, id, input).is_err());
    assert!(lock(&state.shared).unwrap().jobs.is_empty());
    assert!(!state.has_active_requests());
    assert_eq!(mock.count(), 0);
}

#[test]
fn original_regular_attachments_outside_home_are_allowed_but_credential_paths_are_not() {
    let home = TempHome::new();
    let outside = TempHome::new();
    let attachment = outside.0.join("actual-upload.png");
    fs::write(&attachment, png(1, 1)).unwrap();
    assert!(load_references_in_home(&[attachment.to_string_lossy().into_owned()], &home.0).is_ok());
    for name in ["config.toml", "auth.json", ".env", "credentials.json", "config.toml.backup"] {
        for root in [&home.0, &outside.0] {
            let path = root.join(name);
            fs::write(&path, png(1, 1)).unwrap();
            assert!(load_references_in_home(&[path.to_string_lossy().into_owned()], &home.0).is_err());
        }
    }
    assert!(load_references_in_home(&[outside.0.to_string_lossy().into_owned()], &home.0).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_originals_owned_indexes_manifests_and_workspace_roots_are_rejected() {
    use std::os::unix::fs::symlink;
    let home = TempHome::new();
    let outside = TempHome::new();
    let original = outside.0.join("original.png");
    fs::write(&original, png(1, 1)).unwrap();
    let link = outside.0.join("link.png");
    symlink(&original, &link).unwrap();
    assert!(load_references_in_home(&[link.to_string_lossy().into_owned()], &home.0).is_err());
    symlink(&outside.0, home.0.join("output")).unwrap();
    assert!(create_workspace_directory(&home.0).is_err());
    symlink(&outside.0, home.0.join("oceanway-image-jobs")).unwrap();
    assert!(ImageJobs::new(&home.0).is_err());
}

#[test]
fn configured_timeout_and_public_timeout_errors_agree_on_240_seconds() {
    assert_eq!(TIMEOUT, Duration::from_secs(240));
    let error = remaining(Instant::now() - TIMEOUT - Duration::from_millis(1)).err().unwrap();
    assert!(error.message.contains("240s"));
    assert!(!error.message.contains("180s"));
}

#[test]
fn auth_json_is_authoritative_without_a_shell_environment_requirement() {
    let home = TempHome::new();
    save_provider_fixture(&home.0, "legacy-inline-token");
    fs::write(home.0.join("auth.json"), json!({ "OPENAI_API_KEY": "saved-auth-json-token" }).to_string()).unwrap();
    let provider = load_provider_in_home(&home.0).unwrap();
    assert_eq!(provider.key, "saved-auth-json-token");
    fs::write(home.0.join("config.toml"),
        "model_provider = 'OceanWay'\n[model_providers.OceanWay]\n\
         base_url = 'https://example.invalid/v1'\nenv_key = 'MUST_NOT_REQUIRE_THIS_VARIABLE'\n"
    ).unwrap();
    assert_eq!(load_provider_in_home(&home.0).unwrap().key, "saved-auth-json-token");
    save_provider_fixture(&home.0, "legacy-inline-token");
    fs::write(home.0.join("auth.json"), json!({ "OPENAI_API_KEY": "" }).to_string()).unwrap();
    assert!(load_provider_in_home(&home.0).is_err(), "invalid auth.json must not resurrect an old inline key");
}

#[test]
fn final_dispatch_rebinds_current_auth_json_key_after_body_and_manifest_preflight() {
    let home = TempHome::new();
    save_provider_fixture(&home.0, "legacy-inline-token");
    fs::write(home.0.join("auth.json"), json!({ "OPENAI_API_KEY": "first-auth-key" }).to_string()).unwrap();
    let mock = Mock::new(|number, request| {
        let expected = if number == 1 { "latest-during-preflight" } else { "updated-between-slots" };
        assert_eq!(request.headers["authorization"], format!("Bearer {expected}"));
        Reply::json(success())
    });
    let (id, input) = input(&home, &mock, 2);
    let service = ImageJobs::default();
    record_without_workers(&service, id.clone(), input, false);
    for index in 1..=2 {
        let input = {
            let mut registry = lock(&service.shared).unwrap();
            let job = find_job_mut(&mut registry, &id).unwrap();
            assert_eq!(job.claim(), Some(index));
            Arc::clone(&job.input)
        };
        let mut checks = 0;
        let result = execute_with_guard(&input, index, Instant::now(), || {
            authorize_post_with_loader(&service.shared, &id, index, &input, || {
                checks += 1;
                if index == 1 && checks == 2 {
                    fs::write(home.0.join("auth.json"),
                        json!({ "OPENAI_API_KEY": "latest-during-preflight" }).to_string()).unwrap();
                }
                let mut provider = load_provider_in_home(&home.0).map_err(Failure::from)?;
                // Only test transport changes; the saved auth loader is the production one.
                provider.api_base = mock.base.clone();
                provider.local_mock = true;
                Ok(provider)
            })
        }).unwrap();
        assert_eq!(checks, 2);
        let mut registry = lock(&service.shared).unwrap();
        let job = find_job_mut(&mut registry, &id).unwrap();
        finish_item(job, index, Ok(result), 1);
        persist_or_stop(job).unwrap();
        fs::write(home.0.join("auth.json"),
            json!({ "OPENAI_API_KEY": "updated-between-slots" }).to_string()).unwrap();
    }
    assert_eq!(service.status(&id).unwrap().completed, 2);
    let registry = lock(&service.shared).unwrap();
    let job = find_job(&registry, &id).unwrap();
    let manifest = fs::read_to_string(job.input.directory.join("manifest.json")).unwrap();
    for key in ["first-auth-key", "latest-during-preflight", "updated-between-slots"] {
        assert!(!manifest.contains(key));
    }
    assert_eq!(mock.count(), 2);
}

#[test]
fn independent_job_handles_cannot_write_or_recover_a_live_registry_entry() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("ownership test must not POST"));
    let (id, input) = input(&home, &mock, 2);
    let duplicate = StoredJob::new(id.clone(), input.clone());
    let directory = input.directory.clone();
    let owner = ImageJobs::default();
    record_without_workers(&owner, id.clone(), input, false);
    let manifest = directory.join("manifest.json");
    let before = fs::read(&manifest).unwrap();
    assert!(try_job_ownership(&home.0, &id).unwrap().is_none());
    assert!(persist_manifest(&duplicate).is_err());
    assert!(register_manifest(&duplicate).is_err());
    let observer = ImageJobs::new(&home.0).unwrap();
    assert!(observer.status(&id).is_err());
    observer.cancel_all();
    assert_eq!(fs::read(&manifest).unwrap(), before);
    owner.cancel_all();
    assert!(try_job_ownership(&home.0, &id).unwrap().is_none(),
        "shutdown must retain ownership until the registry lifecycle ends");
    let clone = owner.clone();
    drop(owner);
    assert!(try_job_ownership(&home.0, &id).unwrap().is_none());
    drop(clone);
    let recovered = ImageJobs::new(&home.0).unwrap();
    assert!(recovered.status(&id).is_ok());
    assert!(!recovered.has_active_requests());
    assert_eq!(mock.count(), 0);
}

struct LockTestChild(Option<std::process::Child>);

impl LockTestChild {
    fn spawn(home: &Path, mode: &str) -> Self {
        use std::process::{Command, Stdio};
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "image_api::tests::cross_process_image_lock_helper", "--nocapture"])
            .env("OCEANWAY_LOCK_TEST_HOME", home)
            .env("OCEANWAY_LOCK_TEST_MODE", mode)
            .current_dir(home)
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        Self(Some(command.spawn().unwrap()))
    }

    fn id(&self) -> u32 {
        self.0.as_ref().unwrap().id()
    }

    fn finish(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "image lock test child did not finish");
            thread::sleep(Duration::from_millis(10));
        }
        let output = self.0.take().unwrap().wait_with_output().unwrap();
        assert!(output.status.success(), "child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }

    fn kill_and_wait(&mut self) {
        let mut child = self.0.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

impl Drop for LockTestChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// Invoked only by this executable's subprocess tests. No ImageJobs workers or
// network clients are started, and all paths are explicit temporary fixtures.
#[test]
fn cross_process_image_lock_helper() {
    let Some(home) = std::env::var_os("OCEANWAY_LOCK_TEST_HOME").map(PathBuf::from) else { return };
    let mode = std::env::var("OCEANWAY_LOCK_TEST_MODE").unwrap();
    assert!(mode == "hold" || mode == "write");
    let make_job = || {
        let (id, directory) = create_directory(&home).unwrap();
        StoredJob::new(id, Input {
            request: request(2),
            provider: SavedProvider {
                home: home.clone(), config: String::new(), api_base: "https://example.invalid/v1".into(),
                key: MOCK_KEY.into(), local_mock: true,
            },
            references: Vec::new(), directory,
        })
    };
    if mode == "hold" {
        let mut job = make_job();
        job.claim().unwrap();
        job.items.get_mut(&1).unwrap().post_started = true;
        persist_manifest(&job).unwrap();
        register_manifest(&job).unwrap();
        fs::write(home.join("held.json.tmp"), json!({
            "id": job.id, "directory": job.input.directory,
        }).to_string()).unwrap();
        fs::rename(home.join("held.json.tmp"), home.join("held.json")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !home.join("release").is_file() {
            assert!(Instant::now() < deadline, "parent did not release lock holder");
            thread::sleep(Duration::from_millis(10));
        }
        drop(job);
    } else {
        fs::write(home.join(format!("ready-{}", std::process::id())), b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !home.join("go").is_file() {
            assert!(Instant::now() < deadline, "parent did not start index writers");
            thread::sleep(Duration::from_millis(10));
        }
        for _ in 0..8 {
            let job = make_job();
            persist_manifest(&job).unwrap();
            register_manifest(&job).unwrap();
        }
    }
}

#[test]
fn live_process_manifest_is_unchanged_and_crash_releases_job_ownership() {
    let home = TempHome::new();
    let mut child = LockTestChild::spawn(&home.0, "hold");
    wait_until(|| home.0.join("held.json").is_file());
    let held: Value = serde_json::from_slice(&fs::read(home.0.join("held.json")).unwrap()).unwrap();
    let id = held["id"].as_str().unwrap();
    let manifest = Path::new(held["directory"].as_str().unwrap()).join("manifest.json");
    let before = fs::read(&manifest).unwrap();
    let observer = ImageJobs::new(&home.0).unwrap();
    assert!(observer.status(id).is_err());
    observer.cancel_all();
    assert_eq!(fs::read(&manifest).unwrap(), before, "live owner manifest was modified");
    assert!(try_job_ownership(&home.0, id).unwrap().is_none());
    child.kill_and_wait();
    let recovered = ImageJobs::new(&home.0).unwrap();
    let status = recovered.status(id).unwrap();
    assert_eq!(status.status, "interrupted");
    assert_eq!(status.outcome_unknown, 1);
    assert_eq!(status.items[0].status, "uncertain");
    assert!(!recovered.has_active_requests());
    assert_eq!(lock(&recovered.shared).unwrap().workers, 0);
    assert!(try_job_ownership(&home.0, id).unwrap().is_none(),
        "recovered registry must now own the job lock");
}

#[test]
fn independent_process_index_writers_preserve_every_new_job() {
    let home = TempHome::new();
    let directory = home.0.join("oceanway-image-jobs");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join(".index-0.tmp"), b"stale old-format temporary").unwrap();
    let mut children: Vec<_> = (0..4).map(|_| LockTestChild::spawn(&home.0, "write")).collect();
    wait_until(|| children.iter().all(|child| home.0.join(format!("ready-{}", child.id())).is_file()));
    fs::write(home.0.join("go"), b"start").unwrap();
    for child in &mut children {
        child.finish();
    }
    let index: Value = serde_json::from_slice(&fs::read(directory.join("index.json")).unwrap()).unwrap();
    assert_eq!(index["jobs"].as_object().unwrap().len(), 32);
    assert_eq!(fs::read(directory.join(".index-0.tmp")).unwrap(), b"stale old-format temporary");
    let recovered = ImageJobs::new(&home.0).unwrap();
    assert_eq!(lock(&recovered.shared).unwrap().jobs.len(), 32);
    assert!(!recovered.has_active_requests());
    for id in index["jobs"].as_object().unwrap().keys() {
        assert_eq!(recovered.status(id).unwrap().status, "interrupted");
    }
}

fn cancellation_guard(cancelled: &Arc<AtomicBool>) -> ImageSubmissionGuard {
    let cancelled = Arc::clone(cancelled);
    Arc::new(move || cancelled.load(Ordering::SeqCst))
}

#[test]
fn start_cancellation_during_reference_preflight_never_publishes_or_posts() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("cancelled reference preflight must not POST"));
    let (_, input) = input(&home, &mock, 3);
    let provider = input.provider;
    let source = home.0.join("reference.png");
    fs::write(&source, png(3, 4)).unwrap();
    let mut request = request(3);
    request.reference_paths = vec![source.to_string_lossy().into_owned()];
    let state = ImageJobs::new(&home.0).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let ready = Arc::new(Gate::default());
    let resume = Arc::new(Gate::default());
    let service = state.clone();
    let guard = cancellation_guard(&cancelled);
    let thread_ready = Arc::clone(&ready);
    let thread_resume = Arc::clone(&resume);
    let start = thread::spawn(move || service.start_with_loaders(
        request, None, true, guard, |_| Ok(provider), |paths, home| {
            let references = load_references_in_home(paths, home)?;
            thread_ready.release();
            thread_resume.wait();
            Ok(references)
        },
    ));
    ready.wait();
    assert!(lock(&state.shared).unwrap().jobs.is_empty());
    cancelled.store(true, Ordering::SeqCst);
    resume.release();
    assert!(start.join().unwrap().err().unwrap().contains("cancelled before admission"));
    assert!(lock(&state.shared).unwrap().jobs.is_empty());
    assert!(!state.has_active_requests());
    assert_eq!(mock.count(), 0);
}

#[test]
fn cancellation_after_index_persistence_but_before_publication_cannot_send() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("cancelled unpublished job must not POST"));
    let (id, input) = input(&home, &mock, 2);
    let directory = input.directory.clone();
    let index_path = home.0.join("oceanway-image-jobs").join("index.json");
    let watched_id = id.clone();
    let guard: ImageSubmissionGuard = Arc::new(move || {
        fs::read(&index_path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|index| index["jobs"].get(watched_id.as_str()).is_some())
    });
    let state = ImageJobs::new(&home.0).unwrap();
    let error = submit_guarded(&state.shared, id.clone(), input, true, guard).err().unwrap();
    assert!(error.contains("cancelled before admission"));
    assert!(lock(&state.shared).unwrap().jobs.is_empty());
    let manifest = fs::read_to_string(directory.join("manifest.json")).unwrap();
    assert!(!manifest.contains("submissionGuard"));
    assert!(!manifest.contains("submission_guard"));
    let recovered = ImageJobs::new(&home.0).unwrap();
    assert_eq!(recovered.status(&id).unwrap().status, "interrupted");
    assert!(!recovered.has_active_requests());
    assert_eq!(lock(&recovered.shared).unwrap().workers, 0);
    assert_eq!(mock.count(), 0);
}

#[test]
fn authorize_checks_submission_guard_before_provider_and_after_manifest_preflight() {
    for after_persistence in [false, true] {
        let home = TempHome::new();
        let mock = Mock::new(|_, _| panic!("cancelled authorization must not POST"));
        let (id, input) = input(&home, &mock, 3);
        let manifest = input.directory.join("manifest.json");
        let provider = input.provider.clone();
        let state = ImageJobs::default();
        record_without_workers(&state, id.clone(), input, false);
        let cancelled = Arc::new(AtomicBool::new(false));
        let guard: ImageSubmissionGuard = if after_persistence {
            Arc::new(move || {
                fs::read(&manifest).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .is_some_and(|manifest| manifest["job"]["items"][0]["postStarted"] == true)
            })
        } else {
            cancellation_guard(&cancelled)
        };
        let input = {
            let mut registry = lock(&state.shared).unwrap();
            let job = find_job_mut(&mut registry, &id).unwrap();
            job.submission_guard = guard;
            job.claim().unwrap();
            Arc::clone(&job.input)
        };
        if !after_persistence {
            cancelled.store(true, Ordering::SeqCst);
        }
        let mut provider_reads = 0;
        let outcome = execute_with_guard(&input, 1, Instant::now(), || {
            authorize_post_with_loader(&state.shared, &id, 1, &input, || {
                provider_reads += 1;
                Ok(provider.clone())
            })
        });
        let failure = outcome.as_ref().err().unwrap();
        assert!(failure.not_sent && failure.stop_job);
        assert_eq!(provider_reads, if after_persistence { 1 } else { 0 });
        {
            let mut registry = lock(&state.shared).unwrap();
            finish_item(find_job_mut(&mut registry, &id).unwrap(), 1, outcome, 1);
        }
        let status = state.status(&id).unwrap();
        assert_eq!((status.completed, status.failed, status.cancelled), (0, 0, 3));
        assert_eq!(mock.count(), 0);
    }
}

#[test]
fn queued_guard_is_retained_and_normal_response_detachment_or_explicit_retry_releases_it() {
    let _serial = WORKER_TEST.lock().unwrap();
    for detach in [false, true] {
        let home = TempHome::new();
        let gate = Arc::new(Gate::default());
        let handler_gate = Arc::clone(&gate);
        let mock = Mock::new(move |_, _| { handler_gate.wait(); Reply::json(success()) });
        let (id, input) = input(&home, &mock, 5);
        let provider = input.provider.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(true));
        let request_cancelled = Arc::clone(&cancelled);
        let request_active = Arc::clone(&active);
        let guard: ImageSubmissionGuard = Arc::new(move ||
            request_cancelled.load(Ordering::SeqCst) && request_active.load(Ordering::SeqCst));
        let state = ImageJobs::default();
        submit_guarded(&state.shared, id.clone(), input, false, guard).unwrap();
        wait_until(|| mock.count() == 2);
        if detach {
            active.store(false, Ordering::SeqCst);
        }
        cancelled.store(true, Ordering::SeqCst);
        gate.release();
        let done = finished(&state, &id);
        if detach {
            assert_eq!(done.status, "completed");
            assert_eq!(mock.count(), 5);
        } else {
            assert_eq!((done.completed, done.cancelled), (2, 3));
            assert_eq!(mock.count(), 2);
            state.retry_with_loader(&id, |_| Ok(provider)).unwrap();
            assert_eq!(finished(&state, &id).completed, 5);
            assert_eq!(mock.count(), 5, "explicit retry replaces the old submission guard");
        }
    }
}

#[test]
fn retry_cancellation_during_reference_preflight_preserves_existing_output_and_queue() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let (id, mut input) = input(&home, &mock, 2);
    let source = home.0.join("reference.png");
    fs::write(&source, png(3, 4)).unwrap();
    input.request.reference_paths = vec![source.to_string_lossy().into_owned()];
    input.references = load_references_in_home(&input.request.reference_paths, &home.0).unwrap();
    let provider = input.provider.clone();
    let directory = input.directory.clone();
    let state = ImageJobs::default();
    record_without_workers(&state, id.clone(), input, false);
    let saved = {
        let mut registry = lock(&state.shared).unwrap();
        let job = find_job_mut(&mut registry, &id).unwrap();
        let index = job.claim().unwrap();
        let result = execute(&job.input, index, Instant::now()).unwrap();
        let path = result.path.clone();
        finish_item(job, index, Ok(result), 1);
        job.cancel();
        persist_or_stop(job).unwrap();
        path
    };
    let manifest = fs::read(directory.join("manifest.json")).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let ready = Arc::new(Gate::default());
    let resume = Arc::new(Gate::default());
    let service = state.clone();
    let retry_id = id.clone();
    let guard = cancellation_guard(&cancelled);
    let thread_ready = Arc::clone(&ready);
    let thread_resume = Arc::clone(&resume);
    let retry = thread::spawn(move || service.retry_with_loaders(
        &retry_id, guard, |_| Ok(provider), |paths, home| {
            let references = load_references_in_home(paths, home)?;
            thread_ready.release();
            thread_resume.wait();
            Ok(references)
        },
    ));
    ready.wait();
    cancelled.store(true, Ordering::SeqCst);
    resume.release();
    assert!(retry.join().unwrap().err().unwrap().contains("cancelled before admission"));
    assert_eq!(fs::read(directory.join("manifest.json")).unwrap(), manifest);
    assert_eq!(fs::read(saved).unwrap(), png(320, 160));
    assert_eq!(state.status(&id).unwrap().completed, 1);
    assert_eq!(mock.count(), 1);
}

#[test]
fn submission_guard_panics_fail_closed_without_poisoning_the_registry() {
    let guard: ImageSubmissionGuard = Arc::new(|| panic!("test-only guard failure"));
    let failure = check_submission_guard(&guard).err().unwrap();
    assert!(failure.not_sent && failure.stop_job);
    assert!(failure.message.contains("guard failed"));
    assert!(!failure.message.contains("test-only guard failure"));
}

#[test]
fn invalid_unrelated_history_is_preserved_and_does_not_block_new_jobs() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let mock = Mock::new(|_, _| Reply::json(success()));
    let mut history = Vec::new();
    for _ in 0..5 {
        let (id, input) = input(&home, &mock, 2);
        let directory = input.directory.clone();
        let job = StoredJob::new(id.clone(), input);
        persist_manifest(&job).unwrap();
        register_manifest(&job).unwrap();
        history.push((id, directory));
    }
    fs::write(history[0].1.join("manifest.json"), b"invalid mock-only historical JSON").unwrap();
    fs::remove_dir_all(&history[1].1).unwrap();
    let foreign_path = history[2].1.join("manifest.json");
    let mut foreign: Value = serde_json::from_slice(&fs::read(&foreign_path).unwrap()).unwrap();
    foreign["ownershipToken"] = json!("not-the-recorded-owner");
    fs::write(&foreign_path, serde_json::to_vec(&foreign).unwrap()).unwrap();
    let index_path = home.0.join("oceanway-image-jobs").join("index.json");
    let mut index: Value = serde_json::from_slice(&fs::read(&index_path).unwrap()).unwrap();
    let malformed_entry = json!({ "unrecognized": MOCK_KEY });
    index["jobs"][history[3].0.as_str()] = malformed_entry.clone();
    fs::write(&index_path, serde_json::to_vec(&index).unwrap()).unwrap();
    let index_before = fs::read(&index_path).unwrap();
    let unchanged: Vec<_> = [0, 2, 3].into_iter().map(|position| {
        let path = history[position].1.join("manifest.json");
        let bytes = fs::read(&path).unwrap();
        (path, bytes)
    }).collect();
    let service = ImageJobs::new(&home.0).unwrap();
    assert_eq!(fs::read(&index_path).unwrap(), index_before);
    assert_eq!(service.status(&history[4].0).unwrap().status, "interrupted");
    for (id, _) in &history[..4] {
        assert!(service.status(id).is_err());
    }
    assert!(!history[1].1.exists(), "recovery must not recreate a deleted output directory");
    let (_, next_input) = input(&home, &mock, 1);
    let next = service.start_with_loaders(
        request(1), None, false, no_submission_guard(),
        |_| Ok(next_input.provider), load_references_in_home,
    ).unwrap();
    assert_eq!(finished(&service, &next.id).completed, 1);
    assert_eq!(mock.count(), 1, "only the newly authorized request was sent");
    let index_after: Value = serde_json::from_slice(&fs::read(&index_path).unwrap()).unwrap();
    assert_eq!(index_after["jobs"].as_object().unwrap().len(), 6);
    assert_eq!(index_after["jobs"][history[3].0.as_str()], malformed_entry);
    for (path, bytes) in unchanged {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

fn save_local_fixture_provider(home: &Path, base: &str, key: &str) {
    fs::write(home.join("config.toml"), format!(
        "model_provider = 'OceanWay'\n[model_providers.OceanWay]\n\
         base_url = '{base}'\nenv_key = 'FIXTURE_MUST_NOT_REQUIRE_ENV'\n"
    )).unwrap();
    fs::write(home.join("auth.json"), json!({ "OPENAI_API_KEY": key }).to_string()).unwrap();
}

#[test]
fn fixture_transport_accepts_only_numeric_loopback_http_with_the_exact_reserved_key() {
    for base in [
        "http://127.0.0.1:41000/v1", "http://127.8.9.10:41000/custom",
        "http://[::1]:41000/v1", "http://[0:0:0:0:0:0:0:1]:41000/v1",
        // The URL parser canonicalizes these numeric forms to 127.0.0.1.
        "http://2130706433:41000/v1", "http://0x7f000001:41000/v1",
    ] {
        assert!(matches!(provider_transport(base, LOCAL_FIXTURE_KEY), Ok(ProviderTransport::LoopbackHttp)), "{base}");
        for key in ["", MOCK_KEY, "oceanway-local-fixture-extra", " oceanway-local-fixture "] {
            assert!(provider_transport(base, key).is_err(), "{base}");
        }
    }
    for base in [
        "http://example.invalid/v1", "http://localhost:41000/v1", "http://x.localhost/v1",
        "http://127.0.0.1.example.invalid/v1", "http://0.0.0.0/v1", "http://8.8.8.8/v1",
        "http://10.0.0.1/v1", "http://169.254.169.254/v1", "http://192.168.1.1/v1",
        "http://[::]/v1", "http://[::ffff:127.0.0.1]/v1", "http://[fc00::1]/v1",
        "http://user:secret@127.0.0.1/v1", "http://127.0.0.1/v1?key=secret",
        "http://127.0.0.1/v1#fragment", "file:///127.0.0.1/v1",
    ] {
        assert!(provider_transport(base, LOCAL_FIXTURE_KEY).is_err(), "{base}");
    }
    assert!(matches!(provider_transport("https://example.invalid/custom", MOCK_KEY),
        Ok(ProviderTransport::Https)));
}

#[test]
fn saved_http_fixture_rejects_arbitrary_auth_keys_before_start_or_client_creation() {
    let home = TempHome::new();
    let mock = Mock::new(|_, _| panic!("non-fixture credentials must never reach HTTP"));
    let service = ImageJobs::new(&home.0).unwrap();
    for key in ["", MOCK_KEY, "oceanway-local-fixture-extra"] {
        save_local_fixture_provider(&home.0, &mock.base, key);
        assert!(load_provider_in_home(&home.0).is_err());
        assert!(service.start(request(1), None, false).is_err());
        let (_, mut input) = input(&home, &mock, 1);
        input.provider.local_mock = false;
        input.provider.key = key.into();
        let failure = provider_client(&input.provider).err().unwrap();
        assert!(failure.not_sent && failure.stop_job);
    }
    assert!(lock(&service.shared).unwrap().jobs.is_empty());
    assert!(!service.has_active_requests());
    assert_eq!(mock.count(), 0);
}

#[test]
fn production_fixture_auth_metadata_partial_retry_and_ordered_edits_share_the_real_engine() {
    let _serial = WORKER_TEST.lock().unwrap();
    let home = TempHome::new();
    let fail_once = AtomicBool::new(true);
    let mock = Mock::new(move |_, request| {
        assert_eq!(request.headers["authorization"], format!("Bearer {LOCAL_FIXTURE_KEY}"));
        if request.method == "GET" {
            assert_eq!(request.path, "/v1/models");
            return Reply::json(json!({ "data": [{ "id": "gpt-image-2" }] }));
        }
        if request.path.ends_with("/generations") {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            if body["prompt"] == "fixture-fail-once" && fail_once.swap(false, Ordering::SeqCst) {
                return Reply { status: 503, headers: Vec::new(), body: b"mock-only failure".to_vec() };
            }
        }
        Reply::json(success())
    });
    save_local_fixture_provider(&home.0, &mock.base, LOCAL_FIXTURE_KEY);
    let provider = load_provider_in_home(&home.0).unwrap();
    assert!(!provider.local_mock, "exercise production validation, not the unit-test bypass");
    assert!(probe_capabilities(&provider, "gpt-image-2".into()).available);
    let service = ImageJobs::new(&home.0).unwrap();
    let mut generation = request(2);
    generation.prompts = vec!["fixture-success".into(), "fixture-fail-once".into()];
    let submitted = service.start(generation, None, false).unwrap();
    let partial = finished(&service, &submitted.id);
    assert_eq!((partial.status.as_str(), partial.completed, partial.failed), ("partial", 1, 1));
    assert_eq!(mock.count(), 3, "metadata plus two POSTs; failure must not automatically retry");
    let saved = partial.items.iter().find(|item| item.status == "succeeded").unwrap();
    let path = saved.path.as_ref().unwrap();
    let original = fs::read(path).unwrap();
    assert_eq!(saved.sha256.as_deref(), Some(hash(&original).as_str()));
    service.retry(&submitted.id).unwrap();
    assert_eq!(finished(&service, &submitted.id).completed, 2);
    assert_eq!(fs::read(path).unwrap(), original);
    assert_eq!(mock.count(), 4);

    let first = home.0.join("fixture-first.png");
    let second = home.0.join("fixture-second.png");
    fs::write(&first, png(1, 2)).unwrap();
    fs::write(&second, png(3, 4)).unwrap();
    let mut edit = request(1);
    edit.reference_paths = [&first, &second, &first].into_iter()
        .map(|path| path.to_string_lossy().into_owned()).collect();
    let submitted = service.start(edit, None, false).unwrap();
    let done = finished(&service, &submitted.id);
    assert_eq!(done.completed, 1);
    assert_eq!(done.reference_hashes, vec![hash(&png(1, 2)), hash(&png(3, 4)), hash(&png(1, 2))]);
    let captured = mock.requests.lock().unwrap();
    assert_eq!(captured.len(), 5);
    let edit = captured.last().unwrap();
    assert_eq!(edit.path, "/v1/images/edits");
    let body = String::from_utf8_lossy(&edit.body);
    let positions: Vec<_> = (1..=3).map(|index| body.find(&format!("reference-{index}.png")).unwrap()).collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    let manifest = fs::read_to_string(Path::new(&done.output_directory).join("manifest.json")).unwrap();
    assert!(!manifest.contains(LOCAL_FIXTURE_KEY));
}

#[test]
fn production_fixture_preflight_transport_or_key_changes_are_not_sent_and_stop_the_queue() {
    for change in ["key", "to_https", "to_http"] {
        for after_persistence in [false, true] {
            let home = TempHome::new();
            let mock = Mock::new(|_, _| panic!("changed fixture preflight must never POST"));
            let https_base = mock.base.replacen("http://", "https://", 1);
            let original_base = if change == "to_http" { &https_base } else { &mock.base };
            save_local_fixture_provider(&home.0, original_base, LOCAL_FIXTURE_KEY);
            let (id, mut input) = input(&home, &mock, 3);
            input.provider = load_provider_in_home(&home.0).unwrap();
            assert!(!input.provider.local_mock);
            let service = ImageJobs::default();
            record_without_workers(&service, id.clone(), input, false);
            let input = {
                let mut registry = lock(&service.shared).unwrap();
                let job = find_job_mut(&mut registry, &id).unwrap();
                job.claim().unwrap();
                Arc::clone(&job.input)
            };
            let mut reads = 0;
            let outcome = execute_with_guard(&input, 1, Instant::now(), || {
                authorize_post_with_loader(&service.shared, &id, 1, &input, || {
                    reads += 1;
                    if reads == if after_persistence { 2 } else { 1 } {
                        if after_persistence {
                            let manifest: Value = serde_json::from_slice(
                                &fs::read(input.directory.join("manifest.json")).unwrap()
                            ).unwrap();
                            assert_eq!(manifest["job"]["items"][0]["postStarted"], true);
                        }
                        let base = if change == "to_https" { &https_base } else { &mock.base };
                        let key = if change == "key" { "different-nonsecret-fixture-key" } else { LOCAL_FIXTURE_KEY };
                        save_local_fixture_provider(&home.0, base, key);
                    }
                    load_post_provider(&input.provider)
                })
            });
            let failure = outcome.as_ref().err().unwrap();
            assert!(failure.not_sent && failure.stop_job, "{change}");
            assert!(!failure.message.contains("different-nonsecret-fixture-key"));
            assert_eq!(reads, if after_persistence { 2 } else { 1 });
            {
                let mut registry = lock(&service.shared).unwrap();
                let job = find_job_mut(&mut registry, &id).unwrap();
                finish_item(job, 1, outcome, 1);
                persist_or_stop(job).unwrap();
            }
            let done = service.status(&id).unwrap();
            assert_eq!((done.completed, done.failed, done.cancelled), (0, 0, 3));
            assert!(!service.has_active_requests());
            assert_eq!(mock.count(), 0);
        }
    }
}

#[test]
fn production_loopback_fixture_keeps_redirects_disabled_and_output_downloads_public() {
    let _serial = WORKER_TEST.lock().unwrap();
    for scenario in ["redirect", "http", "https"] {
        let home = TempHome::new();
        let mock = Mock::new(move |_, request| {
            if scenario == "redirect" {
                return Reply {
                    status: 307,
                    headers: vec![("Location".into(), format!("http://{}/redirect", request.headers["host"]))],
                    body: Vec::new(),
                };
            }
            Reply::json(json!({
                "data": [{ "url": format!("{scenario}://{}/image.png", request.headers["host"]) }]
            }))
        });
        save_local_fixture_provider(&home.0, &mock.base, LOCAL_FIXTURE_KEY);
        let service = ImageJobs::new(&home.0).unwrap();
        let submitted = service.start(request(1), None, false).unwrap();
        assert_eq!(finished(&service, &submitted.id).failed, 1);
        assert_eq!(mock.count(), 1, "no redirect or loopback download may follow the provider POST");
    }
}
