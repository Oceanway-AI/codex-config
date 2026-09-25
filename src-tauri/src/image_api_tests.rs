//! Mock-only tests: no real provider, external download, or saved user credential.
use super::*;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize};

const MOCK_KEY: &str = "mock-only-credential";

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
    // Production command additionally reloads and compares the saved config/key.
    find_job_mut(&mut lock(&state.shared).unwrap(), id).unwrap().retry().unwrap();
    state.shared.wake.notify_all();
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
    assert_eq!(manifest["schemaVersion"], 1);
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
    for field in ["model", "prompt", "referencePaths", "size"] {
        let (id, mut input) = input(&home, &mock, 1);
        match field {
            "model" => input.request.model = MOCK_KEY.into(),
            "prompt" => input.request.prompt = format!("accidental {MOCK_KEY} paste"),
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
fn retries_reject_changed_configuration_endpoint_home_or_credentials() {
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
        .map(|item| (item.index, item.path.clone(), item.preview_data_url.clone())).collect();
    retry_mock(&state, &id);
    let second = finished(&state, &id);
    assert_eq!(second.status, "completed");
    assert_eq!((second.completed, second.failed), (4, 0));
    assert_eq!(mock.count(), 5);
    for (index, path, preview) in previous {
        let item = second.items.iter().find(|item| item.index == index).unwrap();
        assert_eq!(item.path, path);
        assert_eq!(item.preview_data_url, preview);
    }
    assert!(find_job_mut(&mut lock(&state.shared).unwrap(), &id).unwrap().retry().is_err());
    let serialized = serde_json::to_string(&second).unwrap();
    assert!(!serialized.contains(MOCK_KEY));
    assert!(!serialized.contains("apiBase"));
}

#[test]
fn all_failed_job_retries_explicitly_and_counts_are_consistent() {
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
