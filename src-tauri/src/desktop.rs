use super::*;

#[cfg(target_os = "windows")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowsHost {
    path: String,
    version: Option<String>,
    running: bool,
    aumid: Option<String>,
}

#[cfg(target_os = "windows")]
fn windows_host() -> Option<WindowsHost> {
    // An executable name alone also matches the CLI. Verify the app installation.
    let script = r#"
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new()
$paths=@()
$aumids=@{}
$versions=@{}
Get-AppxPackage -Name OpenAI.Codex -ErrorAction SilentlyContinue | ForEach-Object {
  $package=$_
  $manifest=$package | Get-AppxPackageManifest
  foreach($name in @('ChatGPT.exe','Codex.exe')) {
    $p=Join-Path $_.InstallLocation "app\$name"
    if(Test-Path -LiteralPath $p){
      $paths += $p
      $versions[$p]=$package.Version.ToString()
      $application=@($manifest.Package.Applications.Application | Where-Object {
        (Join-Path $package.InstallLocation $_.Executable) -eq $p
      }) | Select-Object -First 1
      if($application){$aumids[$p]=$package.PackageFamilyName+'!'+$application.Id}
    }
  }
}
foreach($p in @("$env:LOCALAPPDATA\Programs\Codex\Codex.exe","$env:LOCALAPPDATA\Codex\Codex.exe")){
  if((Test-Path -LiteralPath $p) -and (Test-Path -LiteralPath (Join-Path (Split-Path $p) 'resources'))){$paths += $p}
}
$processes=@(Get-CimInstance Win32_Process -Filter "Name='ChatGPT.exe' OR Name='Codex.exe'" -ErrorAction SilentlyContinue)
$running=@($paths | Where-Object {$candidate=$_; $processes | Where-Object {$_.ExecutablePath -eq $candidate}})
$selected=if($running.Count){$running[0]}elseif($paths.Count){$paths[0]}else{$null}
if($selected){
  $version=if($versions[$selected]){$versions[$selected]}else{(Get-Item -LiteralPath $selected).VersionInfo.ProductVersion}
  @{path=$selected;version=$version;running=($running -contains $selected);aumid=$aumids[$selected]} | ConvertTo-Json -Compress
}
"#;
    let text = command_output("powershell", &["-NoProfile", "-NonInteractive", "-Command", script])?;
    serde_json::from_str(&text).ok()
}

#[cfg(target_os = "windows")]
pub(super) fn runtime_info() -> (Option<String>, Option<String>, bool) {
    match windows_host() {
        Some(host) => (host.version, Some("Codex Desktop".into()), host.running),
        None => (None, None, false),
    }
}

#[cfg(target_os = "windows")]
pub(super) fn restart() -> Result<RestartCodexResult, String> {
    let host = windows_host().ok_or("未检测到已确认的 Codex 桌面宿主，请手动重启；不会处理 CLI。")?;
    let script = r#"
$ErrorActionPreference='Stop'
$path=$env:OCEANWAY_CODEX_HOST
$processes=@(Get-Process | Where-Object {try {$_.Path -eq $path -and $_.MainWindowHandle -ne 0}catch{$false}})
foreach($p in $processes){if(-not $p.CloseMainWindow()){throw '宿主拒绝退出，请保存任务后手动重启。'}}
foreach($p in $processes){if(-not $p.WaitForExit(10000)){throw '宿主仍在运行，未强制结束，请保存任务后手动重启。'}}
"#;
    if host.running {
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("OCEANWAY_CODEX_HOST", &host.path).output()
            .map_err(|_| "无法请求桌面宿主退出。")?;
        if !output.status.success() || windows_host().is_some_and(|h| h.running) {
            return Err("桌面宿主未退出；不会强制关闭任务，请手动重启。".into());
        }
    }
    if let Some(aumid) = &host.aumid {
        Command::new("explorer.exe").arg(format!("shell:AppsFolder\\{aumid}")).spawn()
            .map_err(|_| "无法启动已注册的桌面应用，请手动打开。")?;
    } else {
        Command::new(&host.path).spawn().map_err(|_| "配置已保留，但启动桌面宿主失败，请手动打开。")?;
    }
    let mut started = false;
    for _ in 0..8 {
        if windows_host().is_some_and(|next| next.path == host.path && next.running) {
            started = true;
            break;
        }
        thread::sleep(Duration::from_millis(300));
    }
    if !started { return Err("已请求启动，但未确认桌面宿主运行，请手动检查。".into()); }
    Ok(RestartCodexResult {
        restarted: true,
        was_running: host.running,
        message: "已打开 Codex 桌面宿主。请新建任务确认规则生效。".into(),
    })
}

#[tauri::command]
pub(super) async fn pick_reference_images() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(pick_files).await
        .map_err(|_| "参考图选择任务异常。".to_string())?
}

fn pick_files() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new()
$dialog=New-Object System.Windows.Forms.OpenFileDialog
$dialog.Title='选择参考图'
$dialog.Filter='Images (*.png;*.jpg;*.jpeg;*.webp)|*.png;*.jpg;*.jpeg;*.webp'
$dialog.Multiselect=$true
$dialog.CheckFileExists=$true
if($dialog.ShowDialog() -eq 'OK'){ConvertTo-Json -InputObject @($dialog.FileNames) -Compress}else{'[]'}
$dialog.Dispose()
"#;
        let output = Command::new("powershell").args(["-NoProfile", "-STA", "-Command", script])
            .output().map_err(|_| "无法打开系统图片选择器。".to_string())?;
        if !output.status.success() { return Err("系统图片选择器失败。".into()); }
        return serde_json::from_slice(&output.stdout).map_err(|_| "无法读取选择的文件路径。".into());
    }
    #[cfg(target_os = "macos")]
    {
        let script = r#"
try
    set chosen to choose file with prompt "选择参考图" of type {"public.png", "public.jpeg", "org.webmproject.webp"} with multiple selections allowed
    set resultText to ""
    repeat with f in chosen
        set resultText to resultText & POSIX path of f & linefeed
    end repeat
    return resultText
on error number -128
    return ""
end try
"#;
        let output = Command::new("osascript").args(["-e", script]).output()
            .map_err(|_| "无法打开系统图片选择器。".to_string())?;
        if !output.status.success() { return Err("系统图片选择器失败。".into()); }
        return Ok(String::from_utf8_lossy(&output.stdout).lines()
            .filter(|s| !s.is_empty()).map(str::to_string).collect());
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    Err("当前平台尚未实现参考图选择器。".into())
}
