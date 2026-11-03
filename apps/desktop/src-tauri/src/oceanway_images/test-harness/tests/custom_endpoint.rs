use oceanway_image_service_tests::oceanway_images;
use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
use serde_json::json;
use std::{io::{Read, Write}, net::TcpListener, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};

#[tokio::test]
async fn installed_stdio_process_discovers_and_generates_at_the_saved_custom_endpoint() {
    use base64::Engine;
    let home = std::env::temp_dir().join(format!("oceanway-custom-stdio-{}",std::process::id()));
    std::fs::create_dir(&home).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}/custom/v1",listener.local_addr().unwrap());
    oceanway_images::configure_at(&home,&base,"custom-stdio-key",Some("studio-drawing-v2"),&["studio-drawing-v2".into()]).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        let mut image = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2,2).write_to(&mut image,image::ImageFormat::Png).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(image.into_inner());
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut paths = Vec::new();
        while !server_stop.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(5)); continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0]; stream.read_exact(&mut byte).unwrap(); header.push(byte[0]);
            }
            let header = String::from_utf8(header).unwrap().to_lowercase();
            assert!(header.contains("authorization: bearer custom-stdio-key"));
            let request = header.lines().next().unwrap().to_owned();
            let response = if request.starts_with("get /custom/v1/models ") {
                json!({"data":[{"id":"gpt-image-fixture"},{"id":"studio-drawing-v2","supported_endpoint_types":["openai","image-generation"]},{"id":"other-picture","model_type":"image"},{"id":"gpt-text"}]})
            } else {
                assert!(request.starts_with("post /custom/v1/images/generations "),"{request}");
                let size: usize = header.lines().find_map(|line|line.strip_prefix("content-length: ")).unwrap().parse().unwrap();
                let mut body = vec![0;size]; stream.read_exact(&mut body).unwrap();
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["model"],"studio-drawing-v2");
                assert!(body.get("async").is_none());
                json!({"data":[{"b64_json":encoded}]})
            }.to_string();
            paths.push(request);
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Task-ID: completed-tracking-id\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
        }
        paths
    });
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_oceanway-image-mcp-fixture"));
    command.arg("--image-mcp-stdio").env("OCEANWAY_CONFIG_HOME",&home)
        .env_remove("OCEANWAY_MCP_CHECK_ONLY").kill_on_drop(true);
    #[cfg(windows)] command.creation_flags(0x08000000);
    let mut client = tokio::time::timeout(Duration::from_secs(10),().serve(TokioChildProcess::new(command).unwrap())).await.unwrap().unwrap();
    let catalog = client.call_tool(CallToolRequestParams::new("get_image_models")).await.unwrap();
    assert_ne!(catalog.is_error,Some(true));
    assert_eq!(catalog.structured_content.unwrap()["models"],json!(["gpt-image-fixture","other-picture","studio-drawing-v2"]));
    let generated = client.call_tool(CallToolRequestParams::new("generate_images").with_arguments(
        json!({"workspace_directory":home,"prompt":"two small test pictures","count":2}).as_object().unwrap().clone())).await.unwrap();
    assert_ne!(generated.is_error,Some(true));
    let initial = generated.structured_content.unwrap();
    let done = tokio::time::timeout(Duration::from_secs(10),async {
        loop {
            let result = client.call_tool(CallToolRequestParams::new("get_image_job").with_arguments(
                json!({"job_id":initial["id"],"wait_seconds":1}).as_object().unwrap().clone())).await.unwrap();
            assert_ne!(result.is_error,Some(true));
            let value = result.structured_content.unwrap();
            if value["completed"] == 2 {break value;}
            assert_eq!(value["failed"],0);
        }
    }).await.unwrap();
    for item in done["items"].as_array().unwrap() {
        assert!(std::path::Path::new(item["path"].as_str().unwrap()).is_file());
    }
    assert!(!done.to_string().contains("custom-stdio-key"));
    client.close_with_timeout(Duration::from_secs(5)).await.unwrap();
    stop.store(true,Ordering::SeqCst);
    let requests = server.join().unwrap();
    assert_eq!(requests.len(),3);
    std::fs::remove_dir_all(home).unwrap();
}
