//! In-app, progress-reporting installation from Oceanway's verified offline mirror.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::Write, path::Path, process::Command, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, time::Duration};

const BASE: &str = "https://api.ocean-way.top/downloads/agents/";
const APPS: &[&str] = &["codex", "claude-desktop", "claude", "dsh", "hermes", "kimi"];
#[derive(Clone, Deserialize)]
struct Package {
    app: String, platform: String, architecture: String, filename: String,
    sha256: String, size: u64,
    #[serde(default)] install: String,
    #[serde(default, rename="minimumWindowsBuild")] minimum_windows_build: u32,
}
#[derive(Deserialize)]
struct Manifest { packages: Vec<Package> }
#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct InstallStatus {
    id: String, app_id: String, stage: String, downloaded: u64, total: u64, message: String,
}
struct Job { status: InstallStatus, cancel: Arc<AtomicBool> }
static JOBS: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();
static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn jobs() -> &'static Mutex<HashMap<String, Job>> { JOBS.get_or_init(Default::default) }
fn terminal(stage: &str) -> bool { matches!(stage,"installed"|"failed"|"cancelled") }
fn update(id:&str, stage:&str, downloaded:u64, total:u64, message:&str) {
    if let Ok(mut jobs)=jobs().lock() { if let Some(job)=jobs.get_mut(id) {
        job.status.stage=stage.into(); job.status.downloaded=downloaded; job.status.total=total; job.status.message=message.into();
    }}
}
fn validate_package(p:&Package) -> Result<(),String> {
    if !APPS.contains(&p.app.as_str()) || p.filename.is_empty() || !p.filename.bytes().all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
        || p.filename.contains("..") || p.sha256.len()!=64 || !p.sha256.bytes().all(|c|c.is_ascii_hexdigit()) || p.size==0 || p.size>4*1024*1024*1024 {
        return Err("安装包信息无效，请稍后重试".into());
    }
    Ok(())
}
fn select_package(manifest:Manifest, app:&str, platform:&str, arch:&str) -> Result<Package,String> {
    let arch=match arch {"x86_64"=>"x64","aarch64"=>"arm64",other=>other};
    let package=manifest.packages.into_iter().find(|p|p.app==app && p.platform==platform && (p.architecture==arch || p.architecture=="universal"))
        .ok_or_else(||"当前系统的完整离线包尚未就绪，请稍后重试。".to_string())?;
    validate_package(&package)?; Ok(package)
}

#[tauri::command]
pub async fn oceanway_start_install(app_id:String) -> Result<InstallStatus,String> {
    if !APPS.contains(&app_id.as_str()) {return Err("不支持的应用".into());}
    let mut guard=jobs().lock().map_err(|_|"安装状态不可用")?;
    if let Some(job)=guard.values().find(|j|!terminal(&j.status.stage)) {return Ok(job.status.clone());}
    guard.retain(|_,j|!terminal(&j.status.stage));
    let id=uuid::Uuid::new_v4().to_string();
    let status=InstallStatus{id:id.clone(),app_id:app_id.clone(),stage:"preparing".into(),downloaded:0,total:0,message:"正在检查系统和安装包…".into()};
    let cancel=Arc::new(AtomicBool::new(false));
    guard.insert(id.clone(),Job{status:status.clone(),cancel:cancel.clone()});
    tauri::async_runtime::spawn(async move {
        let _lock=INSTALL_LOCK.lock().await;
        if let Err(e)=install(&id,&app_id,cancel.clone()).await {
            update(&id,if cancel.load(Ordering::Relaxed){"cancelled"}else{"failed"},0,0,&e);
        }
    });
    Ok(status)
}
#[tauri::command]
pub fn oceanway_get_install(id:String) -> Result<InstallStatus,String> {
    jobs().lock().map_err(|_|"安装状态不可用")?.get(&id).map(|j|j.status.clone()).ok_or("安装任务不存在，请重新检测".into())
}
#[tauri::command]
pub fn oceanway_cancel_install(id:String) -> Result<(),String> {
    let guard=jobs().lock().map_err(|_|"安装状态不可用")?;
    let job=guard.get(&id).ok_or("安装任务不存在")?;
    if matches!(job.status.stage.as_str(),"installing"|"checking") {return Err("系统正在安装，请等待完成".into());}
    job.cancel.store(true,Ordering::Relaxed); Ok(())
}
fn check_cancel(cancel:&AtomicBool)->Result<(),String> {
    if cancel.load(Ordering::Relaxed){Err("下载已取消，可以重新安装".into())}else{Ok(())}
}
fn begin_install(id:&str, cancel:&AtomicBool, count:u64)->Result<(),String> {
    let mut guard=jobs().lock().map_err(|_|"安装状态不可用")?;
    check_cancel(cancel)?;
    if let Some(job)=guard.get_mut(id) {
        job.status.stage="installing".into();
        job.status.downloaded=count;
        job.status.total=count;
        job.status.message="正在默认安装；如出现系统权限提示，请选择允许…".into();
    }
    Ok(())
}
fn check_system(package:&Package)->Result<(),String> {
    #[cfg(target_os="windows")]
    if package.minimum_windows_build>0 {
        use winreg::{RegKey,enums::*};
        let key=RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion").map_err(|e|e.to_string())?;
        let build:String=key.get_value("CurrentBuildNumber").map_err(|e|e.to_string())?;
        if build.parse::<u32>().unwrap_or(0)<package.minimum_windows_build {
            return Err(format!("此应用需要 Windows 系统版本 {} 或更新版本，请先更新系统",package.minimum_windows_build));
        }
    }
    let _=package;
    Ok(())
}
async fn installed(app:&str)->Result<bool,String> {
    let app=app.to_string();
    if app=="claude" {
        let versions=crate::commands::get_tool_versions(Some(vec!["claude".into()]),None).await?;
        let rows=serde_json::to_value(versions).map_err(|e|e.to_string())?;
        return Ok(rows.as_array().is_some_and(|a|a.iter().any(|v|v["name"]=="claude" && v["version"].is_string())));
    }
    tauri::async_runtime::spawn_blocking(move || {
        if app=="codex" {Ok(crate::codex_desktop_detection::is_installed())}
        else {crate::agent_desktops::find(&app).map(|p|p.is_some())}
    }).await.map_err(|e|e.to_string())?
}
async fn install(id:&str, app:&str, cancel:Arc<AtomicBool>)->Result<(),String> {
    if installed(app).await? {update(id,"installed",0,0,"应用已安装，可以继续配置");return Ok(());}
    let client=reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(1800))
        .redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())?;
    let response=tokio::time::timeout(Duration::from_secs(25),client.get(format!("{BASE}manifest.json")).send()).await
        .map_err(|_|"下载服务响应超时，请重试")?.map_err(|_|"无法连接 Oceanway 下载服务，请检查网络")?.error_for_status().map_err(|e|format!("无法读取安装包信息：{e}"))?;
    if response.content_length().unwrap_or(0)>256*1024 {return Err("安装清单异常".into());}
    let bytes=tokio::time::timeout(Duration::from_secs(25),response.bytes()).await.map_err(|_|"安装清单读取超时")?.map_err(|e|e.to_string())?;
    if bytes.len()>256*1024 {return Err("安装清单异常".into());}
    let manifest=serde_json::from_slice(&bytes).map_err(|_|"安装清单格式无效")?;
    let package=select_package(manifest,app,std::env::consts::OS,std::env::consts::ARCH)?;
    check_system(&package)?;
    check_cancel(&cancel)?;
    let temp=tempfile::Builder::new().prefix("oceanway-agent-install-").tempdir().map_err(|e|e.to_string())?;
    let path=temp.path().join(&package.filename);
    let mut file=std::fs::File::create(&path).map_err(|e|format!("无法创建下载文件：{e}"))?;
    let mut response=client.get(format!("{BASE}{}",package.filename)).send().await.map_err(|_|"安装包下载连接失败，请重试")?
        .error_for_status().map_err(|e|format!("安装包下载失败：{e}"))?;
    let mut count=0; let mut digest=Sha256::new();
    update(id,"downloading",0,package.size,"正在从 Oceanway 下载完整安装包…");
    loop {
        check_cancel(&cancel)?;
        let chunk=tokio::time::timeout(Duration::from_secs(30),response.chunk()).await.map_err(|_|"下载已超过 30 秒没有响应，请重试")?.map_err(|_|"下载中断，请检查网络后重试")?;
        let Some(chunk)=chunk else {break};
        count+=chunk.len() as u64;
        if count>package.size {return Err("安装包大小异常，请重新下载".into());}
        file.write_all(&chunk).map_err(|e|format!("无法保存安装包，请检查磁盘空间：{e}"))?;
        digest.update(&chunk); update(id,"downloading",count,package.size,"正在下载完整安装包…");
    }
    file.sync_all().map_err(|e|e.to_string())?; drop(file);
    update(id,"verifying",count,package.size,"正在校验安装包…");
    if count!=package.size || format!("{:x}",digest.finalize())!=package.sha256.to_ascii_lowercase(){return Err("安装包校验失败，文件未执行，请重试".into());}
    begin_install(id,&cancel,count)?;
    tauri::async_runtime::spawn_blocking(move || {
        let result=install_local(&package,&path);
        // A timed-out system permission prompt may still own the installer.
        // Keep its source until the OS releases it rather than removing a live file.
        if result.as_ref().is_err_and(|e|e.contains("操作超时")) { let _=temp.keep(); }
        result
    }).await.map_err(|e|e.to_string())??;
    update(id,"checking",count,count,"正在确认安装结果…");
    for _ in 0..10 {
        if installed(app).await? {update(id,"installed",count,count,"安装完成，可以继续配置 Oceanway");return Ok(());}
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Err("安装程序已结束，但尚未检测到应用。请查看系统安装提示，然后点击重新检测。".into())
}

fn install_local(package:&Package,path:&Path)->Result<(),String> {
    if package.app=="claude" && package.install=="native-binary" {return install_claude(path);}
    #[cfg(target_os="windows")]
    {
        if package.filename.ends_with(".msix") {
            let script=r#"$ErrorActionPreference='Stop'; try { $signature=Get-AuthenticodeSignature -LiteralPath $env:OCEANWAY_INSTALL_FILE; if($signature.Status -ne 'Valid'){throw '官方安装包签名校验失败'}; Add-AppxPackage -Path $env:OCEANWAY_INSTALL_FILE -ErrorAction Stop } catch {[Console]::Error.WriteLine($_.Exception.Message); exit 1}"#;
            let result=crate::agent_desktops::bounded_output(Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",script]).env("OCEANWAY_INSTALL_FILE",path),1200)?;
            return if result.status.success(){Ok(())}else{Err(format!("安装未完成：{}",String::from_utf8_lossy(&result.stderr).trim()))};
        }
        if !package.filename.ends_with(".exe") {return Err("Windows 安装包类型不正确".into());}
        // UAC stays visible; the downloader itself never needs administrator access.
        const SCRIPT:&str=r#"
$ErrorActionPreference='Stop'
try {
  $file=$env:OCEANWAY_INSTALL_FILE.Replace("'","''")
  $worker=@'
$ErrorActionPreference='Stop'
try {
  $p=Start-Process -FilePath '__FILE__' -ArgumentList '/S' -PassThru -WindowStyle Hidden
  if(-not $p.WaitForExit(900000)) {
    & "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F | Out-Null
    $p.WaitForExit()
    throw '安装超过 15 分钟，已停止本次安装，请重新检测后重试。'
  }
  if($p.ExitCode -ne 0) {throw "安装程序退出码：$($p.ExitCode)"}
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
'@
  $worker=$worker.Replace('__FILE__',$file)
  if($env:OCEANWAY_INSTALL_ADMIN -eq '1') {
    $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($worker))
    $p=Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden -PassThru -Wait -ArgumentList @('-NoProfile','-NonInteractive','-EncodedCommand',$encoded)
    if($p.ExitCode -ne 0) {throw "系统安装未完成（退出码 $($p.ExitCode)），请重新检测后重试。"}
  } else { & ([ScriptBlock]::Create($worker)); exit $LASTEXITCODE }
} catch {[Console]::Error.WriteLine($_.Exception.Message); exit 1}
"#;
        let result=crate::agent_desktops::bounded_output(Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",SCRIPT])
            .env("OCEANWAY_INSTALL_FILE",path).env("OCEANWAY_INSTALL_ADMIN",if matches!(package.app.as_str(),"codex"|"claude-desktop"|"hermes"){"1"}else{"0"}),1200)?;
        if !result.status.success(){return Err(format!("安装未完成：{}",String::from_utf8_lossy(&result.stderr).trim()));}
        return Ok(());
    }
    #[cfg(target_os="macos")]
    { return install_mac(package,path); }
    #[allow(unreachable_code)] Err("当前系统尚无自动安装支持".into())
}
fn install_claude(path:&Path)->Result<(),String> {
    let bin=crate::config::get_home_dir().join(".local/bin");
    std::fs::create_dir_all(&bin).map_err(|e|e.to_string())?;
    let dest=bin.join(if cfg!(windows){"claude.exe"}else{"claude"});
    if dest.exists() {return Err("已有 Claude Code 文件，请先重新检测现有安装".into());}
    let mut stage=tempfile::Builder::new().prefix(".oceanway-claude-").suffix(if cfg!(windows){".exe"}else{""}).tempfile_in(&bin).map_err(|e|e.to_string())?;
    let mut source=std::fs::File::open(path).map_err(|e|e.to_string())?;
    std::io::copy(&mut source,&mut stage).map_err(|e|e.to_string())?;
    stage.as_file().sync_all().map_err(|e|e.to_string())?;
    let staged=stage.into_temp_path();
    #[cfg(unix)]
    {use std::os::unix::fs::PermissionsExt;std::fs::set_permissions(&staged,std::fs::Permissions::from_mode(0o755)).map_err(|e|e.to_string())?;}
    let result=crate::agent_desktops::bounded_output(Command::new(&staged).arg("--version"),30)?;
    if !result.status.success(){return Err("Claude Code 文件已保存，但启动检查失败".into());}
    staged.persist_noclobber(&dest).map_err(|e|e.to_string())?;
    #[cfg(target_os="windows")]
    {
        use winreg::{RegKey,enums::*};
        let env=RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags("Environment",KEY_READ|KEY_WRITE).map_err(|e|e.to_string())?;
        let old:String=env.get_value("Path").unwrap_or_default();
        let new=path_with_bin(&old,&bin.to_string_lossy(),';');
        env.set_value("Path",&new).map_err(|e|e.to_string())?;
        // Notify future terminals about the new per-user PATH without changing other entries.
        let _=crate::agent_desktops::bounded_output(Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",
          "$s='[DllImport(\"user32.dll\", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, UIntPtr w, string l, uint f, uint t, out UIntPtr r);'; $t=Add-Type -MemberDefinition $s -Name EnvNotify -Namespace Oceanway -PassThru; $r=[UIntPtr]::Zero; $null=$t::SendMessageTimeout([IntPtr]0xffff,0x1A,[UIntPtr]::Zero,'Environment',2,3000,[ref]$r)"]),5);
    }
    #[cfg(target_os="macos")]
    {
        // zsh is the default macOS login shell. Keep any existing profile intact.
        let shell=std::env::var("SHELL").unwrap_or_default();
        let profile=crate::config::get_home_dir().join(if shell.ends_with("/bash"){".bash_profile"}else{".zprofile"});
        let content=match std::fs::read_to_string(&profile){Ok(v)=>v,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>String::new(),Err(e)=>return Err(e.to_string())};
        let rule="\n# Oceanway Claude Code\nexport PATH=\"$HOME/.local/bin:$PATH\"\n";
        if !content.contains("# Oceanway Claude Code") {std::fs::write(&profile,format!("{content}{rule}")).map_err(|e|e.to_string())?;}
    }
    Ok(())
}
fn path_with_bin(old:&str,bin:&str,separator:char)->String {
    if old.split(separator).any(|p|p.trim().eq_ignore_ascii_case(bin)){old.into()}
    else if old.is_empty(){bin.into()}else{format!("{old}{separator}{bin}")}
}

#[cfg(target_os="macos")]
fn mac_admin_command(command:&str)->Result<std::process::Output,String> {
    #[cfg(test)]
    if std::env::var("GITHUB_ACTIONS").as_deref()==Ok("true") {
        // Disposable runners have passwordless sudo but no interactive permission UI.
        return crate::agent_desktops::bounded_output(Command::new("sudo").args(["-n","/bin/sh","-c",command]),1200);
    }
    let script="on run argv\ndo shell script (item 1 of argv) with administrator privileges\nend run";
    crate::agent_desktops::bounded_output(Command::new("osascript").args(["-e",script,command]),1200)
}
#[cfg(target_os="macos")]
fn shell_quote(value:&Path)->String {format!("'{}'",value.to_string_lossy().replace('\'',"'\\''"))}
#[cfg(target_os="macos")]
fn install_mac(package:&Package,path:&Path)->Result<(),String> {
    if package.filename.ends_with(".pkg") {
        let signed=crate::agent_desktops::bounded_output(Command::new("pkgutil").arg("--check-signature").arg(path),60)?;
        if !signed.status.success(){return Err("Mac 安装包签名校验失败".into());}
        let result=mac_admin_command(&format!("/usr/sbin/installer -pkg {} -target /",shell_quote(path)))?;
        return if result.status.success(){Ok(())}else{Err(String::from_utf8_lossy(&result.stderr).trim().into())};
    }
    let mount=tempfile::Builder::new().prefix("oceanway-mount-").tempdir().map_err(|e|e.to_string())?;
    let result=crate::agent_desktops::bounded_output(Command::new("hdiutil").args(["attach","-readonly","-nobrowse","-mountpoint"]).arg(mount.path()).arg(path),120)?;
    if !result.status.success(){return Err("无法打开 Mac 安装包，请重试".into());}
    let outcome=(|| {
        let apps=std::fs::read_dir(mount.path()).map_err(|e|e.to_string())?.filter_map(Result::ok).map(|e|e.path()).filter(|p|p.extension().is_some_and(|e|e=="app")).collect::<Vec<_>>();
        if apps.len()!=1{return Err("安装包中的应用数量异常".into());}
        let app=&apps[0];
        let signed=crate::agent_desktops::bounded_output(Command::new("codesign").args(["--verify","--deep","--strict"]).arg(app),90)?;
        if !signed.status.success(){return Err("Mac 应用签名校验失败".into());}
        let target=std::path::PathBuf::from("/Applications").join(app.file_name().ok_or("无效应用名称")?);
        if target.exists(){return Err("应用目录已存在，请重新检测已有安装".into());}
        // Use the standard Applications destination, with the normal macOS permission prompt.
        let script=format!("set -eu\nstage=$(/usr/bin/mktemp -d /Applications/.oceanway-install.XXXXXX)\ntrap '/bin/rm -rf \"$stage\"' EXIT\n/usr/bin/ditto {} \"$stage/application.app\"\n/usr/bin/codesign --verify --deep --strict \"$stage/application.app\"\ntest ! -e {}\n/bin/mv -n \"$stage/application.app\" {}",shell_quote(app),shell_quote(&target),shell_quote(&target));
        let result=mac_admin_command(&script)?;
        if !result.status.success(){return Err(format!("应用安装未完成：{}",String::from_utf8_lossy(&result.stderr).trim()));}
        Ok(())
    })();
    let detach=crate::agent_desktops::bounded_output(Command::new("hdiutil").arg("detach").arg(mount.path()),45);
    if detach.as_ref().is_ok_and(|o|o.status.success()){drop(mount);}else{let _=mount.keep();}
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Installs real software; run only on a disposable GitHub runner"]
    async fn install_on_disposable_runner() {
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"), "Never install test agents in a user's current environment");
        let app=std::env::var("OCEANWAY_INSTALL_TEST_TARGET").expect("explicit agent target");
        assert!(APPS.contains(&app.as_str()));
        assert!(!installed(&app).await.unwrap(), "runner must not already have this agent installed");
        let result=install("isolated-runner",&app,Arc::new(AtomicBool::new(false))).await;
        assert!(result.is_ok(), "default installation failed: {result:?}");
        assert!(installed(&app).await.unwrap());
        println!("Verified default installation and native detection: {app}");
    }
    fn package(arch:&str)->Package {Package{app:"codex".into(),platform:"macos".into(),architecture:arch.into(),filename:"codex.dmg".into(),sha256:"a".repeat(64),size:1024,install:String::new(),minimum_windows_build:0}}
    #[test] fn cancellation_prevents_starting_the_installer() {
        assert!(begin_install("cancelled-test",&AtomicBool::new(true),1024).is_err());
    }
    #[test] fn selects_actual_platform_and_architecture() {
        let m=||Manifest{packages:vec![package("arm64"),package("x64")]};
        assert_eq!(select_package(m(),"codex","macos","x86_64").unwrap().architecture,"x64");
        assert!(select_package(m(),"codex","windows","x86_64").is_err());
        assert!(select_package(m(),"hermes","macos","aarch64").is_err());
    }
    #[test] fn rejects_unsafe_or_unverifiable_downloads() {
        for filename in ["../setup.exe","https://other/setup.exe","setup.exe?x=1",""] {let mut p=package("x64");p.filename=filename.into();assert!(validate_package(&p).is_err());}
        let mut p=package("x64");p.sha256="unknown".into();assert!(validate_package(&p).is_err());
    }
    #[test] fn path_update_preserves_existing_entries_and_is_idempotent() {
        assert_eq!(path_with_bin("C:\\Windows;C:\\tools","C:\\Users\\me\\.local\\bin",';'),"C:\\Windows;C:\\tools;C:\\Users\\me\\.local\\bin");
        assert_eq!(path_with_bin("C:\\bin","c:\\bin",';'),"C:\\bin");
    }
}
