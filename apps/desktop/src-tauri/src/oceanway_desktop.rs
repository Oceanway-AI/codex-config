//! Graceful restart for the real Codex desktop host. Never kills a terminal or
//! every process sharing an executable name. Only called after an explicit click.
#[cfg(target_os = "windows")]
pub fn launch_codex(restart: bool) -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;
    // All script text is fixed; no API key, model or user path is interpolated.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
try {
  $roots = @(Get-Process -Name Codex -ErrorAction SilentlyContinue | Where-Object {
    $_.MainWindowHandle -ne 0 -and $_.Path -and
    (Test-Path -LiteralPath (Join-Path (Split-Path -Parent $_.Path) 'resources\app.asar'))
  })
  if ($roots.Count -gt 1) { throw '检测到多个 Codex 主窗口，请手动选择并关闭后重试。' }
  $executable = if ($roots.Count -eq 1) { $roots[0].Path } else { $null }
  if (-not $executable) {
    $candidates = @(
      (Join-Path $env:LOCALAPPDATA 'Programs\Codex\Codex.exe'),
      (Join-Path $env:LOCALAPPDATA 'Codex\Codex.exe')
    )
    $packages = @(Get-AppxPackage -Name '*Codex*' -ErrorAction SilentlyContinue)
    foreach ($package in $packages) {
      $candidates += Join-Path $package.InstallLocation 'app\Codex.exe'
      $candidates += Join-Path $package.InstallLocation 'Codex.exe'
    }
    foreach ($candidate in $candidates) {
      if ((Test-Path -LiteralPath $candidate -PathType Leaf) -and
          (Test-Path -LiteralPath (Join-Path (Split-Path -Parent $candidate) 'resources\app.asar'))) {
        $executable = $candidate; break
      }
    }
  }
  if (-not $executable) { @{found=$false;ok=$true} | ConvertTo-Json -Compress; exit 0 }
  if ($env:OCEANWAY_RESTART_REQUEST -eq '1' -and $roots.Count -eq 1) {
    if (-not $roots[0].CloseMainWindow()) { throw 'Codex 没有接受正常关闭请求，请保存任务后重试。' }
    if (-not $roots[0].WaitForExit(10000)) { throw 'Codex 尚未退出，可能有任务或确认窗口；已停止重启，请处理后重试。' }
  }
  Start-Process -FilePath $executable | Out-Null
  @{found=$true;ok=$true} | ConvertTo-Json -Compress
} catch {
  @{found=$true;ok=$false;error=$_.Exception.Message} | ConvertTo-Json -Compress
}
"#;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("OCEANWAY_RESTART_REQUEST", if restart { "1" } else { "0" })
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("无法检测 Codex 桌面应用：{e}"))?;
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Codex 桌面检测失败，请手动打开应用".to_owned())?;
    if value["ok"] != true {
        return Err(value["error"]
            .as_str()
            .unwrap_or("Codex 重启未完成")
            .to_owned());
    }
    Ok((value["found"] == true).then(|| {
        if restart {
            "Codex 桌面应用已正常关闭并重新启动，新配置将在新请求中生效".into()
        } else {
            "已请求打开 Codex 桌面应用".into()
        }
    }))
}

#[cfg(target_os = "macos")]
pub fn launch_codex(restart: bool) -> Result<Option<String>, String> {
    use std::{path::PathBuf, process::Command, thread, time::Duration};
    fn processes() -> String {
        Command::new("ps")
            .args(["-axo", "command="])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    }
    let process_list = processes();
    let home = crate::config::get_home_dir();
    let selected = ["Codex", "ChatGPT"].into_iter().find_map(|name| {
        [PathBuf::from("/Applications"), home.join("Applications")]
            .into_iter()
            .find_map(|base| {
                let path = base.join(format!("{name}.app"));
                let executable = path.join(format!("Contents/MacOS/{name}"));
                let supported = name == "Codex" || path.join("Contents/Resources/codex").is_file();
                (executable.is_file() && supported).then_some((name, path, executable))
            })
    });
    let Some((name, path, executable)) = selected else {
        return Ok(None);
    };
    let needle = executable.to_string_lossy();
    if restart && process_list.contains(needle.as_ref()) {
        let status = Command::new("osascript")
            .args(["-e", &format!("tell application \"{name}\" to quit")])
            .status()
            .map_err(|e| format!("无法正常退出应用：{e}"))?;
        if !status.success() {
            return Err("应用拒绝退出，请保存任务后重试".into());
        }
        for _ in 0..50 {
            if !processes().contains(needle.as_ref()) {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        if processes().contains(needle.as_ref()) {
            return Err("应用仍在运行，已停止重启，请处理当前任务后重试".into());
        }
    }
    let status = Command::new("open")
        .arg(path)
        .status()
        .map_err(|e| format!("无法打开应用：{e}"))?;
    if !status.success() {
        return Err("应用未能重新打开；新配置和历史记录已保留".into());
    }
    Ok(Some(format!(
        "{name} 已{}，新配置将在新请求中生效",
        if restart { "重新启动" } else { "打开" }
    )))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn launch_codex(_restart: bool) -> Result<Option<String>, String> {
    Ok(None)
}
