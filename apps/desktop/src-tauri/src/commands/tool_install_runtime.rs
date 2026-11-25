use super::*;

fn requires_node(tool: &str) -> bool {
    matches!(
        tool,
        "claude" | "gemini" | "grok" | "opencode" | "openclaw" | "pi" | "mcode" | "dsh"
    )
}

pub(super) fn posix(tool: &str, command: String) -> String {
    if !requires_node(tool) {
        return command;
    }
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    // Keep the WSL batch invocation on one line, including when compiled on Windows.
    let encoded = STANDARD.encode(include_str!("install_node.sh").replace("\r\n", "\n"));
    let decode = if cfg!(target_os = "macos") {
        "-D"
    } else {
        "-d"
    };
    format!(
        "bash -c {} oceanway-install {}",
        shell_single_quote(&format!(
            "ow_script=$(printf %s {encoded} | base64 {decode}) && eval \"$ow_script\""
        )),
        shell_single_quote(&command)
    )
}

#[cfg(target_os = "windows")]
pub(super) fn windows(tool: &str, command: String) -> String {
    if !requires_node(tool) {
        return format!("call {command}");
    }
    let script = format!(
        "$owInstallCommand = '{}'\n{}",
        command.replace('\'', "''"),
        include_str!("install_node.ps1")
    );
    format!(
        "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(&script)
    )
}

pub(super) fn bin() -> PathBuf {
    let root = crate::config::get_home_dir().join(".oceanway-config/runtimes/node");
    if cfg!(target_os = "windows") {
        root
    } else {
        root.join("bin")
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn install_runtime_reuses_usable_node_and_propagates_agent_failure() {
        let root = tempfile::tempdir().unwrap();
        for name in ["node", "npm"] {
            let path = root.path().join(name);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(posix("gemini", "exit 23".into()))
            .env("HOME", root.path())
            .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(23),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!root.path().join(".oceanway-config").exists());
    }

    #[test]
    fn install_runtime_rejects_corrupt_download_before_installing_agent() {
        let root = tempfile::tempdir().unwrap();
        let fake = root.path().join("fake");
        std::fs::create_dir(&fake).unwrap();
        for (name,body) in [
            ("node", "#!/bin/sh\nexit 1\n"),
            ("curl", "#!/bin/sh\nwhile [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then shift; target=$1; fi; shift; done\ncase \"$target\" in *SHASUMS256.txt) printf '%064d  node-v24.16.0-linux-x64.tar.gz\\n' 0 > \"$target\";; *) printf corrupt > \"$target\";; esac\n"),
            ("uname", "#!/bin/sh\ncase \"$1\" in -s) echo Linux;; *) echo x86_64;; esac\n"),
        ] {
            let path=fake.join(name); std::fs::write(&path,body).unwrap();
            std::fs::set_permissions(path,std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(posix("gemini", "touch \"$HOME/agent-executed\"".into()))
            .env("HOME", root.path())
            .env("PATH", format!("{}:/usr/bin:/bin", fake.display()))
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("checksum mismatch"));
        assert!(!root.path().join("agent-executed").exists());
        assert!(!root.path().join(".oceanway-config").exists());
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;
    #[test]
    fn install_runtime_reuses_windows_node_and_propagates_agent_failure() {
        let root = tempfile::tempdir().unwrap();
        for name in ["node.cmd", "npm.cmd"] {
            std::fs::write(root.path().join(name), "@exit /b 0\r\n").unwrap();
        }
        let output = std::process::Command::new("cmd")
            .arg("/d")
            .arg("/c")
            .arg(windows("gemini", "exit /b 23".into()))
            .env("USERPROFILE", root.path())
            .env(
                "PATH",
                format!(
                    "{};{}",
                    root.path().display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(23),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!root.path().join(".oceanway-config").exists());
    }
}
