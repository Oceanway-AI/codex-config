//! Graceful restart for the real Codex desktop host. Never kills a terminal or
//! every process sharing an executable name. Only called after an explicit click.
#[cfg(target_os = "windows")]
pub fn launch_codex(restart: bool) -> Result<Option<String>, String> {
    // All script text is fixed; no API key, model or user path is interpolated.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
function Test-CodexHost($path) {
  if(-not $path -or -not (Test-Path -LiteralPath $path -PathType Leaf)){return $false}
  $resources=Join-Path (Split-Path -Parent $path) 'resources'
  return (Test-Path -LiteralPath (Join-Path $resources 'app.asar')) -and ((Split-Path -Leaf $path) -eq 'Codex.exe' -or (Test-Path -LiteralPath (Join-Path $resources 'codex.exe')) -or (Test-Path -LiteralPath (Join-Path $resources 'codex-cli\bin\codex.exe')))
}
try {
  $roots = @(Get-Process -Name Codex,ChatGPT -ErrorAction SilentlyContinue | Where-Object {
    $_.MainWindowHandle -ne 0 -and $_.Path -and
    (Test-CodexHost $_.Path)
  })
  if ($roots.Count -gt 1) { throw '检测到多个 Codex 主窗口，请手动选择并关闭后重试。' }
  $executable = if ($roots.Count -eq 1) { $roots[0].Path } else { $null }
  if (-not $executable) {
    $candidates = @(
      (Join-Path $env:LOCALAPPDATA 'Programs\Codex\Codex.exe'),
      (Join-Path $env:LOCALAPPDATA 'Codex\Codex.exe'),
      (Join-Path $env:LOCALAPPDATA 'Programs\ChatGPT\ChatGPT.exe'),
      (Join-Path $env:LOCALAPPDATA 'ChatGPT\ChatGPT.exe')
    )
    $packages = @(Get-AppxPackage -Name '*Codex*' -ErrorAction SilentlyContinue; Get-AppxPackage -Name '*ChatGPT*' -ErrorAction SilentlyContinue)
    foreach ($package in $packages) {
      $candidates += Join-Path $package.InstallLocation 'app\Codex.exe'
      $candidates += Join-Path $package.InstallLocation 'Codex.exe'
      $candidates += Join-Path $package.InstallLocation 'app\ChatGPT.exe'
      $candidates += Join-Path $package.InstallLocation 'ChatGPT.exe'
    }
    foreach ($candidate in $candidates) {
      if (Test-CodexHost $candidate) {
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
    let output = crate::agent_desktops::bounded_output(std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("OCEANWAY_RESTART_REQUEST", if restart { "1" } else { "0" }),25)
        .map_err(|e| format!("无法打开 Codex 桌面应用：{e}"))?;
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
    let candidates = ["Codex", "ChatGPT"]
        .into_iter()
        .flat_map(|name| {
            [PathBuf::from("/Applications"), home.join("Applications")]
                .into_iter()
                .filter_map(move |base| {
                    let path = base.join(format!("{name}.app"));
                    let executable = path.join(format!("Contents/MacOS/{name}"));
                    let supported =
                        name == "Codex" || path.join("Contents/Resources/codex").is_file();
                    (executable.is_file() && supported).then_some((
                        name.to_owned(),
                        path,
                        executable,
                    ))
                })
        })
        .collect::<Vec<_>>();
    let Some((name, path, executable)) = select_macos_host(&candidates, &process_list)? else {
        return Ok(None);
    };
    if restart && process_is_running(&process_list, executable) {
        let status = Command::new("osascript")
            .args(["-e", &format!("tell application \"{name}\" to quit")])
            .status()
            .map_err(|e| format!("无法正常退出应用：{e}"))?;
        if !status.success() {
            return Err("应用拒绝退出，请保存任务后重试".into());
        }
        for _ in 0..50 {
            if !process_is_running(&processes(), executable) {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        if process_is_running(&processes(), executable) {
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

#[cfg(any(target_os = "macos", test))]
fn process_is_running(processes: &str, executable: &std::path::Path) -> bool {
    let executable = executable.to_string_lossy();
    processes.lines().any(|line| {
        line.trim_start()
            .strip_prefix(executable.as_ref())
            .is_some_and(|tail| tail.is_empty() || tail.starts_with(' '))
    })
}

#[cfg(any(target_os = "macos", test))]
fn select_macos_host<'a>(
    candidates: &'a [(String, std::path::PathBuf, std::path::PathBuf)],
    processes: &str,
) -> Result<Option<&'a (String, std::path::PathBuf, std::path::PathBuf)>, String> {
    let running = candidates
        .iter()
        .filter(|(_, _, executable)| process_is_running(processes, executable))
        .collect::<Vec<_>>();
    match running.as_slice() {
        [] => Ok(candidates.first()),
        [candidate] => Ok(Some(*candidate)),
        _ => Err("检测到多个正在运行的 Codex 桌面宿主，请保存任务并手动关闭后重试".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oceanway_macos_restart_prefers_running_host_and_rejects_ambiguity() {
        let candidates = ["Codex", "ChatGPT"].map(|name| {
            (
                name.to_owned(),
                std::path::PathBuf::from(format!("/Applications/{name}.app")),
                std::path::PathBuf::from(format!("/Applications/{name}.app/Contents/MacOS/{name}")),
            )
        });
        let codex = candidates[0].2.to_string_lossy();
        let chatgpt = candidates[1].2.to_string_lossy();
        assert_eq!(
            select_macos_host(&candidates, &chatgpt).unwrap().unwrap().0,
            "ChatGPT"
        );
        assert!(select_macos_host(&candidates, &format!("{codex}\n{chatgpt}")).is_err());
        assert_eq!(
            select_macos_host(&candidates, "").unwrap().unwrap().0,
            "Codex"
        );
        assert!(!process_is_running(
            &format!("{codex} Helper"),
            &candidates[1].2
        ));
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn launch_codex(_restart: bool) -> Result<Option<String>, String> {
    Ok(None)
}
