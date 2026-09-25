//! Desktop thread overrides can replace config developer_instructions. Global
//! AGENTS is a separate supported input; only our marker-delimited block is owned.
use super::*;

const BEGIN: &str = "<!-- OCEANWAY:IMAGE-MCP-ROUTING:BEGIN v1 -->";
const END: &str = "<!-- OCEANWAY:IMAGE-MCP-ROUTING:END -->";
const LEGACY_BEGIN: &str = "<!-- OCEANWAY:DIRECT-IMAGE-API:BEGIN -->";
const LEGACY_END: &str = "<!-- OCEANWAY:DIRECT-IMAGE-API:END -->";
pub(super) const RULES: &str = include_str!("image-mcp-instructions.md");
const NAMES: [&str; 2] = ["AGENTS.md", "AGENTS.override.md"];

fn read(path: &Path) -> Result<Option<String>, String> {
    managed_files::text(path)
}

fn strip_block(text: &str, begin: &str, end_marker: &str) -> Result<String, String> {
    match (text.find(begin), text.find(end_marker)) {
        (None, None) => Ok(text.to_string()),
        (Some(start), Some(end)) if end > start => {
            let finish = end + end_marker.len();
            if text[start + begin.len()..end].contains(begin)
                || text[finish..].contains(begin) || text[finish..].contains(end_marker) {
                return Err("全局任务指令中的图片规则标记重复，未覆盖。".into());
            }
            // The prefix and its separator are always inserted together.
            let tail = text[finish..].strip_prefix("\n\n").unwrap_or(&text[finish..]);
            Ok(format!("{}{tail}", &text[..start]))
        }
        _ => Err("全局任务指令中的图片规则标记损坏，未覆盖。".into()),
    }
}

fn strip(text: &str) -> Result<String, String> {
    strip_block(&strip_block(text, LEGACY_BEGIN, LEGACY_END)?, BEGIN, END)
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
        .is_some_and(|text| text.starts_with(RULES.trim_end()))
}

pub(super) fn prepare(home: &Path) -> Result<(PathBuf, Option<String>, String), String> {
    validate_restore(home)?;
    let path = target(home)?;
    let previous = read(&path)?;
    let remaining = strip(previous.as_deref().unwrap_or(""))?;
    let next = format!("{}\n\n{remaining}", RULES.trim_end());
    Ok((path, previous, next))
}

pub(super) fn write_with_config(home: &Path, config: &Path, rendered: &str) -> Result<(), String> {
    let (agents, _, next) = prepare(home)?;
    let transaction = managed_files::Snapshot::capture(home, &["config.toml", "AGENTS.md", "AGENTS.override.md"])?;
    backup_file(&agents)?;
    let operation = (|| {
        // Remove our old block from the inactive file as well; never duplicate rules.
        for name in NAMES {
            let path = home.join(name);
            if path == agents { continue; }
            if let Some(content) = read(&path)? {
                let remaining = strip(&content)?;
                if remaining != content {
                    backup_file(&path)?;
                    write_private_atomic(&path, remaining.as_bytes())
                        .map_err(|_| "无法迁移旧图片任务指令。")?;
                }
            }
        }
        write_private_atomic(&agents, next.as_bytes()).map_err(|_| "无法保存全局图片任务指令。")?;
        write_private_atomic(config, rendered.as_bytes()).map_err(|_| "无法保存 config.toml。")?;
        verify_config_write(config, rendered)?;
        if read(&agents)?.as_deref() != Some(next.as_str()) {
            return Err("全局图片任务指令回读不一致。".into());
        }
        Ok(())
    })();
    if let Err(error) = operation {
        return Err(transaction.rollback(error));
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

pub(super) fn validate_restore(home: &Path) -> Result<(), String> {
    for name in NAMES {
        if let Some(content) = read(&home.join(name))? { strip(&content)?; }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserve_existing_instructions_and_later_edits_exactly() {
        let original = "Keep my settings.\r\n";
        let merged = format!("{}\n\n{original}", RULES.trim_end());
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
