use oceanway_image_service_tests::oceanway_images::mcp_stdio;
use std::path::Path;

#[tokio::test]
async fn image_model_discovery_requires_configuration_instead_of_returning_a_fake_catalog() {
    use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
    use std::time::Duration;
    use tokio::time::timeout;

    let home = std::env::temp_dir().join(format!("oceanway-model-discovery-{}", std::process::id()));
    std::fs::create_dir(&home).unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_oceanway-image-mcp-fixture"));
    command.arg("--image-mcp-stdio").env("OCEANWAY_CONFIG_HOME", &home)
        .env_remove("OCEANWAY_MCP_CHECK_ONLY").kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut client = timeout(Duration::from_secs(10), ().serve(TokioChildProcess::new(command).unwrap()))
        .await.unwrap().unwrap();
    let listed = client.list_tools(None).await.unwrap();
    assert_eq!(listed.ttl_ms, Some(0));
    assert_eq!(listed.cache_scope, Some(rmcp::model::CacheScope::Private));
    let mut names: Vec<_> = listed.tools.iter().map(|tool| tool.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(names, ["cancel_image_job", "generate_images", "get_image_job", "get_image_models", "retry_image_job"]);
    for removed in ["get_video_models", "generate_video", "get_video_job"] {
        let rejected = timeout(Duration::from_secs(5), client.call_tool(CallToolRequestParams::new(removed)))
            .await.unwrap().unwrap();
        assert_eq!(rejected.is_error, Some(true));
        assert_eq!(rejected.structured_content.unwrap()["error"], "Unknown image tool.");
    }
    let result = timeout(Duration::from_secs(5), client.call_tool(CallToolRequestParams::new("get_image_models")))
        .await.unwrap().unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(result.structured_content.as_ref().and_then(|value| value.get("models")).is_none());
    assert!(client.close_with_timeout(Duration::from_secs(5)).await.unwrap().is_some());
    std::fs::remove_dir_all(home).unwrap();
}

#[tokio::test]
async fn real_child_process_initializes_without_credentials_or_image_requests() {
    let directory =
        std::env::temp_dir().join(format!("oceanway-stdio-fixture-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let result = mcp_stdio::check(
        Path::new(env!("CARGO_BIN_EXE_oceanway-image-mcp-fixture")),
        &directory,
    )
    .await;
    let untouched = std::fs::read_dir(&directory).unwrap().next().is_none();
    std::fs::remove_dir(&directory).unwrap();
    assert!(result.unwrap().tools_available);
    assert!(
        untouched,
        "discovery must not create files or need an API key"
    );
}

#[tokio::test]
async fn relocated_binary_and_app_home_with_unicode_spaces_need_no_developer_checkout() {
    let root = std::env::temp_dir().join(format!("Oceanway portable 中文 {}", std::process::id()));
    let home = root.join("application data 空格");
    let executable = root.join(if cfg!(windows) {
        "Oceanway native 图片.exe"
    } else {
        "Oceanway native 图片"
    });
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_oceanway-image-mcp-fixture"),
        &executable,
    )
    .unwrap();
    let result = mcp_stdio::check(&executable, &home).await;
    let untouched = std::fs::read_dir(&home).unwrap().next().is_none();
    std::fs::remove_file(executable).unwrap();
    std::fs::remove_dir(home).unwrap();
    std::fs::remove_dir(root).unwrap();
    assert!(result.unwrap().tools_available);
    assert!(untouched);
}

#[tokio::test]
async fn missing_runtime_is_reported_before_any_configuration_change() {
    let root =
        std::env::temp_dir().join(format!("oceanway-missing-runtime-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let result = mcp_stdio::check(&root.join("missing-executable"), &root).await;
    let untouched = std::fs::read_dir(&root).unwrap().next().is_none();
    std::fs::remove_dir(root).unwrap();
    assert!(matches!(result, Err(ref message) if message.contains("Could not start")));
    assert!(untouched);
}

#[tokio::test]
async fn discovery_check_preserves_existing_job_bytes_and_rejects_all_tool_calls() {
    use oceanway_image_service_tests::oceanway_images::ImageJobs;
    use rmcp::model::CallToolRequestParams;
    use rmcp::transport::TokioChildProcess;
    use rmcp::ServiceExt;
    use serde_json::json;
    use std::time::Duration;
    use tokio::time::timeout;

    let root = std::env::temp_dir().join(format!(
        "oceanway-existing-job-check-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let home = std::fs::canonicalize(&root).unwrap();
    let store = home.join("oceanway-image-jobs");
    let id = "image-123-456-789";
    let directory = home.join("oceanway-image-tests").join(id);
    std::fs::create_dir(&store).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    let token = "a".repeat(64);
    let engine = "oceanway-desktop-image-engine/v1";
    let index = json!({
        "schemaVersion": 1, "engine": engine, "appHome": home,
        "jobs": {id: {"directory": directory, "ownershipToken": token}},
    });
    let manifest = json!({
        "schemaVersion": 2, "updatedAtMs": 1234, "requiresExplicitResume": true,
        "recoveryMessage": "fixture awaiting explicit recovery", "engine": engine,
        "appHome": home, "ownershipToken": token, "prompt": "fixture image",
        "prompts": [], "model": "fixture-image-v1", "referencePaths": [],
        "size": "1024x1024", "count": 2, "outputDirectory": directory, "leased": true,
        "job": {
            "id": id, "status": "running", "model": "fixture-image-v1", "mode": "generate",
            "total": 2, "completed": 0, "failed": 0, "cancelled": 0,
            "items": [{"index": 1, "status": "running", "postStarted": true}, {"index": 2, "status": "queued"}],
            "message": "original progress", "outputDirectory": directory,
            "referencePaths": [], "referenceHashes": [], "warnings": [], "countMismatch": false,
            "persistenceError": null, "paused": false, "leased": true, "recovered": false,
            "outcomeUnknown": 0, "leaseRemainingMs": 120000,
        },
    });
    let index_path = store.join("index.json");
    let manifest_path = directory.join("manifest.json");
    let original_index = serde_json::to_vec_pretty(&index).unwrap();
    let original_manifest = serde_json::to_vec_pretty(&manifest).unwrap();
    std::fs::write(&index_path, &original_index).unwrap();
    std::fs::write(&manifest_path, &original_manifest).unwrap();
    let executable = Path::new(env!("CARGO_BIN_EXE_oceanway-image-mcp-fixture"));
    assert!(
        mcp_stdio::check(executable, &home)
            .await
            .unwrap()
            .tools_available
    );

    // The check-only process exposes identical discovery metadata but cannot
    // submit, inspect, retry, or cancel jobs even if a caller sends tools/call.
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--image-mcp-stdio")
        .env("OCEANWAY_CONFIG_HOME", &home)
        .env("OCEANWAY_MCP_CHECK_ONLY", "1")
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let transport = TokioChildProcess::new(command).unwrap();
    let mut client = timeout(Duration::from_secs(5), ().serve(transport))
        .await
        .unwrap()
        .unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 5);
    for tool in tools {
        let result = timeout(
            Duration::from_secs(5),
            client.call_tool(
                CallToolRequestParams::new(tool.name)
                    .with_arguments(json!({"job_id": id}).as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(serde_json::to_string(&result)
            .unwrap()
            .contains("discovery check"));
    }
    assert!(client
        .close_with_timeout(Duration::from_secs(5))
        .await
        .unwrap()
        .is_some());
    assert_eq!(std::fs::read(&index_path).unwrap(), original_index);
    assert_eq!(std::fs::read(&manifest_path).unwrap(), original_manifest);
    assert_eq!(
        std::fs::read_dir(&store).unwrap().count(),
        1,
        "check must not create ownership or index lock files"
    );
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);

    // Positive control: this is a valid recoverable record, not an invalid
    // sentinel that ordinary startup would silently skip.
    let recovered = ImageJobs::with_home(&home).unwrap();
    let status = recovered.status(id).unwrap();
    assert!(status.recovered && status.paused);
    assert_ne!(std::fs::read(&manifest_path).unwrap(), original_manifest);
    drop(recovered);
    std::fs::remove_dir_all(root).unwrap();
}
