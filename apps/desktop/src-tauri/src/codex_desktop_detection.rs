//! Read-only desktop detection. A standalone/npm CLI never proves the GUI is
//! installed. Do not launch, close, or inspect any account/session data here.

#[cfg(any(target_os = "windows", test))]
fn windows_bundle_present(executable: &std::path::Path) -> bool {
    let Some(directory) = executable.parent() else {
        return false;
    };
    let name = executable.file_name().and_then(|name| name.to_str());
    let resources = directory.join("resources");
    let codex_host = name.is_some_and(|name| name.eq_ignore_ascii_case("Codex.exe"));
    let renamed_host = name.is_some_and(|name| name.eq_ignore_ascii_case("ChatGPT.exe"))
        && ["codex.exe", "codex-cli/bin/codex.exe"]
            .iter()
            .any(|path| resources.join(path).is_file());
    executable.is_file() && resources.join("app.asar").is_file() && (codex_host || renamed_host)
}

#[cfg(any(target_os = "macos", test))]
fn macos_bundle_present(bundle: &std::path::Path) -> bool {
    let Some(name) = bundle.file_stem().and_then(|name| name.to_str()) else {
        return false;
    };
    if !bundle.join("Contents/MacOS").join(name).is_file() {
        return false;
    }
    name == "Codex"
        || (name == "ChatGPT"
            && [
                "Contents/Resources/codex",
                "Contents/Resources/codex-cli/bin/codex",
            ]
            .iter()
            .any(|path| bundle.join(path).is_file()))
}

#[cfg(target_os = "windows")]
pub fn is_installed() -> bool {
    use std::os::windows::process::CommandExt;
    use std::{
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    // Probe ordinary installation directories before asking Windows about Store
    // packages or a running GUI installed in a custom location.
    let mut roots = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        roots.extend([local.join("Programs"), local]);
    }
    for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            roots.push(PathBuf::from(root));
        }
    }
    for root in roots {
        for host in ["Codex", "ChatGPT"] {
            for directory in [root.join(host), root.join("OpenAI").join(host)] {
                if windows_bundle_present(&directory.join(format!("{host}.exe"))) {
                    return true;
                }
            }
        }
    }

    const SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$candidates = @(Get-Process -Name Codex,ChatGPT | ForEach-Object { if ($_.Path) { $_.Path } })
$packages = @(Get-AppxPackage -Name '*Codex*'; Get-AppxPackage -Name '*ChatGPT*')
foreach ($package in $packages) {
  if ($package.InstallLocation) {
    foreach ($relative in @('app\Codex.exe','Codex.exe','app\ChatGPT.exe','ChatGPT.exe')) {
      $candidates += Join-Path $package.InstallLocation $relative
    }
  }
}
$candidates | Sort-Object -Unique
"#;
    let Ok(mut child) = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
    else {
        return false;
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().ok().is_some_and(|output| {
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .any(|line| windows_bundle_present(std::path::Path::new(line.trim())))
                });
            }
            Ok(None) if started.elapsed() < Duration::from_secs(8) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                // Only our own read-only detector is terminated on timeout.
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub fn is_installed() -> bool {
    let mut roots = vec![std::path::PathBuf::from("/Applications")];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    roots.iter().any(|root| {
        ["Codex.app", "ChatGPT.app"]
            .iter()
            .any(|name| macos_bundle_present(&root.join(name)))
    })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn is_installed() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "oceanway-desktop-detection-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn file(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"fixture, never executed").unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn oceanway_windows_desktop_requires_gui_bundle_not_a_cli() {
        let fixture = Fixture::new();
        let cli = fixture.file("bin/codex.exe");
        assert!(!windows_bundle_present(&cli));
        let gui = fixture.file("app/Codex.exe");
        assert!(!windows_bundle_present(&gui));
        fixture.file("app/resources/app.asar");
        assert!(windows_bundle_present(&gui));
        let chatgpt = fixture.file("renamed/ChatGPT.exe");
        fixture.file("renamed/resources/app.asar");
        assert!(!windows_bundle_present(&chatgpt));
        fixture.file("renamed/resources/codex-cli/bin/codex.exe");
        assert!(windows_bundle_present(&chatgpt));
    }

    #[test]
    fn oceanway_macos_desktop_recognizes_current_and_legacy_codex_hosts() {
        let fixture = Fixture::new();
        fixture.file("Codex.app/Contents/MacOS/Codex");
        assert!(macos_bundle_present(&fixture.0.join("Codex.app")));
        fixture.file("ChatGPT.app/Contents/MacOS/ChatGPT");
        assert!(!macos_bundle_present(&fixture.0.join("ChatGPT.app")));
        fixture.file("ChatGPT.app/Contents/Resources/codex-cli/bin/codex");
        assert!(macos_bundle_present(&fixture.0.join("ChatGPT.app")));
        fixture.file("Legacy/ChatGPT.app/Contents/MacOS/ChatGPT");
        fixture.file("Legacy/ChatGPT.app/Contents/Resources/codex");
        assert!(macos_bundle_present(&fixture.0.join("Legacy/ChatGPT.app")));
        fixture.file("Fake.app/Contents/MacOS/Fake");
        assert!(!macos_bundle_present(&fixture.0.join("Fake.app")));
    }
}
