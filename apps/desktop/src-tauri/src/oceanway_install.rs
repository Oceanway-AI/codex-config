//! Download verified offline packages, then let the user open the system installer.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::{Read, Write}, path::{Path, PathBuf}, process::Command, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, time::Duration};

const BASE: &str = "https://api.ocean-way.top/downloads/agents/";
const APPS: &[&str] = &["codex", "claude-desktop", "claude", "dsh", "hermes", "kimi"];
#[derive(Clone, Deserialize, Serialize)]
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
    action_error: Option<String>,
}
struct Job { status: InstallStatus, cancel: Arc<AtomicBool>, package: Option<(Package, PathBuf)> }
static JOBS: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();
static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn jobs() -> &'static Mutex<HashMap<String, Job>> { JOBS.get_or_init(Default::default) }
fn busy(stage: &str) -> bool { matches!(stage,"preparing"|"downloading"|"verifying"|"opening"|"installing"|"checking") }
fn update(id:&str, stage:&str, downloaded:u64, total:u64, message:&str) {
    if let Ok(mut jobs)=jobs().lock() { if let Some(job)=jobs.get_mut(id) {
        job.status.stage=stage.into(); job.status.downloaded=downloaded; job.status.total=total; job.status.message=message.into(); job.status.action_error=None;
    }}
}
fn validate_package(p:&Package) -> Result<(),String> {
    if !APPS.contains(&p.app.as_str()) || p.filename.is_empty() || !p.filename.bytes().all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
        || p.filename.contains("..") || p.sha256.len()!=64 || !p.sha256.bytes().all(|c|c.is_ascii_hexdigit()) || p.size==0 || p.size>4*1024*1024*1024 {
        return Err("安装包信息无效，请稍后重试".into());
    }
    if p.app=="claude" {
        if p.install!="native-binary" {return Err("Claude Code 安装包类型不正确".into());}
    } else {
        let valid=match p.platform.as_str() {
            "windows" => ["exe","msix","msixbundle"].contains(&Path::new(&p.filename).extension().and_then(|e|e.to_str()).unwrap_or("")),
            "macos" => ["dmg","pkg"].contains(&Path::new(&p.filename).extension().and_then(|e|e.to_str()).unwrap_or("")),
            _ => false,
        };
        if !valid {return Err("当前系统的安装包类型不正确".into());}
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
    if let Some(job)=guard.values().find(|j|busy(&j.status.stage)) {return Ok(job.status.clone());}
    if let Some(job)=guard.values().find(|j|j.status.app_id==app_id && matches!(j.status.stage.as_str(),"ready"|"awaiting_install")) {return Ok(job.status.clone());}
    guard.retain(|_,j|j.status.app_id!=app_id);
    let id=uuid::Uuid::new_v4().to_string();
    let status=InstallStatus{id:id.clone(),app_id:app_id.clone(),stage:"preparing".into(),downloaded:0,total:0,message:"正在检查系统和安装包…".into(),action_error:None};
    let cancel=Arc::new(AtomicBool::new(false));
    guard.insert(id.clone(),Job{status:status.clone(),cancel:cancel.clone(),package:None});
    tauri::async_runtime::spawn(async move {
        let _lock=INSTALL_LOCK.lock().await;
        if let Err(e)=download(&id,&app_id,cancel.clone()).await {
            update(&id,if cancel.load(Ordering::Relaxed){"cancelled"}else{"failed"},0,0,&e);
        }
    });
    Ok(status)
}
#[tauri::command]
pub fn oceanway_get_install(id:String) -> Result<InstallStatus,String> {
    jobs().lock().map_err(|_|"安装状态不可用")?.get(&id).map(|j|j.status.clone()).ok_or("安装任务不存在，请重新下载".into())
}
#[tauri::command]
pub fn oceanway_cancel_install(id:String) -> Result<(),String> {
    let guard=jobs().lock().map_err(|_|"安装状态不可用")?;
    let job=guard.get(&id).ok_or("安装任务不存在")?;
    if !matches!(job.status.stage.as_str(),"preparing"|"downloading"|"verifying") {return Err("当前没有可取消的下载".into());}
    job.cancel.store(true,Ordering::Relaxed); Ok(())
}
fn check_cancel(cancel:&AtomicBool)->Result<(),String> {
    if cancel.load(Ordering::Relaxed){Err("下载已取消，可以重新下载".into())}else{Ok(())}
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
fn cache_root()->PathBuf {crate::config::get_app_config_dir().join("downloads")}
fn package_path(root:&Path,p:&Package)->PathBuf {root.join(&p.app).join(&p.sha256).join(&p.filename)}
fn verify_file(package:&Package,path:&Path)->Result<(),String> {
    let mut file=std::fs::File::open(path).map_err(|_|"安装包已被移动或删除，请重新下载")?;
    if file.metadata().map_err(|e|e.to_string())?.len()!=package.size {return Err("安装包大小不符，请重新下载".into());}
    let mut digest=Sha256::new();
    let mut buf=[0u8;65536];
    loop {let n=file.read(&mut buf).map_err(|e|e.to_string())?;if n==0 {break;}digest.update(&buf[..n]);}
    if format!("{:x}",digest.finalize())!=package.sha256.to_ascii_lowercase() {return Err("安装包校验失败，请重新下载".into());}
    Ok(())
}
fn cached_package(root:&Path,app:&str)->Option<(Package,PathBuf)> {
    let metadata=root.join(app).join("package.json");
    if std::fs::metadata(&metadata).ok()?.len()>256*1024 {return None;}
    let package:Package=serde_json::from_slice(&std::fs::read(&metadata).ok()?).ok()?;
    let package=select_package(Manifest{packages:vec![package]},app,std::env::consts::OS,std::env::consts::ARCH).ok()?;
    let path=package_path(root,&package);
    verify_file(&package,&path).ok()?;
    Some((package,path))
}
fn mark_ready(id:&str,package:Package,path:PathBuf,cancel:&AtomicBool)->Result<(),String> {
    let mut guard=jobs().lock().map_err(|_|"安装状态不可用")?;
    check_cancel(cancel)?;
    let job=guard.get_mut(id).ok_or("安装任务不存在")?;
    job.status.stage="ready".into();job.status.downloaded=package.size;job.status.total=package.size;
    job.status.message=if package.app=="claude" {"下载完成，点击“安装 Claude Code”继续。"}else{"下载完成，点击“打开安装包”，按提示安装。"}.into();
    job.package=Some((package,path));Ok(())
}
async fn download(id:&str, app:&str, cancel:Arc<AtomicBool>)->Result<(),String> {
    if installed(app).await? {update(id,"installed",0,0,"应用已安装，可以继续配置");return Ok(());}
    check_cancel(&cancel)?;
    let root=cache_root();
    let cached_root=root.clone();let cached_app=app.to_string();
    if let Some((package,path))=tauri::async_runtime::spawn_blocking(move || cached_package(&cached_root,&cached_app)).await.map_err(|e|e.to_string())? {
        check_system(&package)?;
        return mark_ready(id,package,path,&cancel);
    }
    let client=reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(1800))
        .redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())?;
    let response=tokio::time::timeout(Duration::from_secs(25),client.get(format!("{BASE}manifest.json")).send()).await
        .map_err(|_|"下载服务响应超时，请重试")?.map_err(|_|"无法连接 Oceanway 下载服务，请检查网络后重试")?.error_for_status().map_err(|e|format!("无法读取安装包信息：{e}"))?;
    if response.content_length().unwrap_or(0)>256*1024 {return Err("安装清单异常".into());}
    let bytes=tokio::time::timeout(Duration::from_secs(25),response.bytes()).await.map_err(|_|"安装清单读取超时")?.map_err(|e|e.to_string())?;
    if bytes.len()>256*1024 {return Err("安装清单异常".into());}
    let manifest=serde_json::from_slice(&bytes).map_err(|_|"安装清单格式无效")?;
    let package=select_package(manifest,app,std::env::consts::OS,std::env::consts::ARCH)?;
    check_system(&package)?;
    check_cancel(&cancel)?;
    let path=package_path(&root,&package);
    let parent=path.parent().ok_or("安装包目录无效")?;
    std::fs::create_dir_all(parent).map_err(|e|format!("无法创建下载目录：{e}"))?;
    // Partial/cancelled downloads disappear; only verified files are persisted.
    let mut file=tempfile::NamedTempFile::new_in(parent).map_err(|e|e.to_string())?;
    update(id,"downloading",0,package.size,"正在连接 Oceanway 下载服务…");
    let mut response=tokio::time::timeout(Duration::from_secs(25),client.get(format!("{BASE}{}",package.filename)).send()).await
        .map_err(|_|"安装包下载响应超时，请重试")?.map_err(|_|"安装包下载连接失败，请重试")?
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
    file.as_file().sync_all().map_err(|e|e.to_string())?;
    update(id,"verifying",count,package.size,"正在校验安装包…");
    if count!=package.size || format!("{:x}",digest.finalize())!=package.sha256.to_ascii_lowercase(){return Err("安装包校验失败，请重试".into());}
    check_cancel(&cancel)?;
    file.persist(&path).map_err(|e|e.to_string())?;
    let metadata=root.join(app).join("package.json");
    let mut record=tempfile::NamedTempFile::new_in(root.join(app)).map_err(|e|e.to_string())?;
    serde_json::to_writer(&mut record,&package).map_err(|e|e.to_string())?;
    record.persist(metadata).map_err(|e|e.to_string())?;
    mark_ready(id,package,path,&cancel)
}

#[tauri::command]
pub async fn oceanway_open_installer(id:String)->Result<InstallStatus,String> {
    let (package,path,previous)={
        let mut guard=jobs().lock().map_err(|_|"安装状态不可用")?;
        if guard.values().any(|j|busy(&j.status.stage)) {return Err("请等待当前操作完成".into());}
        let job=guard.get_mut(&id).ok_or("安装任务不存在，请重新下载")?;
        if !matches!(job.status.stage.as_str(),"ready"|"awaiting_install") {return Err("请先下载完整安装包".into());}
        let (package,path)=job.package.clone().ok_or("安装包不存在，请重新下载")?;
        let previous=job.status.stage.clone();
        job.status.stage="opening".into();job.status.message="正在校验并打开安装包…".into();job.status.action_error=None;
        (package,path,previous)
    };
    let task_id=id.clone();let status_id=id.clone();
    tauri::async_runtime::spawn(async move {
        let app=package.app.clone();let count=package.size;
        let result=tauri::async_runtime::spawn_blocking(move || {
            if let Err(e)=verify_file(&package,&path) {return Err((true,e));}
            check_system(&package).map_err(|e|(false,e))?;
            if package.app=="claude" {
                update(&task_id,"installing",count,count,"正在安装 Claude Code…");
                install_claude(&path).map_err(|e|(false,e))
            } else {open_package(&package,&path).map_err(|e|(false,e))}
        }).await.unwrap_or_else(|e|Err((false,e.to_string())));
        match result {
            Ok(()) => {
                update(&id,"awaiting_install",count,count,if app=="claude" {"安装操作完成，请检测安装结果。"}else if cfg!(target_os="macos") {"安装包已打开。DMG 请将应用拖到 Applications；PKG 请按向导安装。装好后点击“检测安装结果”。"}else{"安装包已打开，请按系统提示完成安装，再点击“检测安装结果”。"});
                if app=="claude" {let _=oceanway_check_install(id).await;}
            }
            Err((invalid,message)) => {
                if invalid {update(&id,"failed",count,count,&message);}
                else if let Ok(mut guard)=jobs().lock() {if let Some(job)=guard.get_mut(&id) {
                    job.status.stage=previous;job.status.action_error=Some(message);job.status.message="安装包已下载，可再次打开或检测安装结果。".into();
                }}
            }
        }
    });
    oceanway_get_install(status_id)
}
#[tauri::command]
pub async fn oceanway_check_install(id:String)->Result<InstallStatus,String> {
    let (app,previous,count)={
        let mut guard=jobs().lock().map_err(|_|"安装状态不可用")?;
        let job=guard.get_mut(&id).ok_or("安装任务不存在")?;
        if busy(&job.status.stage) {return Err("请等待当前操作完成".into());}
        let previous=job.status.stage.clone();let count=job.status.total;
        job.status.stage="checking".into();job.status.message="正在检测安装结果…".into();job.status.action_error=None;
        (job.status.app_id.clone(),previous,count)
    };
    match installed(&app).await {
        Ok(true)=>update(&id,"installed",count,count,"已检测到应用，安装完成，可以继续配置 Oceanway。"),
        Ok(false)=>update(&id,&previous,count,count,"尚未检测到应用，请先完成安装，再点击“检测安装结果”。"),
        Err(e)=>{update(&id,&previous,count,count,"安装检测未完成，请重试。");if let Ok(mut guard)=jobs().lock(){if let Some(job)=guard.get_mut(&id){job.status.action_error=Some(e);}}}
    }
    oceanway_get_install(id)
}
fn open_package(package:&Package,path:&Path)->Result<(),String> {
    validate_package(package)?;
    #[cfg(target_os="windows")]
    {
        // Start the normal visible installer. Do not wait for the wizard or claim completion.
        let script=r#"$ErrorActionPreference='Stop';try {Start-Process -FilePath $env:OCEANWAY_INSTALL_FILE -ErrorAction Stop | Out-Null} catch {[Console]::Error.WriteLine($_.Exception.Message);exit 1}"#;
        let result=crate::agent_desktops::bounded_output(crate::agent_desktops::windows_powershell(script).env("OCEANWAY_INSTALL_FILE",path),45)?;
        return if result.status.success(){Ok(())}else{Err(format!("安装包未打开，请重试。{}",String::from_utf8_lossy(&result.stderr).trim()))};
    }
    #[cfg(target_os="macos")]
    {
        let result=crate::agent_desktops::bounded_output(Command::new("open").arg(path),45)?;
        return if result.status.success(){Ok(())}else{Err(format!("安装包未打开，请重试。{}",String::from_utf8_lossy(&result.stderr).trim()))};
    }
    #[allow(unreachable_code)] Err("当前系统暂不支持打开此安装包".into())
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
        let _=crate::agent_desktops::bounded_output(&mut crate::agent_desktops::windows_powershell(
          "$s='[DllImport(\"user32.dll\", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, UIntPtr w, string l, uint f, uint t, out UIntPtr r);'; $t=Add-Type -MemberDefinition $s -Name EnvNotify -Namespace Oceanway -PassThru; $r=[UIntPtr]::Zero; $null=$t::SendMessageTimeout([IntPtr]0xffff,0x1A,[UIntPtr]::Zero,'Environment',2,3000,[ref]$r)"),5);
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


#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Downloads real software; run only on a disposable GitHub runner"]
    async fn download_on_disposable_runner() {
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        let app=std::env::var("OCEANWAY_INSTALL_TEST_TARGET").expect("explicit agent target");
        assert!(APPS.contains(&app.as_str()));
        assert!(!installed(&app).await.unwrap(),"runner must not already have this agent installed");
        let started=oceanway_start_install(app.clone()).await.unwrap();
        let deadline=std::time::Instant::now()+Duration::from_secs(2400);
        let mut previous=String::new();
        loop {
            let status=oceanway_get_install(started.id.clone()).unwrap();
            let progress=format!("{}: {} / {} - {}",status.stage,status.downloaded,status.total,status.message);
            if progress!=previous {println!("{progress}");previous=progress;}
            if !busy(&status.stage) {
                assert_eq!(status.stage,"ready","download failed: {}",status.message);break;
            }
            assert!(std::time::Instant::now()<deadline,"download did not complete");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        let cached=cached_package(&cache_root(),&app).expect("verified package survives the download task");
        verify_file(&cached.0,&cached.1).unwrap();
        assert!(!installed(&app).await.unwrap(),"downloading must not install an application");
        assert_eq!(oceanway_check_install(started.id.clone()).await.unwrap().stage,"ready");
        assert_eq!(oceanway_start_install(app.clone()).await.unwrap().id,started.id,"reuse downloaded package");
        // Other workflows verify the system installers themselves. Native Claude Code
        // has no wizard, so exercise its explicitly requested installation here.
        if app=="claude" {
            oceanway_open_installer(started.id.clone()).await.unwrap();
            for _ in 0..30 {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let status=oceanway_get_install(started.id.clone()).unwrap();
                if !busy(&status.stage) {assert_eq!(status.stage,"installed","{} {:?}",status.message,status.action_error);break;}
            }
            assert!(installed(&app).await.unwrap());
        }
        println!("Verified complete download, checksum, reuse and explicit installation boundary: {app}");
    }
    fn package(arch:&str)->Package {Package{app:"codex".into(),platform:"macos".into(),architecture:arch.into(),filename:"codex.dmg".into(),sha256:"a".repeat(64),size:1024,install:String::new(),minimum_windows_build:0}}
    fn fixture()->(tempfile::TempDir,Package,PathBuf) {
        let temp=tempfile::tempdir().unwrap();let mut p=package("x64");
        p.size=4;p.sha256=format!("{:x}",Sha256::digest(b"test"));
        let path=temp.path().join("installer.dmg");std::fs::write(&path,b"test").unwrap();(temp,p,path)
    }
    fn ready_job(p:Package,path:PathBuf)->String {
        let id=uuid::Uuid::new_v4().to_string();
        jobs().lock().unwrap().insert(id.clone(),Job{status:InstallStatus{id:id.clone(),app_id:p.app.clone(),stage:"verifying".into(),downloaded:0,total:0,message:String::new(),action_error:None},cancel:Arc::new(AtomicBool::new(false)),package:None});
        mark_ready(&id,p,path,&AtomicBool::new(false)).unwrap();id
    }
    #[test] fn download_completion_waits_for_the_user_and_allows_another_download() {
        let (_temp,p,path)=fixture();let id=ready_job(p,path.clone());
        let status=oceanway_get_install(id.clone()).unwrap();
        assert_eq!(status.stage,"ready");assert!(!busy(&status.stage));assert!(path.is_file());
        assert!(mark_ready(&id,package("x64"),path,&AtomicBool::new(true)).is_err());
        jobs().lock().unwrap().remove(&id);
    }
    #[tokio::test] async fn deleted_or_replaced_packages_cannot_be_opened() {
        let (_temp,p,path)=fixture();let id=ready_job(p,path.clone());
        std::fs::write(&path,b"evil").unwrap();
        oceanway_open_installer(id.clone()).await.unwrap();
        for _ in 0..100 {
            let status=oceanway_get_install(id.clone()).unwrap();
            if !busy(&status.stage) {assert_eq!(status.stage,"failed");assert!(status.message.contains("校验失败"));break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(oceanway_get_install(id.clone()).unwrap().stage,"failed");
        jobs().lock().unwrap().remove(&id);
        std::fs::remove_file(&path).unwrap();assert!(verify_file(&package("x64"),&path).is_err());
    }
    #[test] fn cache_survives_new_jobs_but_rejects_corruption() {
        let (temp,mut p,_path)=fixture();p.platform=std::env::consts::OS.into();
        p.architecture=match std::env::consts::ARCH {"x86_64"=>"x64","aarch64"=>"arm64",a=>a}.into();
        p.filename=if cfg!(windows){"installer.exe"}else{"installer.dmg"}.into();
        if !cfg!(any(target_os="windows",target_os="macos")){return;}
        let path=package_path(temp.path(),&p);std::fs::create_dir_all(path.parent().unwrap()).unwrap();std::fs::write(&path,b"test").unwrap();
        std::fs::write(temp.path().join("codex/package.json"),serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(cached_package(temp.path(),"codex").unwrap().1,path);
        std::fs::write(&path,b"evil").unwrap();assert!(cached_package(temp.path(),"codex").is_none());
    }
    #[test] fn validates_the_entire_file() {
        let (_temp,p,path)=fixture();assert!(verify_file(&p,&path).is_ok());
        std::fs::write(&path,b"truncated").unwrap();assert!(verify_file(&p,&path).is_err());
    }
    #[test] fn selects_actual_platform_and_architecture() {
        let m=||Manifest{packages:vec![package("arm64"),package("x64")]};
        assert_eq!(select_package(m(),"codex","macos","x86_64").unwrap().architecture,"x64");
        assert!(select_package(m(),"codex","windows","x86_64").is_err());
        assert!(select_package(m(),"hermes","macos","aarch64").is_err());
    }
    #[test] fn rejects_unsafe_or_unverifiable_downloads() {
        for filename in ["../setup.exe","https://other/setup.exe","setup.exe?x=1","","payload.sh"] {let mut p=package("x64");p.filename=filename.into();assert!(validate_package(&p).is_err());}
        let mut p=package("x64");p.sha256="unknown".into();assert!(validate_package(&p).is_err());
    }
    #[test] fn path_update_preserves_existing_entries_and_is_idempotent() {
        assert_eq!(path_with_bin("a;b","c",';'),"a;b;c");
        assert_eq!(path_with_bin("C:/bin","c:/bin",';'),"C:/bin");
    }
}
