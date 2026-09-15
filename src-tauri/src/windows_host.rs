//! Windows host discovery uses current-user processes and Start menu registration.
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub name: String,
    pub app_id: Option<String>,
    pub path: Option<String>,
    pub version: Option<String>,
    pub running: bool,
    pub server_running: bool,
}

pub fn select_host(hosts: &[Host]) -> Option<&Host> {
    // An installed ChatGPT takes precedence over the legacy desktop client.
    hosts
        .iter()
        .find(|h| h.name == "ChatGPT")
        .or_else(|| hosts.iter().find(|h| h.name == "Codex Desktop"))
}

pub fn parse_hosts(json: &str) -> Result<Vec<Host>, String> {
    serde_json::from_str(json).map_err(|_| "Windows 应用检测返回了无效数据，请重试。".into())
}

#[cfg(target_os = "windows")]
fn powershell(script: &str) -> Result<String, String> {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    // EncodedCommand preserves Unicode paths and avoids shell interpolation.
    use base64::{engine::general_purpose::STANDARD, Engine};
    let script = format!("$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); {script}");
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &STANDARD.encode(bytes),
        ])
        .creation_flags(0x08000000)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "无法启动 Windows 应用检测，请检查 PowerShell 是否可用。".to_string())?;
    // Drain output while waiting so even a large response cannot deadlock the pipe.
    let mut stdout = child.stdout.take().ok_or("无法读取检测结果。")?;
    let reader = thread::spawn(move || {
        use std::io::Read;
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "无法读取 Windows 检测进程状态。")?
        {
            let output = reader
                .join()
                .map_err(|_| "读取检测结果失败。")?
                .map_err(|_| "读取检测结果失败。")?;
            if !status.success() {
                return Err(
                    "Windows 应用操作失败；请检查权限，并确认 ChatGPT 已退出后重试。".into(),
                );
            }
            return String::from_utf8(output).map_err(|_| "Windows 检测结果编码无效。".into());
        }
        if start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Windows 应用操作超时，请手动检查 ChatGPT 状态后重试。".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(target_os = "windows")]
const DISCOVER: &str = r#"
$session = (Get-Process -Id $PID).SessionId
$processes = @(Get-CimInstance Win32_Process | Where-Object { $_.SessionId -eq $session })
$apps = @(Get-StartApps)
$result = @()
foreach ($name in @('ChatGPT', 'Codex')) {
    $exe = "$name.exe"
    $live = @($processes | Where-Object { $_.Name -ieq $exe -and ($name -eq 'ChatGPT' -or ($_.ExecutablePath -match '\\(?:app|Codex)\\' -and $_.CommandLine -notmatch 'app-server|--version')) })
    $app = $apps | Where-Object { $_.Name -ieq $name } | Select-Object -First 1
    $path = $live | Where-Object { $_.ExecutablePath } | Select-Object -First 1 -ExpandProperty ExecutablePath
    if (!$path) {
        foreach ($candidate in @("$env:LOCALAPPDATA\Programs\$name\$exe", "$env:LOCALAPPDATA\$name\$exe")) {
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { $path = $candidate; break }
        }
    }
    if ($app -or $path -or $live.Count) {
        $ids = @($live | ForEach-Object { [uint32]$_.ProcessId })
        # Follow descendants: app-server can sit behind a helper process.
        for ($i=0; $i -lt 12; $i++) {
            $children = @($processes | Where-Object { $ids -contains [uint32]$_.ParentProcessId -and $ids -notcontains [uint32]$_.ProcessId } | ForEach-Object { [uint32]$_.ProcessId })
            if (!$children.Count) { break }; $ids += $children
        }
        $server = @($processes | Where-Object { $ids -contains [uint32]$_.ProcessId -and $_.Name -match '^codex(?:[-_].*)?\.exe$' -and $_.CommandLine -match '(?:^|\s)app-server(?:\s|$)' }).Count -gt 0
        $version = $null
        if ($path) { try { $version = (Get-Item -LiteralPath $path).VersionInfo.ProductVersion } catch {} }
        $result += @{name=$(if($name -eq 'Codex'){'Codex Desktop'}else{'ChatGPT'}); appId=$app.AppID; path=$path; version=$version; running=($live.Count -gt 0); serverRunning=$server}
    }
}
ConvertTo-Json -InputObject @($result) -Compress
"#;

#[cfg(target_os = "windows")]
pub fn discover() -> Result<Option<Host>, String> {
    let hosts = parse_hosts(&powershell(DISCOVER)?)?;
    Ok(select_host(&hosts).cloned())
}

fn restart_script(host: &Host) -> Result<String, String> {
    let executable = if host.name == "ChatGPT" {
        "ChatGPT"
    } else {
        "Codex"
    };
    // Legacy CLI and desktop share a process name: only close the discovered desktop path.
    let path_filter = if host.name == "Codex Desktop" {
        let path = host
            .path
            .as_deref()
            .ok_or("无法确认旧版 Codex 的桌面进程路径，请手动重启。")?;
        format!(" -and $_.Path -ieq '{}'", path.replace('\'', "''"))
    } else {
        String::new()
    };
    let launch = if let Some(id) = host.app_id.as_deref().filter(|id| !id.is_empty()) {
        format!(
            "Start-Process 'shell:AppsFolder\\{}'",
            id.replace('\'', "''")
        )
    } else if let Some(path) = host.path.as_deref() {
        format!("Start-Process -FilePath '{}'", path.replace('\'', "''"))
    } else {
        return Err("已检测到应用，但无法确定启动入口，请手动重启。".into());
    };
    // Do not force-kill: a save prompt or tray-only host must stop the workflow.
    let script = format!(
        r#"
$session = (Get-Process -Id $PID).SessionId
$old = @(Get-Process -Name '{executable}' -ErrorAction SilentlyContinue | Where-Object {{ $_.SessionId -eq $session{path_filter} }})
foreach ($p in $old) {{ if ($p.MainWindowHandle -ne 0) {{ $null = $p.CloseMainWindow() }} }}
$deadline = [DateTime]::UtcNow.AddSeconds(12)
do {{
    $remaining = @($old | Where-Object {{ !$_.HasExited }})
    if (!$remaining.Count) {{ break }}
    Start-Sleep -Milliseconds 250
}} while ([DateTime]::UtcNow -lt $deadline)
if ($remaining.Count) {{ throw 'Host did not exit' }}
{launch}
$deadline = [DateTime]::UtcNow.AddSeconds(12)
do {{
    $new = @(Get-Process -Name '{executable}' -ErrorAction SilentlyContinue | Where-Object {{ $_.SessionId -eq $session{path_filter} }})
    if ($new.Count) {{ 'started'; exit 0 }}
    Start-Sleep -Milliseconds 250
}} while ([DateTime]::UtcNow -lt $deadline)
throw 'Host did not start'
"#
    );
    Ok(script)
}

#[cfg(target_os = "windows")]
pub fn restart() -> Result<super::RestartCodexResult, String> {
    let host = discover()?.ok_or("未找到 ChatGPT 或旧版 Codex，请确认应用已安装。")?;
    powershell(&restart_script(&host)?)?;
    Ok(super::RestartCodexResult {
        restarted: true,
        was_running: host.running,
        message: format!(
            "{} 已重新启动。请进入 Codex 并新建任务；尚未验证配置已被任务加载。",
            host.name
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host(name: &str, running: bool) -> Host {
        Host {
            name: name.into(),
            app_id: None,
            path: None,
            version: None,
            running,
            server_running: false,
        }
    }
    #[test]
    fn chatgpt_precedes_legacy() {
        let hosts = [host("Codex Desktop", true), host("ChatGPT", false)];
        assert_eq!(select_host(&hosts).unwrap().name, "ChatGPT");
    }
    #[test]
    fn store_launch_and_safe_close_are_generated() {
        let mut app = host("ChatGPT", true);
        app.app_id = Some("OpenAI.ChatGPT_example!App".into());
        let script = restart_script(&app).unwrap();
        assert!(script.contains("shell:AppsFolder\\OpenAI.ChatGPT_example!App"));
        assert!(script.contains("CloseMainWindow"));
        assert!(script.contains("if ($remaining.Count) { throw"));
        assert!(script.contains("throw 'Host did not start'"));
        assert!(!script.contains("taskkill"));
        assert!(!script.contains("Stop-Process"));
    }
    #[test]
    fn executable_launch_escapes_paths_and_scopes_legacy_processes() {
        let mut app = host("Codex Desktop", true);
        assert!(restart_script(&app).is_err());
        app.path = Some("C:\\Users\\O'Brien\\Codex\\Codex.exe".into());
        let script = restart_script(&app).unwrap();
        assert!(script.contains("O''Brien"));
        assert_eq!(script.matches("$_.Path -ieq").count(), 2);
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn powershell_scripts_parse_without_running_app_operations() {
        let mut app = host("ChatGPT", true);
        app.app_id = Some("OpenAI.ChatGPT_example!App".into());
        for source in [DISCOVER.to_string(), restart_script(&app).unwrap()] {
            let check = format!("$tokens=$null; $errors=$null; $null=[System.Management.Automation.Language.Parser]::ParseInput('{}',[ref]$tokens,[ref]$errors); if($errors.Count){{throw 'Syntax error'}}; 'ok'", source.replace('\'', "''"));
            assert_eq!(powershell(&check).unwrap().trim(), "ok");
        }
    }
    #[test]
    fn legacy_fallback_and_missing_host() {
        assert!(select_host(&[]).is_none());
        assert_eq!(
            select_host(&[host("Codex Desktop", true)]).unwrap().name,
            "Codex Desktop"
        );
    }
    #[test]
    fn invalid_detection_is_not_not_running() {
        assert!(parse_hosts("garbage").is_err());
        assert!(parse_hosts("null").is_err());
        assert!(parse_hosts("[]").unwrap().is_empty());
    }
    #[test]
    fn chatgpt_running_does_not_imply_server_running() {
        let parsed = parse_hosts(r#"[{"name":"ChatGPT","appId":"OpenAI.ChatGPT_example!App","path":null,"version":null,"running":true,"serverRunning":false}]"#).unwrap();
        assert!(parsed[0].running);
        assert!(!parsed[0].server_running);
        assert!(parsed[0].app_id.is_some());
        assert!(parsed[0].path.is_none());
        assert!(parsed[0].version.is_none());
    }
}
