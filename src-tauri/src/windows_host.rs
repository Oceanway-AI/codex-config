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
    // An installed but idle application must not displace the running host.
    hosts
        .iter()
        .find(|h| h.running)
        .or_else(|| hosts.iter().find(|h| h.name == "ChatGPT"))
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
    // Keep the Windows command line bounded. The full script (including long
    // profile paths) is UTF-8 on stdin, not expanded into -EncodedCommand.
    use base64::{engine::general_purpose::STANDARD, Engine};
    let script = format!("$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);\n{}\n{script}",
        include_str!("windows_process_helpers.ps1"));
    let bootstrap = "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false); & ([ScriptBlock]::Create([Console]::In.ReadToEnd()))";
    let bytes: Vec<u8> = bootstrap.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &STANDARD.encode(bytes),
        ])
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "无法启动 Windows 应用检测，请检查 PowerShell 是否可用。".to_string())?;
    // Drain output while waiting so even a large response cannot deadlock the pipe.
    let mut stdout = child.stdout.take().ok_or("无法读取检测结果。")?;
    let reader = thread::spawn(move || {
        use std::io::Read;
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let mut stderr = child.stderr.take().ok_or("无法读取操作错误。")?;
    let error_reader = thread::spawn(move || {
        use std::io::Read;
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let mut input = child.stdin.take().ok_or("无法发送 Windows 操作脚本。")?;
    use std::io::Write;
    if input.write_all(script.as_bytes()).is_err() {
        drop(input);
        let _ = child.kill();
        let _ = child.wait();
        return Err("无法发送 Windows 操作脚本，操作未完成。".into());
    }
    drop(input);
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
                let detail = error_reader.join().unwrap_or_default();
                // Scripts carry paths and process IDs only, never credentials.
                return Err(format!("Windows 重启未完成，配置已保存：{}", detail.chars().take(1200).collect::<String>()));
            }
            let _ = error_reader.join();
            return String::from_utf8(output).map_err(|_| "Windows 检测结果编码无效。".into());
        }
        if start.elapsed() > Duration::from_secs(60) {
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
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$processes = @(Get-CimInstance Win32_Process | Where-Object { $_.SessionId -eq $session })
$apps = @(Get-StartApps)
$result = @()
foreach ($name in @('ChatGPT', 'Codex')) {
    $exe = "$name.exe"
    $live = @($processes | Where-Object {
        $_.Name -ieq $exe -and $_.ExecutablePath -match '\\(?:app|Codex)\\' -and
        $_.CommandLine -notmatch '(?:^|\s)--type(?:=|\s)|app-server|--version' -and
        !(Get-DesktopProfile $_.CommandLine) -and
        (Invoke-CimMethod -InputObject $_ -MethodName GetOwnerSid).Sid -eq $sid
    })
    $app = $apps | Where-Object { $_.Name -ieq $name } | Select-Object -First 1
    $path = $live | Where-Object { $_.ExecutablePath } | Select-Object -First 1 -ExpandProperty ExecutablePath
    if (!$path) {
        foreach ($candidate in @("$env:LOCALAPPDATA\Programs\$name\$exe", "$env:LOCALAPPDATA\$name\$exe")) {
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { $path = $candidate; break }
        }
        if (!$path -and $name -eq 'ChatGPT') {
            $package = Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
            if ($package) {
                $candidate = Join-Path $package.InstallLocation 'app\ChatGPT.exe'
                if (Test-Path -LiteralPath $candidate -PathType Leaf) { $path = $candidate }
            }
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
        if ($path) { $version = Get-DesktopVersion $path }
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

#[cfg(target_os = "windows")]
pub fn restart() -> Result<super::RestartCodexResult, String> {
    use serde_json::json;
    let mut target = if std::env::var_os("CODEX_HOME").is_some()
        || std::env::var_os("OCEANWAY_RESTART_TARGET").is_some()
        || std::env::var_os("CODEX_ELECTRON_USER_DATA_PATH").is_some() {
        // Test mode is explicit and fail-closed; it never falls back to normal discovery.
        if std::env::var_os("CODEX_HOME").is_none() {
            return Err("隔离重启未指定 CODEX_HOME，未操作桌面。".into());
        }
        let home = super::codex_home()?.canonicalize().map_err(|_| "隔离配置目录不可用。")?;
        let manifest = std::env::var_os("OCEANWAY_RESTART_TARGET")
            .map(std::path::PathBuf::from)
            .ok_or("配置已保存。自定义目录没有显式绑定测试桌面，未操作日常 Codex。")?
            .canonicalize().map_err(|_| "隔离桌面清单不可用，未操作日常 Codex。")?;
        if manifest != home.join("oceanway-test-desktop.json") {
            return Err("隔离桌面清单路径不匹配。".into());
        }
        let text = std::fs::read_to_string(&manifest)
            .map_err(|_| "配置已保存。自定义 CODEX_HOME 未绑定隔离桌面，已阻止重启，不会关闭现有 Codex。")?;
        let mut target: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| "隔离桌面清单无效，未关闭任何窗口。")?;
        let profile = target["userData"].as_str().ok_or("隔离桌面缺少独立数据目录。")?;
        let root = home.parent().ok_or("隔离目录无效。")?;
        let profile_path = std::path::Path::new(profile).canonicalize().map_err(|_| "隔离桌面目录不可用。")?;
        let declared_home = target["codexHome"].as_str().and_then(|p| std::path::Path::new(p).canonicalize().ok());
        let marker: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".oceanway-isolation.json")).map_err(|_| "缺少隔离验收目录标记。")?
        ).map_err(|_| "隔离目录标记无效。")?;
        if marker["purpose"] != "oceanway-codex-isolated-acceptance"
            || target["isolated"] != true || declared_home.as_ref() != Some(&home)
            || home != root.join("codex-home") || profile_path != root.join("desktop-profile")
            || target["pid"].as_u64().is_none()
            || target["started"].as_str().is_none()
            || !target["engine"].as_str().is_some_and(|p| std::path::Path::new(p).is_file())
            || !target["path"].as_str().is_some_and(|p| std::path::Path::new(p).is_file())
            || !target["port"].as_u64().is_some_and(|port| (1024..=65535).contains(&port)) {
            return Err("隔离桌面清单与本次配置目录不匹配，未关闭任何窗口。".into());
        }
        target["protectedPids"] = marker["protectedPids"].clone();
        if !target["protectedPids"].as_array().is_some_and(|pids| !pids.is_empty()
            && pids.iter().all(|p| p.as_u64().is_some())) {
            return Err("缺少需要保护的日常桌面身份，未执行测试重启。".into());
        }
        target["manifest"] = json!(manifest.to_string_lossy());
        target
    } else {
        let host = discover()?.ok_or("未找到 Codex 桌面，请确认应用已安装。")?;
        let path = host.path.ok_or("无法确认桌面可执行文件，请手动打开应用后重试。")?;
        json!({"isolated":false,"path":path})
    };
    target["configurationApp"] = json!(std::env::current_exe()
        .map_err(|_| "无法定位语言配置程序。")?.to_string_lossy());
    let serialized = serde_json::to_string(&target).map_err(|_| "无法建立重启目标。")?;
    let script = format!("$target = '{}' | ConvertFrom-Json\n{}", serialized.replace('\'', "''"),
        include_str!("restart_windows.ps1"));
    let result: serde_json::Value = serde_json::from_str(&powershell(&script)?)
        .map_err(|_| "重启结果无法确认，请检查桌面。")?;
    Ok(super::RestartCodexResult {
        restarted: true,
        was_running: result["wasRunning"].as_bool().unwrap_or(false),
        message: "旧宿主已退出，新桌面窗口已出现。请新建任务；配置加载与图片能力仍需实际验证。".into(),
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
    fn running_host_precedes_idle_installation() {
        let hosts = [host("Codex Desktop", true), host("ChatGPT", false)];
        assert_eq!(select_host(&hosts).unwrap().name, "Codex Desktop");
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn powershell_scripts_parse_without_running_app_operations() {
        for source in [DISCOVER.to_string(), include_str!("restart_windows.ps1").to_string()] {
            let check = format!("$tokens=$null; $errors=$null; $null=[System.Management.Automation.Language.Parser]::ParseInput('{}',[ref]$tokens,[ref]$errors); if($errors.Count){{throw 'Syntax error'}}; 'ok'", source.replace('\'', "''"));
            assert_eq!(powershell(&check).unwrap().trim(), "ok");
        }
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn long_unicode_scripts_do_not_exceed_the_windows_command_line_limit() {
        let script = format!("# {}\n'长路径测试成功'", "x".repeat(40_000));
        assert_eq!(powershell(&script).unwrap().trim(), "长路径测试成功");
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_argv_handles_both_quoted_profile_forms() {
        for command in [
            r#""C:\App\ChatGPT.exe" "--user-data-dir=C:\private profile""#,
            r#""C:\App\ChatGPT.exe" --user-data-dir="C:\private profile""#,
            r#""C:\App\ChatGPT.exe" --user-data-dir "C:\private profile""#,
        ] {
            let script = format!("Get-DesktopProfile '{}'", command.replace('\'', "''"));
            assert_eq!(powershell(&script).unwrap().trim(), r"C:\private profile");
        }
    }
    #[test]
    fn real_restart_binds_handles_and_requires_a_new_window() {
        let script = include_str!("restart_windows.ps1");
        assert!(script.contains("$null = $handle.Handle"));
        assert!(script.contains("$target.started"));
        assert!(script.contains("$protected -contains"));
        assert!(script.contains("if ($target.isolated)"));
        assert!(script.contains("$p.Kill()"));
        assert!(script.contains("$launched.MainWindowHandle -ne 0"));
        assert!(script.contains("$hadServer -or $serverReady"));
        assert!(!script.contains("taskkill"));
        assert!(!script.contains("Stop-Process"));
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
