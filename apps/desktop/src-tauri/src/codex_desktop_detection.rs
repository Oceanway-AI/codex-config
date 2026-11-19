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
pub(crate) fn macos_bundle_present(bundle: &std::path::Path) -> bool {
    if bundle.extension().is_none_or(|ext| ext != "app") { return false; }
    let plist_path = bundle.join("Contents/Info.plist");
    if plist_path.is_file() {
        let Ok(info) = plist::Value::from_file(plist_path) else { return false; };
        let Some(info) = info.as_dictionary() else { return false; };
        if let Some(id) = info.get("CFBundleIdentifier").and_then(plist::Value::as_string) {
            let Some(exe) = info.get("CFBundleExecutable").and_then(plist::Value::as_string) else { return false; };
            return id == "com.openai.codex"
                && std::path::Path::new(exe).file_name().is_some_and(|name| name == exe)
                && bundle.join("Contents/MacOS").join(exe).is_file();
        }
    }
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
    use std::path::PathBuf;

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

    // Use the launcher's exact package, registry and shortcut resolution.
    crate::oceanway_desktop::installed()
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_install_location(path: &std::path::Path) -> bool {
    if !path.is_absolute() || path.components().any(|c| [".Trash", ".Trashes", "AppTranslocation"].iter().any(|n| c.as_os_str() == *n)) { return false; }
    if !path.starts_with("/Volumes") { return true; }
    // Installed on a separate disk is valid; mounted installer images are not.
    let Ok(output) = crate::agent_desktops::bounded_output(std::process::Command::new("/usr/bin/hdiutil").args(["info","-plist"]), 5) else {return false;};
    if !output.status.success() { return false; }
    let Ok(info) = plist::Value::from_reader(std::io::Cursor::new(output.stdout)) else { return false; };
    let Some(images) = info.as_dictionary().and_then(|d|d.get("images")).and_then(plist::Value::as_array) else { return false; };
    !images.iter().filter_map(|image|image.as_dictionary()?.get("system-entities")?.as_array()).flatten()
        .filter_map(|entity|entity.as_dictionary()?.get("mount-point")?.as_string())
        .any(|mount| path.starts_with(mount))
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_installed_bundle() -> Option<std::path::PathBuf> {
    let mut roots = vec![std::path::PathBuf::from("/Applications")];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    let conventional = roots.into_iter().flat_map(|root| ["Codex.app", "ChatGPT.app"].map(|name|root.join(name))).find(|path| macos_bundle_present(path));
    if conventional.is_some() { return conventional }
    // Launch Services also knows installed apps moved to a different folder.
    // Lookup only: this must never launch Codex while detecting an installation.
    let script = r#"ObjC.import('AppKit'); function run() { const url = $.NSWorkspace.sharedWorkspace.URLForApplicationWithBundleIdentifier('com.openai.codex'); return ObjC.unwrap(url.path) || ''; }"#;
    let result = crate::agent_desktops::bounded_output(std::process::Command::new("/usr/bin/osascript").args(["-l","JavaScript","-e",script]), 5).ok()?;
    if !result.status.success() { return None; }
    let path = std::path::PathBuf::from(std::str::from_utf8(&result.stdout).ok()?.trim_end());
    // A still-mounted download is not an installation.
    (macos_install_location(&path) && macos_bundle_present(&path)).then_some(path)
}

#[cfg(target_os = "macos")]
pub fn is_installed() -> bool { macos_installed_bundle().is_some() }
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

    #[test]
    fn oceanway_macos_recognizes_bundle_identity_even_when_renamed() {
        let fixture = Fixture::new();
        fixture.file("Work Tools/My Agent.app/Contents/MacOS/ChatGPT");
        let path = fixture.file("Work Tools/My Agent.app/Contents/Info.plist");
        let bundle = fixture.0.join("Work Tools/My Agent.app");
        for (id, exe, expected) in [
            ("com.openai.codex","ChatGPT",true),
            ("com.openai.chat","ChatGPT",false),
            ("com.openai.codex","../MacOS/ChatGPT",false),
        ] {
            let mut info = plist::Dictionary::new();
            info.insert("CFBundleIdentifier".into(),id.into());
            info.insert("CFBundleExecutable".into(),exe.into());
            plist::Value::Dictionary(info).to_file_binary(&path).unwrap();
            assert_eq!(macos_bundle_present(&bundle),expected);
        }
    }
}
