//! Local image-job MCP transport. Discovery never loads a provider or submits images.

use crate::image_api::{ImageJob, ImageJobs, ImageTestRequest};
use crate::mcp_config::{self, McpStatus};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{ImageFormat, ImageReader, Limits};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::schemars;
use rmcp::service::{QuitReason, RequestContext};
use rmcp::transport::{stdio, TokioChildProcess};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, ReadBuf};
use tokio::time::{sleep, timeout, Instant};

const TOOL_NAMES: [&str; 4] = [
    "generate_images",
    "get_image_job",
    "cancel_image_job",
    "retry_image_job",
];
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
// The image engine allows 180 seconds per in-flight request; leave time to save.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(195);
const PREVIEW_SOURCE_LIMIT: usize = 512 * 1024;
const PREVIEW_BYTES_LIMIT: usize = 256 * 1024;
const RESPONSE_PREVIEW_LIMIT: usize = 512 * 1024;
const MAX_PREVIEWS: usize = 4;
const BILLING_NOTICE: &str = "Each attempt may be charged. Cancellation stops queued work; \
    in-flight requests finish and are saved. Retry is explicit and may incur another charge.";

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct GenerateArgs {
    /// Absolute path to the workspace in which image results will be saved.
    workspace: String,
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    prompts: Vec<String>,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default = "default_count")]
    count: usize,
    #[serde(default = "default_size")]
    size: String,
    /// Local image paths. Relative paths are resolved against workspace.
    #[serde(default)]
    references: Vec<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct GetArgs {
    job_id: String,
    /// Wait for progress or completion, at most 30 seconds. Zero returns immediately.
    #[serde(default)]
    #[schemars(range(min = 0, max = 30))]
    wait_seconds: u64,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct JobArgs {
    job_id: String,
}

fn default_model() -> String {
    "gpt-image-2".into()
}

fn default_count() -> usize {
    1
}

fn default_size() -> String {
    "1024x1024".into()
}

fn tools() -> Vec<Tool> {
    let annotations = |read_only, idempotent, open_world| {
        ToolAnnotations::from_raw(
            None,
            Some(read_only),
            Some(false),
            Some(idempotent),
            Some(open_world),
        )
    };
    vec![
        Tool::new(
            TOOL_NAMES[0],
            "Start explicitly requested paid image generation or editing in an absolute workspace. \
             Uses saved provider settings. Returns a job; no automatic retries.",
            JsonObject::new(),
        )
        .with_input_schema::<GenerateArgs>()
        .with_annotations(annotations(false, false, true)),
        Tool::new(
            TOOL_NAMES[1],
            "Read an image job; optionally wait up to 30 seconds for progress. \
             Cancelling a pending poll also cancels the job's queued work.",
            JsonObject::new(),
        )
        .with_input_schema::<GetArgs>()
        .with_annotations(annotations(true, true, false)),
        Tool::new(
            TOOL_NAMES[2],
            "Cancel queued image work. In-flight requests finish and are saved.",
            JsonObject::new(),
        )
        .with_input_schema::<JobArgs>()
        .with_annotations(annotations(false, true, false)),
        Tool::new(
            TOOL_NAMES[3],
            "Explicitly retry unfinished image indices. Successful images are retained; \
             new attempts may incur charges.",
            JsonObject::new(),
        )
        .with_input_schema::<JobArgs>()
        .with_annotations(annotations(false, false, true)),
    ]
}

#[derive(Default)]
struct Session {
    closing: AtomicBool,
    operations: AtomicUsize,
    previews_sent: Mutex<HashSet<(String, usize)>>,
}

struct Operation(Arc<Session>);

impl Operation {
    fn new(session: Arc<Session>) -> Self {
        session.operations.fetch_add(1, Ordering::SeqCst);
        Self(session)
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.0.operations.fetch_sub(1, Ordering::SeqCst);
    }
}

struct DisconnectReader<R> {
    inner: R,
    jobs: ImageJobs,
    session: Arc<Session>,
    disconnected: bool,
}

impl<R: AsyncRead + Unpin> AsyncRead for DisconnectReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let remaining = buffer.remaining();
        let filled = buffer.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(context, buffer);
        let disconnected = matches!(&result, Poll::Ready(Err(_)))
            || (remaining > 0
                && matches!(&result, Poll::Ready(Ok(())))
                && buffer.filled().len() == filled);
        if disconnected && !this.disconnected {
            this.disconnected = true;
            this.session.closing.store(true, Ordering::SeqCst);
            let jobs = this.jobs.clone();
            let guard = Operation::new(this.session.clone());
            // Stop queued charges at EOF, before rmcp's response-drain grace.
            let _ = tokio::task::spawn_blocking(move || {
                jobs.cancel_all();
                drop(guard);
            });
        }
        result
    }
}

#[derive(Clone)]
struct ImageMcp {
    jobs: ImageJobs,
    session: Arc<Session>,
}

impl ImageMcp {
    fn new(jobs: ImageJobs) -> Self {
        Self {
            jobs,
            session: Arc::default(),
        }
    }

    async fn mutate<F>(&self, operation: F) -> Result<ImageJob, &'static str>
    where
        F: FnOnce(ImageJobs) -> Result<ImageJob, &'static str> + Send + 'static,
    {
        let jobs = self.jobs.clone();
        let guard = Operation::new(self.session.clone());
        blocking(move || {
            if guard.0.closing.load(Ordering::SeqCst) {
                return Err("The image server is shutting down.");
            }
            let result = operation(jobs);
            drop(guard);
            result
        })
        .await
    }

    async fn cancel_job(&self, id: String) -> Result<ImageJob, &'static str> {
        let jobs = self.jobs.clone();
        blocking(move || {
            jobs.cancel(&id)
                .map_err(|_| "Could not cancel this image job.")
        })
        .await
    }

    async fn generate(
        &self,
        args: GenerateArgs,
        context: RequestContext<RoleServer>,
    ) -> Result<ImageJob, &'static str> {
        if context.ct.is_cancelled() {
            return Err("The image request was cancelled before submission.");
        }
        let job = self
            .mutate(move |jobs| {
                let workspace = absolute_workspace(&args.workspace)?;
                let references = args
                    .references
                    .into_iter()
                    .map(|reference| {
                        let path = PathBuf::from(reference);
                        if path.is_absolute() {
                            path
                        } else {
                            workspace.join(path)
                        }
                        .to_string_lossy()
                        .into_owned()
                    })
                    .collect();
                let request = ImageTestRequest {
                    model: args.model,
                    prompt: args.prompt,
                    prompts: args.prompts,
                    count: args.count,
                    size: args.size,
                    reference_paths: references,
                };
                jobs.start(request, Some(&workspace), true)
                    .map_err(|_| "Could not start the image job. Check the saved configuration and inputs.")
            })
            .await?;
        // Submission is blocking and cannot be aborted safely. Cancel its queue
        // if the client cancelled while submission was being prepared.
        if context.ct.is_cancelled() {
            self.cancel_job(job.id).await
        } else {
            Ok(job)
        }
    }

    async fn retry(
        &self,
        args: JobArgs,
        context: RequestContext<RoleServer>,
    ) -> Result<ImageJob, &'static str> {
        if context.ct.is_cancelled() {
            return Err("The retry was cancelled before submission.");
        }
        let job = self
            .mutate(move |jobs| {
                jobs.retry(&args.job_id)
                    .map_err(|_| "Could not retry this image job. Wait for active requests to finish.")
            })
            .await?;
        if context.ct.is_cancelled() {
            self.cancel_job(job.id).await
        } else {
            Ok(job)
        }
    }

    async fn poll(&self, args: &GetArgs) -> Result<ImageJob, &'static str> {
        let started = Instant::now();
        let status = || {
            let jobs = self.jobs.clone();
            let id = args.job_id.clone();
            blocking(move || jobs.status(&id).map_err(|_| "Could not find this image job."))
        };
        let mut job = timeout(Duration::from_secs(30), status())
            .await
            .map_err(|_| "The image job status lookup timed out.")??;
        if args.wait_seconds == 0 || terminal(&job) {
            return Ok(job);
        }
        let baseline = progress(&job);
        let deadline = started + Duration::from_secs(args.wait_seconds);
        loop {
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => return Ok(job),
                _ = sleep(POLL_INTERVAL) => {}
            }
            // Include a potentially contended registry read in the poll deadline.
            job = match tokio::time::timeout_at(deadline, status()).await {
                Ok(result) => result?,
                Err(_) => return Ok(job),
            };
            if terminal(&job) || progress(&job) != baseline {
                return Ok(job);
            }
        }
    }

    async fn get(
        &self,
        args: GetArgs,
        context: RequestContext<RoleServer>,
    ) -> Result<ImageJob, &'static str> {
        if args.wait_seconds > 30 {
            return Err("wait_seconds must be between 0 and 30.");
        }
        // Keep this select inside the request. rmcp also cancels context.ct after
        // a normal response, so no watcher may outlive this method.
        tokio::select! {
            biased;
            _ = context.ct.cancelled() => self.cancel_job(args.job_id.clone()).await,
            result = self.poll(&args) => result,
        }
    }

    async fn dispatch(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let arguments = request.arguments.unwrap_or_default();
        let result = match request.name.as_ref() {
            "generate_images" => match parse(arguments) {
                Ok(args) => self.generate(args, context).await,
                Err(message) => Err(message),
            },
            "get_image_job" => match parse(arguments) {
                Ok(args) => self.get(args, context).await,
                Err(message) => Err(message),
            },
            "cancel_image_job" => match parse::<JobArgs>(arguments) {
                Ok(args) => self.cancel_job(args.job_id).await,
                Err(message) => Err(message),
            },
            "retry_image_job" => match parse(arguments) {
                Ok(args) => self.retry(args, context).await,
                Err(message) => Err(message),
            },
            _ => Err("Unknown image tool."),
        };
        match result {
            Ok(job) => {
                let session = self.session.clone();
                match blocking(move || render_job(job, &session)).await {
                    Ok(result) => result,
                    Err(message) => tool_error(message),
                }
            }
            Err(message) => tool_error(message),
        }
    }
}

impl ServerHandler for ImageMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools().into_iter().find(|tool| tool.name == name)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.and_then(|params| params.cursor).is_some() {
            return Err(ErrorData::invalid_params("Invalid tools cursor.", None));
        }
        let mut result = ListToolsResult::default();
        result.tools = tools();
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        Ok(self.dispatch(request, context).await.into())
    }
}

fn parse<T: DeserializeOwned>(arguments: JsonObject) -> Result<T, &'static str> {
    // Serde errors can quote caller-supplied values. Never return them verbatim.
    serde_json::from_value(serde_json::Value::Object(arguments))
        .map_err(|_| "Invalid image tool arguments. Use the advertised input schema.")
}

fn tool_error(message: &'static str) -> CallToolResult {
    CallToolResult::structured_error(json!({ "error": message }))
}

async fn blocking<T, F>(operation: F) -> Result<T, &'static str>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, &'static str> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| "The local image operation could not finish.")?
}

fn absolute_workspace(value: &str) -> Result<PathBuf, &'static str> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err("workspace must be an absolute directory path.");
    }
    let path = std::fs::canonicalize(path).map_err(|_| "workspace is not accessible.")?;
    if !path.is_dir() {
        return Err("workspace must be a directory.");
    }
    Ok(path)
}

fn terminal(job: &ImageJob) -> bool {
    matches!(job.status.as_str(), "completed" | "partial" | "failed" | "cancelled")
}

fn progress(job: &ImageJob) -> (String, usize, usize, usize) {
    (job.status.clone(), job.completed, job.failed, job.cancelled)
}

fn thumbnail(data_url: &str) -> Option<String> {
    if data_url.len() > PREVIEW_SOURCE_LIMIT {
        return None;
    }
    let encoded = data_url.strip_prefix("data:image/png;base64,")?;
    let bytes = STANDARD.decode(encoded).ok()?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?.thumbnail(256, 256);
    let mut output = Cursor::new(Vec::new());
    decoded.write_to(&mut output, ImageFormat::Png).ok()?;
    if output.get_ref().len() > PREVIEW_BYTES_LIMIT {
        return None;
    }
    Some(STANDARD.encode(output.into_inner()))
}

fn render_job(mut job: ImageJob, session: &Session) -> Result<CallToolResult, &'static str> {
    let mut images = Vec::new();
    let mut bytes = 0;
    let mut sent = session
        .previews_sent
        .lock()
        .map_err(|_| "Could not prepare image previews.")?;
    for item in &mut job.items {
        let preview = item.preview_data_url.take();
        let identity = (job.id.clone(), item.index);
        if item.status == "succeeded" && !sent.contains(&identity) && images.len() < MAX_PREVIEWS {
            if let Some(data) = preview.as_deref().and_then(thumbnail) {
                if bytes + data.len() <= RESPONSE_PREVIEW_LIMIT {
                    bytes += data.len();
                    images.push(ContentBlock::image(data, "image/png"));
                    sent.insert(identity);
                }
            }
        }
        if item.error.is_some() {
            item.error = Some("This image attempt failed; inspect the saved local job details.".into());
        }
        if item.warning.is_some() {
            item.warning = Some("This image has a response warning; inspect the saved local job details.".into());
        }
    }
    drop(sent);
    // Do not mirror provider-originated error text or duplicate base64 in JSON.
    job.message = BILLING_NOTICE.into();
    let value = serde_json::to_value(job).map_err(|_| "Could not serialize the image job.")?;
    let mut result = CallToolResult::structured(value);
    result.content.extend(images);
    Ok(result)
}

async fn shutdown(server: &ImageMcp) -> Result<(), String> {
    server.session.closing.store(true, Ordering::SeqCst);
    let drain = async {
        let mut previous_submissions = true;
        loop {
            let submissions = server.session.operations.load(Ordering::SeqCst) != 0;
            let cancel_queued = submissions || previous_submissions;
            let jobs = server.jobs.clone();
            let active = blocking(move || {
                if cancel_queued {
                    jobs.cancel_all();
                }
                Ok(jobs.has_active_requests())
            })
            .await
            .map_err(str::to_owned)?;
            if !active && server.session.operations.load(Ordering::SeqCst) == 0 {
                return Ok::<(), String>(());
            }
            previous_submissions = submissions;
            sleep(POLL_INTERVAL).await;
        }
    };
    timeout(SHUTDOWN_TIMEOUT, drain)
        .await
        .map_err(|_| "Image shutdown timed out while waiting for in-flight requests to save.".to_owned())?
}

pub async fn serve_stdio() -> Result<(), String> {
    let server = ImageMcp::new(ImageJobs::default());
    let (input, output) = stdio();
    let input = DisconnectReader {
        inner: input,
        jobs: server.jobs.clone(),
        session: server.session.clone(),
        disconnected: false,
    };
    let result = match server.clone().serve((input, output)).await {
        Ok(service) => match service.waiting().await {
            Ok(QuitReason::Closed | QuitReason::Cancelled) => Ok(()),
            _ => Err("The image MCP service stopped unexpectedly.".to_owned()),
        },
        Err(_) => Err("Could not initialize the image MCP service.".to_owned()),
    };
    // Run this even after failed initialization or transport failure.
    let drained = shutdown(&server).await;
    result.and(drained)
}

fn all_tools_available(tools: &[Tool]) -> bool {
    TOOL_NAMES
        .iter()
        .all(|name| tools.iter().any(|tool| tool.name.as_ref() == *name))
}

pub async fn check(home: &Path) -> Result<McpStatus, String> {
    let home = home.to_path_buf();
    let (executable, home, mut status) = blocking(move || {
        let executable = mcp_config::command(&home)
            .map_err(|_| "The installed image MCP runtime could not be verified.")?;
        let config = std::fs::read_to_string(home.join("config.toml"))
            .map_err(|_| "Could not read the image MCP configuration.")?;
        let status = mcp_config::status(&home, &config);
        Ok((executable, home, status))
    })
    .await
    .map_err(str::to_owned)?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--image-mcp-stdio")
        .env("CODEX_HOME", home)
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let transport = TokioChildProcess::new(command)
        .map_err(|_| "Could not start the image MCP runtime.".to_owned())?;
    let mut client = timeout(HANDSHAKE_TIMEOUT, ().serve(transport))
        .await
        .map_err(|_| "The image MCP initialization timed out.".to_owned())?
        .map_err(|_| "The image MCP initialization failed.".to_owned())?;
    let listed = timeout(HANDSHAKE_TIMEOUT, client.list_all_tools()).await;
    let closed = client.close_with_timeout(Duration::from_secs(10)).await;
    let tools = listed
        .map_err(|_| "The image MCP tool discovery timed out.".to_owned())?
        .map_err(|_| "The image MCP tool discovery failed.".to_owned())?;
    if !matches!(closed, Ok(Some(_))) {
        return Err("The image MCP check could not close its runtime cleanly.".into());
    }
    status.tools_available = all_tools_available(&tools);
    status.message = if status.tools_available {
        "Image MCP initialized and all four tools are available. No image request was sent."
    } else {
        "Image MCP initialized, but one or more required image tools are missing."
    }
    .into();
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_api::ImageItem;

    fn sample_job() -> ImageJob {
        ImageJob {
            id: "test-job".into(),
            status: "completed".into(),
            model: default_model(),
            mode: "generate".into(),
            total: 1,
            completed: 1,
            failed: 0,
            cancelled: 0,
            items: vec![ImageItem {
                index: 1,
                status: "succeeded".into(),
                path: Some("image.png".into()),
                preview_data_url: None,
                error: None,
                request_id: None,
                elapsed_ms: None,
                warning: None,
                additional_paths: vec![],
            }],
            message: String::new(),
        }
    }

    #[test]
    fn metadata_has_exact_tools_and_no_credentials() {
        let tools = tools();
        assert_eq!(tools.len(), 4);
        assert!(all_tools_available(&tools));
        for count in 0..4 {
            assert!(!all_tools_available(&tools[..count]));
        }
        let metadata = serde_json::to_string(&tools).unwrap().to_lowercase();
        for forbidden in ["api_key", "apikey", "authorization", "password", "bearer", "token"] {
            assert!(!metadata.contains(forbidden), "{forbidden}");
        }
        let schema = serde_json::to_value(&tools[0].input_schema).unwrap();
        assert_eq!(schema["required"], json!(["workspace"]));
        assert_eq!(schema["properties"]["model"]["default"], "gpt-image-2");
        assert_eq!(schema["properties"]["count"]["default"], 1);
        assert_eq!(schema["properties"]["size"]["default"], "1024x1024");
        assert!(schema["properties"].get("reference_paths").is_none());
        assert!(schema["properties"].get("references").is_some());
        assert!(schema["properties"].get("prompts").is_some());
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn defaults_and_parse_failures_do_not_echo_secrets() {
        let args: GenerateArgs =
            serde_json::from_value(json!({"workspace": "/workspace"})).unwrap();
        assert_eq!(args.count, 1);
        assert!(args.references.is_empty());
        assert!(args.prompt.is_empty());
        assert!(args.prompts.is_empty());
        let arguments = json!({"job_id": "x", "wait_seconds": "sk-do-not-echo"})
            .as_object().unwrap().clone();
        let error = parse::<GetArgs>(arguments).err().unwrap();
        assert!(!error.contains("sk-do-not-echo"));
        assert!(absolute_workspace("relative/workspace").is_err());
    }

    #[test]
    fn previews_are_bounded_deduplicated_and_absent_from_structured_json() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(512, 512)
            .write_to(&mut png, ImageFormat::Png).unwrap();
        let mut job = sample_job();
        job.items[0].preview_data_url =
            Some(format!("data:image/png;base64,{}", STANDARD.encode(png.into_inner())));
        job.items[0].error = Some("sk-provider-secret".into());
        job.items[0].warning = Some("sk-provider-secret".into());
        job.message = "sk-provider-secret".into();
        let session = Session::default();
        let first = render_job(job.clone(), &session).unwrap();
        assert_eq!(first.content.len(), 2);
        let ContentBlock::Image(preview) = &first.content[1] else { panic!("missing preview") };
        let bytes = STANDARD.decode(&preview.data).unwrap();
        assert!(bytes.len() <= PREVIEW_BYTES_LIMIT);
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(decoded.width() <= 256 && decoded.height() <= 256);
        let json = serde_json::to_string(&first).unwrap();
        assert!(!json.contains("sk-provider-secret"));
        assert!(!json.contains("previewDataUrl"));
        assert_eq!(render_job(job, &session).unwrap().content.len(), 1);
        assert!(thumbnail(&"x".repeat(PREVIEW_SOURCE_LIMIT + 1)).is_none());
        assert!(thumbnail("data:image/png;base64,invalid").is_none());
    }

    #[test]
    fn each_result_limits_preview_count_and_keeps_remaining_previews() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut png, ImageFormat::Png).unwrap();
        let data_url = format!("data:image/png;base64,{}", STANDARD.encode(png.into_inner()));
        let mut job = sample_job();
        let mut item = job.items[0].clone();
        item.preview_data_url = Some(data_url);
        job.items = (1..=MAX_PREVIEWS * 2)
            .map(|index| {
                let mut item = item.clone();
                item.index = index;
                item
            })
            .collect();
        job.total = job.items.len();
        job.completed = job.total;
        let session = Session::default();
        for _ in 0..2 {
            let result = render_job(job.clone(), &session).unwrap();
            assert_eq!(result.content.len(), MAX_PREVIEWS + 1);
        }
        assert_eq!(render_job(job, &session).unwrap().content.len(), 1);
    }

    #[tokio::test]
    async fn initialization_and_listing_do_not_create_jobs() {
        let jobs = ImageJobs::default();
        let server = ImageMcp::new(jobs.clone());
        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(async move {
            let service = server.serve(server_io).await.unwrap();
            service.waiting().await.unwrap();
        });
        let mut client = timeout(Duration::from_secs(5), ().serve(client_io))
            .await.unwrap().unwrap();
        let listed = client.list_all_tools().await.unwrap();
        assert!(all_tools_available(&listed));
        assert!(!jobs.has_active_requests());
        client.close().await.unwrap();
        timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn stdin_eof_closes_submission_gate_before_service_drain() {
        use tokio::io::AsyncReadExt;
        let jobs = ImageJobs::default();
        let session = Arc::new(Session::default());
        let (input, writer) = tokio::io::duplex(64);
        let mut input = DisconnectReader {
            inner: input,
            jobs: jobs.clone(),
            session: session.clone(),
            disconnected: false,
        };
        let mut byte = [0_u8; 1];
        drop(writer);
        assert_eq!(input.read(&mut byte).await.unwrap(), 0);
        assert!(session.closing.load(Ordering::SeqCst));
        let server = ImageMcp { jobs, session };
        let result = server.mutate(|_| panic!("must not submit after EOF")).await;
        assert!(result.is_err());
        shutdown(&server).await.unwrap();
    }
}
