//! Installed GUI discovery, independent of CLI availability and saved user data.
use std::{path::PathBuf, process::{Command, Stdio}, time::{Duration, Instant}};

pub(crate) fn bounded_output(command: &mut Command, seconds: u64) -> Result<std::process::Output, String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    fn drain(pipe: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || { let mut data=Vec::new(); if let Some(mut pipe)=pipe {let _=pipe.read_to_end(&mut data);} data })
    }
    let stdout=drain(child.stdout.take()); let stderr=drain(child.stderr.take());
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(std::process::Output {status,stdout:stdout.join().unwrap_or_default(),stderr:stderr.join().unwrap_or_default()}),
            Ok(None) if start.elapsed() < Duration::from_secs(seconds) => std::thread::sleep(Duration::from_millis(50)),
            _ => { let _ = child.kill(); let _ = child.wait(); return Err("应用操作超时，请重试".into()); }
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn find(app: &str) -> Result<Option<PathBuf>, String> {
    let names = match app {
        "claude-desktop" => "Claude",
        "dsh" => "DeepSeek Harness",
        "kimi" => "Kimi Code",
        "hermes" => "Hermes Agent",
        _ => return Ok(None),
    };
    const SCRIPT: &str = r#"
$ErrorActionPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$name=$env:OCEANWAY_DESKTOP_NAME
$candidates=@()
foreach($root in @($env:LOCALAPPDATA,(Join-Path $env:LOCALAPPDATA 'Programs'),$env:ProgramFiles)) {
  foreach($folder in @($name,$name.Replace(' ',''),$name.ToLower().Replace(' ','-'),'kimi-code-app')) {
    foreach($exe in @("$name.exe",$name.Replace(' ','')+'.exe','kimi-code-app.exe')) {
      $candidates+=Join-Path (Join-Path $root $folder) $exe
    }
  }
}
$keys=@('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*')
foreach($entry in (Get-ItemProperty $keys | Where-Object { $_.DisplayName -eq $name -or $_.DisplayName -like "$name *" })) {
  if($entry.InstallLocation) { Get-ChildItem -LiteralPath $entry.InstallLocation -Filter '*.exe' | Where-Object { $_.BaseName -eq $name -or $_.BaseName -eq $name.Replace(' ','') -or $_.BaseName -eq 'kimi-code-app' } | ForEach-Object { $candidates+=$_.FullName } }
}
foreach($pkg in (Get-AppxPackage | Where-Object { $_.Name -like "*$($name.Replace(' ',''))*" -or ($name -eq 'Hermes Agent' -and $_.Name -eq 'NousResearch.HermesBundled') })) {
  foreach($relative in @("app\$name.exe","$name.exe")) { $candidates+=Join-Path $pkg.InstallLocation $relative }
}
Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and ($_.ProcessName -eq $name -or $_.ProcessName -eq $name.Replace(' ','')) } | ForEach-Object { $candidates+=$_.Path }
$candidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) -and (Test-Path -LiteralPath (Join-Path (Split-Path -Parent $_) 'resources\app.asar') -PathType Leaf) } | Select-Object -Unique -First 1
"#;
    let output=bounded_output(Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",SCRIPT]).env("OCEANWAY_DESKTOP_NAME",names), 12)?;
    if !output.status.success() { return Err("桌面应用检测失败，请重新检测".into()); }
    Ok(String::from_utf8_lossy(&output.stdout).lines().map(str::trim).find(|line| !line.is_empty()).map(PathBuf::from))
}

#[cfg(target_os = "macos")]
pub(crate) fn find(app: &str) -> Result<Option<PathBuf>, String> {
    let names: &[&str]=match app {
        "claude-desktop" => &["Claude"], "dsh" => &["DeepSeek Harness"],
        "kimi" => &["Kimi Code","KimiCode"], "hermes" => &["Hermes Agent","Hermes"], _=>return Ok(None),
    };
    for root in [PathBuf::from("/Applications"), crate::config::get_home_dir().join("Applications")] {
        for name in names { let path=root.join(format!("{name}.app"));
            if path.join("Contents/Info.plist").is_file() && path.join("Contents/MacOS").is_dir() {return Ok(Some(path));}
        }
    }
    Ok(None)
}
#[cfg(not(any(target_os="windows",target_os="macos")))]
pub(crate) fn find(_app: &str) -> Result<Option<PathBuf>, String> { Ok(None) }

pub(crate) fn launch(app: &str, restart: bool) -> Result<String, String> {
    let path=find(app)?.ok_or("尚未找到桌面应用，请下载安装后重新检测")?;
    #[cfg(target_os="windows")]
    {
        const SCRIPT:&str=r#"
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
try {
  $path=$env:OCEANWAY_DESKTOP_PATH
  if($env:OCEANWAY_DESKTOP_RESTART -eq '1') {
    $running=@(Get-Process | Where-Object {$_.Path -eq $path -and $_.MainWindowHandle -ne 0})
    foreach($p in $running) { if(-not $p.CloseMainWindow() -or -not $p.WaitForExit(10000)) {throw '应用尚未退出，请保存任务并关闭后重试。'} }
  }
  Start-Process -FilePath $path | Out-Null
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
"#;
        let result=bounded_output(Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",SCRIPT]).env("OCEANWAY_DESKTOP_PATH",path).env("OCEANWAY_DESKTOP_RESTART",if restart {"1"} else {"0"}),25)?;
        if !result.status.success() {return Err(String::from_utf8_lossy(&result.stderr).trim().into());}
    }
    #[cfg(target_os="macos")]
    {
        if restart {
            // Pass the bundle path as argv, not interpolated AppleScript source.
            let script="on run argv\ntell application (item 1 of argv) to quit\nend run";
            let result=bounded_output(Command::new("osascript").args(["-e",script]).arg(&path),15)?;
            if !result.status.success() {return Err("应用没有接受退出请求，请保存任务后重试".into());}
        }
        let output=bounded_output(Command::new("open").arg(&path),8)?;
        if !output.status.success() {return Err("无法打开桌面应用，请检查安装是否完成".into());}
    }
    #[cfg(not(any(target_os="windows",target_os="macos")))]
    { let _=(path,restart); return Err("当前系统尚无桌面启动支持".into()); }
    #[allow(unreachable_code)]
    Ok(if restart {"已重新打开应用，请开始新请求加载配置"} else {"已请求打开桌面应用"}.into())
}
