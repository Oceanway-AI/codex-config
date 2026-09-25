//! Desktop thread overrides can replace config developer_instructions. Global
//! AGENTS is a separate supported input; only our marker-delimited block is owned.
use super::*;

const BEGIN: &str = "<!-- OCEANWAY:DIRECT-IMAGE-API:BEGIN -->";
const END: &str = "<!-- OCEANWAY:DIRECT-IMAGE-API:END -->";
const NAMES: [&str; 2] = ["AGENTS.md", "AGENTS.override.md"];

fn read(path: &Path) -> Result<Option<String>, String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("全局任务指令不是普通文件，未修改配置。".into());
        }
    }
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法读取全局任务指令，未覆盖原文件。".into()),
    }
}

fn strip(text: &str) -> Result<String, String> {
    match (text.find(BEGIN), text.find(END)) {
        (None, None) => Ok(text.to_string()),
        (Some(start), Some(end)) if end > start => {
            let finish = end + END.len();
            if text[start + BEGIN.len()..end].contains(BEGIN)
                || text[finish..].contains(BEGIN) || text[finish..].contains(END) {
                return Err("全局任务指令中的图片规则标记重复，未覆盖。".into());
            }
            // The prefix and its separator are always inserted together.
            let tail = text[finish..].strip_prefix("\n\n").unwrap_or(&text[finish..]);
            Ok(format!("{}{tail}", &text[..start]))
        }
        _ => Err("全局任务指令中的图片规则标记损坏，未覆盖。".into()),
    }
}

pub(super) fn target(home: &Path) -> Result<PathBuf, String> {
    let override_path = home.join(NAMES[1]);
    if read(&override_path)?.is_some_and(|text| !text.trim().is_empty()) {
        Ok(override_path)
    } else {
        Ok(home.join(NAMES[0]))
    }
}

pub(super) fn configured(home: &Path) -> bool {
    target(home).and_then(|path| read(&path))
        .ok().flatten()
        .is_some_and(|text| text.starts_with(direct_image_config::RULES.trim_end()))
}

pub(super) fn prepare(home: &Path) -> Result<(PathBuf, Option<String>, String), String> {
    let path = target(home)?;
    let previous = read(&path)?;
    let remaining = strip(previous.as_deref().unwrap_or(""))?;
    let next = format!("{}\n\n{remaining}", direct_image_config::RULES.trim_end());
    Ok((path, previous, next))
}

pub(super) fn write_with_config(home: &Path, config: &Path, rendered: &str) -> Result<(), String> {
    let (agents, previous, next) = prepare(home)?;
    let old_config = if config.exists() {
        Some(fs::read(config).map_err(|_| "无法读取配置以建立写入事务。")?)
    } else { None };
    backup_file(&agents)?;
    let operation = (|| {
        write_private_atomic(&agents, next.as_bytes()).map_err(|_| "无法保存全局图片任务指令。")?;
        write_private_atomic(config, rendered.as_bytes()).map_err(|_| "无法保存 config.toml。")?;
        verify_config_write(config, rendered)?;
        if read(&agents)?.as_deref() != Some(next.as_str()) {
            return Err("全局图片任务指令回读不一致。".into());
        }
        Ok(())
    })();
    if let Err(error) = operation {
        let mut failed = Vec::new();
        for (path, bytes) in [
            (agents.as_path(), previous.as_ref().map(|s| s.as_bytes())),
            (config, old_config.as_deref()),
        ] {
            let restored = match bytes {
                Some(bytes) => write_private_atomic(path, bytes),
                None => fs::remove_file(path).or_else(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }
                }),
            };
            if restored.is_err() { failed.push(display_path(path)); }
        }
        return Err(if failed.is_empty() { error } else {
            format!("{error} 回滚失败，请使用备份恢复：{}", failed.join(", "))
        });
    }
    Ok(())
}

pub(super) fn restore(home: &Path) -> Result<(), String> {
    // Preflight both files before changing either one.
    let files = NAMES.iter().map(|name| {
        let path = home.join(name);
        let original = read(&path)?;
        let remaining = original.as_deref().map(strip).transpose()?;
        Ok((path, original, remaining))
    }).collect::<Result<Vec<_>, String>>()?;
    for (path, original, remaining) in files {
        if let (Some(original), Some(remaining)) = (original, remaining) {
            if original == remaining { continue; }
            backup_file(&path)?;
            // Retain an empty file: users may have had one before configuration.
            write_private_atomic(&path, remaining.as_bytes())
                .map_err(|_| "无法撤销全局图片规则，请保留备份。")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserve_existing_instructions_and_later_edits_exactly() {
        let original = "Keep my settings.\r\n";
        let merged = format!("{}\n\n{original}", direct_image_config::RULES.trim_end());
        assert_eq!(strip(&merged).unwrap(), original);
        assert_eq!(strip(&format!("{merged}New instructions.")).unwrap(),
            format!("{original}New instructions."));
        assert!(strip(BEGIN).is_err());
        assert!(strip(&format!("{merged}{merged}")).is_err());
    }

    #[test]
    fn override_precedence_and_repeat_configuration() {
        let home = tests_home();
        fs::write(home.join(NAMES[0]), "standard").unwrap();
        fs::write(home.join(NAMES[1]), "override").unwrap();
        let (path, _, first) = prepare(&home).unwrap();
        assert!(path.ends_with(NAMES[1]));
        fs::write(&path, &first).unwrap();
        assert_eq!(prepare(&home).unwrap().2, first);
        assert!(configured(&home));
        restore(&home).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "override");
        assert_eq!(fs::read_to_string(home.join(NAMES[0])).unwrap(), "standard");
    }

    fn tests_home() -> PathBuf {
        let home = env::temp_dir().join(format!("ow-agents-{}", Local::now().timestamp_nanos_opt().unwrap()));
        fs::create_dir_all(&home).unwrap();
        home
    }
}
