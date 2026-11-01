use oceanway_image_service_tests::oceanway_images::mcp_stdio;
use std::path::Path;

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
