//! Codex lifecycle for the Oceanway wizard. Only a confirmed apply may stop clients.
#[cfg(target_os = "windows")]
mod platform {
    use std::sync::Mutex;

    // A portable install may only be discoverable while its window is still running.
    static LAST_HOST: Mutex<Option<String>> = Mutex::new(None);

    fn run(action: &str) -> Result<serde_json::Value, String> {
        let script = concat!(
            include_str!("oceanway_codex_windows.ps1"),
            "\ntry { Invoke-CodexLifecycle $env:OW_CODEX_ACTION $env:OW_PREVIOUS_HOST | ConvertTo-Json -Compress } ",
            "catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }"
        );
        let previous = LAST_HOST.lock().map_err(|_| "Codex 状态不可用")?.clone();
        let output = crate::agent_desktops::bounded_output(
            crate::agent_desktops::windows_powershell(script)
                .env("OW_CODEX_ACTION", action)
                .env("OW_PREVIOUS_HOST", previous.unwrap_or_default()),
            50,
        )?;
        if !output.status.success() {
            return Err(format!(
                "Codex 操作未完成：{}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        serde_json::from_slice(&output.stdout).map_err(|_| "无法读取 Codex 操作结果，请重试".into())
    }

    pub fn running() -> Result<bool, String> {
        run("probe")?["running"]
            .as_bool()
            .ok_or_else(|| "无法确认 Codex 状态".into())
    }

    pub fn installed() -> bool {
        run("discover").is_ok_and(|result| result["found"] == true)
    }

    pub fn close() -> Result<(), String> {
        if let Some(path) = run("close")?["path"].as_str() {
            *LAST_HOST.lock().map_err(|_| "Codex 状态不可用")? = Some(path.to_owned());
        }
        Ok(())
    }

    pub fn launch_codex(restart: bool) -> Result<Option<String>, String> {
        let result = run(if restart { "restart" } else { "open" })?;
        if result["restartRequired"] == true {
            return Err("OCEANWAY_CODEX_RESTART_REQUIRED".into());
        }
        Ok((result["found"] == true).then(|| "Codex 窗口已打开，可以开始对话。".into()))
    }
}

#[cfg(target_os = "macos")]
#[path = "oceanway_codex_macos.rs"]
mod platform;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    pub fn running() -> Result<bool, String> {
        Ok(false)
    }
    pub fn close() -> Result<(), String> {
        Ok(())
    }
    pub fn launch_codex(_: bool) -> Result<Option<String>, String> {
        Ok(None)
    }
}

pub use platform::{close, launch_codex, running};
#[cfg(target_os = "windows")]
pub use platform::installed;
