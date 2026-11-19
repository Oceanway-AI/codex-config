use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

static LAST_HOST: Mutex<Option<PathBuf>> = Mutex::new(None);

#[link(name = "proc")]
extern "C" {
    fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, size: u32) -> i32;
}

fn executable(pid: i32) -> Option<PathBuf> {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let mut buf = [0u8; 4096]; // PROC_PIDPATHINFO_MAXSIZE
    let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    let end = buf.iter().position(|byte| *byte == 0).unwrap_or(buf.len());
    Some(PathBuf::from(OsStr::from_bytes(&buf[..end])))
}

fn bundle(exe: &Path) -> Option<&Path> {
    exe.ancestors().find(|p| {
        p.extension().is_some_and(|ext| ext == "app")
            && crate::codex_desktop_detection::macos_bundle_present(p)
    })
}

fn clients() -> Result<Vec<(i32, PathBuf)>, String> {
    // comm is the executable, not the arguments (which can contain user input).
    let output = crate::agent_desktops::bounded_output(
        Command::new("/bin/ps").args([
            "-ww",
            "-U",
            &unsafe { libc::getuid() }.to_string(),
            "-o",
            "pid=,comm=",
        ]),
        4,
    )?;
    if !output.status.success() {
        return Err("无法检查 Codex 后台进程".into());
    }
    let root = crate::config::get_home_dir().join("Library/Application Support/OpenAI/Codex");
    let runtimes = root.join("runtimes");
    let mut known = std::collections::HashMap::new();
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, exe) = line.trim().split_once(char::is_whitespace)?;
            let pid = pid.parse().ok()?;
            let exe = executable(pid).unwrap_or_else(|| PathBuf::from(exe.trim_start()));
            let name = exe.file_name()?.to_str()?;
            let runtime = (name.starts_with("codex-") && exe.starts_with(&root))
                || (matches!(name, "node" | "node_repl") && exe.starts_with(&runtimes));
            if name == "codex"
                || name == "codex-aarch64-apple-darwin"
                || name == "codex-x86_64-apple-darwin"
                || runtime
                || exe.ancestors().filter(|p| p.extension().is_some_and(|ext| ext == "app")).any(|p| {
                    *known.entry(p.to_owned()).or_insert_with(|| crate::codex_desktop_detection::macos_bundle_present(p))
                })
            {
                Some((pid, exe))
            } else {
                None
            }
        })
        .collect())
}

pub fn running() -> Result<bool, String> {
    Ok(!clients()?.is_empty())
}

fn signal(rows: &[(i32, PathBuf)], sig: i32) -> Result<(), String> {
    let current = clients()?;
    for (pid, path) in rows {
        if current.iter().any(|r| r.0 == *pid && r.1 == *path)
            && unsafe { libc::kill(*pid, sig) } != 0
        {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                return Err(format!("无法退出 Codex（PID {pid}）：{err}"));
            }
        }
    }
    Ok(())
}

pub fn close() -> Result<(), String> {
    let rows = clients()?;
    if let Some(path) = rows.iter().find_map(|(_, exe)| bundle(exe)) {
        *LAST_HOST.lock().map_err(|_| "Codex 状态不可用")? = Some(path.to_path_buf());
    }
    if !rows.is_empty() {
        signal(&rows, libc::SIGTERM)?;
    }
    let force_at = Instant::now() + Duration::from_secs(5);
    let deadline = force_at + Duration::from_secs(4);
    let mut quiet = 0;
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(250));
        let remaining = clients()?;
        if remaining.is_empty() {
            quiet += 1;
            if quiet == 3 {
                return Ok(());
            }
            continue;
        }
        quiet = 0;
        for row in &remaining {
            let exe = &row.1;
            if !rows.contains(row)
                && bundle(exe).is_some_and(|b| exe.parent() == Some(&b.join("Contents/MacOS")))
            {
                return Err("检测到新打开的 Codex，请先保存工作后重试。".into());
            }
        }
        if Instant::now() >= force_at {
            signal(&remaining, libc::SIGKILL)?;
        }
    }
    Err("Codex 后台仍在运行，请关闭正在使用 Codex 的终端或编辑器，再重试。".into())
}

pub fn launch_codex(restart: bool) -> Result<Option<String>, String> {
    let previous = LAST_HOST.lock().map_err(|_| "Codex 状态不可用")?.clone();
    let current = clients()?;
    if !restart && !current.is_empty() {
        return Err("OCEANWAY_CODEX_RESTART_REQUIRED".into());
    }
    let target = current
        .into_iter()
        .filter_map(|(_, exe)| bundle(&exe).map(Path::to_path_buf))
        .find(|p| crate::codex_desktop_detection::macos_install_location(p))
        .or_else(|| previous.filter(|p| crate::codex_desktop_detection::macos_install_location(p) && crate::codex_desktop_detection::macos_bundle_present(p)))
        .or_else(crate::codex_desktop_detection::macos_installed_bundle);
    let Some(path) = target else {
        return Ok(None);
    };
    if restart {
        close()?;
    }
    let output =
        crate::agent_desktops::bounded_output(Command::new("/usr/bin/open").arg(&path), 8)?;
    if !output.status.success() {
        return Err("系统未能打开 Codex，请检查安装后重试。".into());
    }
    // Quartz checks on-screen layer-zero windows; no Accessibility permission required.
    let script = r#"ObjC.import('AppKit'); ObjC.import('CoreGraphics');
function run(argv) {
  const until = Date.now() + 30000;
  let last = [];
  while (Date.now() < until) {
    const apps = $.NSWorkspace.sharedWorkspace.runningApplications;
    const pids = [];
    for (let i = 0; i < apps.count; i++) {
      const a = apps.objectAtIndex(i);
      const url = a.bundleURL;
      if (url && !url.isNil() && ObjC.unwrap(url.path) === argv[0]) pids.push(Number(a.processIdentifier));
    }
    const windows = ObjC.deepUnwrap($.CGWindowListCopyWindowInfo(17, 0)) || [];
    const visible = windows.filter(w => pids.includes(w.kCGWindowOwnerPID) && w.kCGWindowLayer === 0 && w.kCGWindowBounds.Width > 1 && w.kCGWindowBounds.Height > 1).map(w => String(w.kCGWindowOwnerPID) + ':' + w.kCGWindowNumber);
    if (visible.some(id => last.includes(id))) return 'opened';
    last = visible;
    $.NSThread.sleepForTimeInterval(0.3);
  }
  return 'timeout';
}"#;
    let check = crate::agent_desktops::bounded_output(
        Command::new("/usr/bin/osascript")
            .args(["-l", "JavaScript", "-e", script])
            .arg(&path),
        35,
    )?;
    if !check.status.success() || String::from_utf8_lossy(&check.stdout).trim() != "opened" {
        return Err("没有检测到 Codex 窗口。配置已保留，请稍等片刻；窗口仍未出现时可以重试，或从“应用程序”打开。".into());
    }
    Ok(Some("Codex 窗口已打开，可以开始对话。".into()))
}

#[cfg(test)]
mod acceptance {
    use super::*;

    #[test]
    #[ignore = "Requires the Oceanway DMG installed on a disposable macOS desktop"]
    fn installed_mac_codex_lifecycle() {
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(std::env::var("RUNNER_ENVIRONMENT").as_deref(), Ok("github-hosted"));
        assert!(!running().unwrap(), "expected an unused test desktop");
        let app = crate::codex_desktop_detection::macos_installed_bundle().expect("fresh install detected");
        assert!(app.starts_with(crate::config::get_home_dir().join("Applications")), "unexpected install: {app:?}");
        struct Cleanup;
        impl Drop for Cleanup { fn drop(&mut self) { let _ = close(); } }
        let _cleanup = Cleanup;
        // A CLI worker outliving the window must still block a new configuration.
        let tmp = tempfile::tempdir().unwrap();
        let worker = tmp.path().join("codex");
        std::fs::copy("/bin/sleep", &worker).unwrap();
        let mut worker = Command::new(&worker).arg("180").spawn().unwrap();
        let reap = std::thread::spawn(move || worker.wait());
        assert!(running().unwrap());
        assert_eq!(launch_codex(false).unwrap_err(), "OCEANWAY_CODEX_RESTART_REQUIRED");
        close().unwrap();
        assert!(reap.join().unwrap().is_ok());
        assert!(!running().unwrap());
        let host = || clients().unwrap().into_iter()
            .find(|(_, exe)| exe.parent() == Some(app.join("Contents/MacOS").as_path())).expect("visible desktop host").0;
        assert!(launch_codex(false).unwrap().is_some());
        let first = host();
        assert_eq!(launch_codex(false).unwrap_err(), "OCEANWAY_CODEX_RESTART_REQUIRED");
        assert!(launch_codex(true).unwrap().is_some());
        let next = host();
        assert_ne!(first, next);
        assert!(!clients().unwrap().iter().any(|(pid,_)| *pid == first));
        close().unwrap();
        assert!(!running().unwrap());
        assert!(crate::codex_desktop_detection::is_installed(), "previously installed, closed app must remain discoverable");
        eprintln!("Verified {app:?}, background worker exit, and visible windows: {first} -> {next}");
    }
}
