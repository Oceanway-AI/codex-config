//! Oceanway-owned native image service, shared by every configured agent.
//!
//! This module never reads or writes agent auth files, session databases, or chat
//! history. Pass the application's own data directory to `ImageJobs::with_home`
//! and `configure`; MCP child processes receive it through OCEANWAY_CONFIG_HOME.

pub mod image_api;
pub mod mcp_stdio;
pub mod video_api;
mod provider;

pub use image_api::{
    ImageCapabilities, ImageItem, ImageJob, ImageJobs, ImageOutput, ImageSubmissionGuard,
    ImageTestRequest,
};
pub use provider::{configure, read_api_key, read_provider_summary, ProviderSummary, API_BASE};

use std::path::PathBuf;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCheck {
    pub tools_available: bool,
    pub message: String,
}

/// Secret-free stdio entry for the existing CC Switch MCP adapters. The
/// application keeps ownership of registration and per-agent configuration.
pub fn mcp_server_spec(home: &std::path::Path) -> Result<serde_json::Value, String> {
    let home = std::fs::canonicalize(home)
        .map_err(|_| "The Oceanway application data directory does not exist.".to_string())?;
    if !home.is_dir() {
        return Err("Oceanway application data is not a directory.".into());
    }
    let executable = installed_mcp_executable()?;
    Ok(serde_json::json!({
        "command": executable,
        "args": ["--image-mcp-stdio"],
        "env": { "OCEANWAY_CONFIG_HOME": home },
        "startup_timeout_sec": 30,
        "tool_timeout_sec": 60
    }))
}

fn installed_mcp_executable() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|_| "Could not locate the installed Oceanway application.".to_string())?;
    #[cfg(target_os = "linux")]
    {
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
        return select_mcp_executable(&executable, appimage.as_deref(), appdir.as_deref());
    }
    #[cfg(not(target_os = "linux"))]
    Ok(executable)
}

#[cfg(any(target_os = "linux", test))]
fn select_mcp_executable(
    executable: &std::path::Path,
    appimage: Option<&std::path::Path>,
    appdir: Option<&std::path::Path>,
) -> Result<PathBuf, String> {
    // AppImage's actual executable lives in a temporary mount that disappears
    // when the GUI exits. Only use APPIMAGE when APPDIR owns this executable;
    // an unrelated parent AppImage may have left those variables in the env.
    if appdir.is_some_and(|directory| executable.starts_with(directory)) {
        let image = appimage
            .and_then(|path| std::fs::canonicalize(path).ok())
            .filter(|path| path.is_file())
            .ok_or_else(|| "Could not locate the installed Oceanway AppImage.".to_string())?;
        return Ok(image);
    }
    Ok(executable.into())
}

/// Call before Tauri initializes. `None` means this is an ordinary GUI launch.
/// The caller must exit with a nonzero code when the returned result is Err.
/// This mode never writes protocol-unrelated output to stdout.
pub fn run_stdio_if_requested() -> Option<Result<(), String>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 || args[0] != "--image-mcp-stdio" {
        return None;
    }
    Some((|| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|_| "Could not initialize the native image runtime.".to_string())?;
        let result = runtime.block_on(mcp_stdio::serve_stdio());
        runtime.shutdown_timeout(std::time::Duration::from_secs(5));
        result
    })())
}

fn app_home() -> Result<PathBuf, String> {
    let home = std::env::var_os("OCEANWAY_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            "OCEANWAY_CONFIG_HOME must name the Oceanway application data directory.".to_string()
        })?;
    let home = std::fs::canonicalize(home)
        .map_err(|_| "The Oceanway application data directory does not exist.".to_string())?;
    if !home.is_dir() {
        return Err("The Oceanway application data path is not a directory.".into());
    }
    Ok(home)
}

fn validate_base_url(base: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(base).map_err(|_| "Invalid image API endpoint.".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || base.chars().any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err("Invalid image API endpoint.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appimage_registration_uses_the_persistent_bundle_not_its_temporary_mount() {
        let root =
            std::env::temp_dir().join(format!("oceanway-appimage-spec-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let image = root.join("Oceanway.AppImage");
        std::fs::write(&image, b"test-only fixture").unwrap();
        let mount = root.join(".mount_oceanway");
        let executable = mount.join("usr/bin/oceanway-config");
        assert_eq!(
            select_mcp_executable(&executable, Some(&image), Some(&mount)).unwrap(),
            std::fs::canonicalize(&image).unwrap()
        );
        assert!(select_mcp_executable(&executable, None, Some(&mount)).is_err());
        let regular = root.join("usr/bin/other-installed-oceanway");
        assert_eq!(
            select_mcp_executable(&regular, Some(&image), Some(&mount)).unwrap(),
            regular
        );
        std::fs::remove_file(image).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
