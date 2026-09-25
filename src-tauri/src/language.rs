//! Native locale persistence, independent of provider and authentication restore.
//!
//! Callers must serialize configuration writers across processes, recheck the
//! installed host version, and stop the exact desktop AND backend before calling
//! apply_pending. This module never controls processes or changes feature gates.
//! Atomic file replacements plus a pending receipt make interrupted commits
//! recoverable; Snapshot also rolls back ordinary I/O/readback failures.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use toml_edit::{value, DocumentMut, Item, Table};

use super::{backup_file, managed_files, read_config_for_write, write_private_atomic};

const CONFIG: &str = "config.toml";
const RECEIPT: &str = "oceanway-language.json";
const OWNER: &str = "oceanway-config";
const SCHEMA: u32 = 1;
const CHINESE: &str = "zh-CN";
static MUTATION: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageStatus {
    /// Compatibility of the supplied/recorded version, not live host detection.
    pub supported: bool,
    /// The effective saved locale is Chinese, not proof of translated UI.
    pub applied: bool,
    pub verified: bool,
    pub pending: bool,
    pub managed: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Phase {
    PendingApply,
    Applied,
    PendingRestore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    owner: String,
    host_version: String,
    canonical_home: PathBuf,
    #[serde(deserialize_with = "required_optional_string")]
    original: Option<String>,
    // Only relevant when the native TOML key was absent. Never copy global state.
    #[serde(deserialize_with = "required_optional_string")]
    legacy_original: Option<String>,
    applied: String,
    phase: Phase,
}

fn required_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

fn supported_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    (3..=4).contains(&parts.len())
        && parts[0] == "26"
        && parts[1] == "917"
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u32>().is_ok()
        })
}

fn canonical_home(home: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(home)
        .map_err(|_| "Cannot inspect the language configuration home.")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Language configuration home must be an existing, unlinked directory.".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Refusing a reparse-point language configuration home.".into());
        }
    }
    fs::canonicalize(home).map_err(|_| "Cannot resolve the language configuration home.".into())
}

fn validate_locale(locale: &str) -> Result<(), String> {
    // Fail closed on unusual data rather than recording arbitrary text as locale.
    if locale.len() > 128
        || !locale
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("The saved locale is not a supported locale identifier.".into());
    }
    Ok(())
}

fn locale(document: &DocumentMut) -> Result<Option<String>, String> {
    let Some(desktop) = document.get("desktop") else {
        return Ok(None);
    };
    let table = desktop
        .as_table_like()
        .ok_or("The desktop configuration must be a TOML table.")?;
    table
        .get("localeOverride")
        .map(|item| {
            let text = item
                .as_str()
                .ok_or("desktop.localeOverride must be a string.")?;
            validate_locale(text)?;
            Ok(text.to_string())
        })
        .transpose()
}

fn render_locale(original: &str, desired: Option<&str>) -> Result<String, String> {
    let mut document = original
        .parse::<DocumentMut>()
        .map_err(|_| "Cannot parse config.toml for the language operation.")?;
    locale(&document)?;
    if document.get("desktop").is_none() && desired.is_some() {
        document["desktop"] = Item::Table(Table::new());
    }
    if let Some(desktop) = document.get_mut("desktop") {
        let table = desktop
            .as_table_like_mut()
            .ok_or("The desktop configuration must be a TOML table.")?;
        match desired {
            Some(desired) => {
                validate_locale(desired)?;
                table.insert("localeOverride", value(desired));
            }
            None => {
                table.remove("localeOverride");
            }
        }
    }
    Ok(document.to_string())
}

fn legacy_locale(home: &Path) -> Result<Option<String>, String> {
    struct LocaleVisitor;
    impl<'de> serde::de::Visitor<'de> for LocaleVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a legacy settings object")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            let mut locale = None;
            let mut seen = false;
            while let Some(key) = map.next_key::<String>()? {
                if key == "localeOverride" {
                    if seen {
                        return Err(serde::de::Error::duplicate_field("localeOverride"));
                    }
                    seen = true;
                    locale = map.next_value::<Option<String>>()?;
                } else {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
            Ok(locale)
        }
    }
    // Unknown fields are skipped by serde and are never included in our receipt,
    // messages or backups. Do not load auth.json, sessions or Electron userData.
    let Some(bytes) = managed_files::read_optional(&home.join(".codex-global-state.json"))? else {
        return Ok(None);
    };
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let selected = serde::Deserializer::deserialize_map(&mut deserializer, LocaleVisitor)
        .map_err(|_| "Cannot safely determine the legacy locale; no language change was made.")?;
    deserializer.end()
        .map_err(|_| "The legacy settings object has trailing data; no language change was made.")?;
    // Deliberately fail closed on corrupt legacy JSON instead of guessing which
    // .bak the desktop would migrate. Leave both legacy files untouched.
    if let Some(locale) = &selected {
        validate_locale(locale)?;
    }
    Ok(selected)
}

struct State {
    home: PathBuf,
    canonical: PathBuf,
    config: String,
    config_bytes: Option<Vec<u8>>,
    receipt_bytes: Option<Vec<u8>>,
    receipt: Option<Receipt>,
    current: Option<String>,
    legacy: Option<String>,
}

impl State {
    fn load(home: &Path) -> Result<Self, String> {
        let canonical = canonical_home(home)?;
        let receipt_bytes = managed_files::read_optional(&home.join(RECEIPT))?;
        let receipt = receipt_bytes
            .as_deref()
            .map(|bytes| -> Result<Receipt, String> {
                let receipt: Receipt = serde_json::from_slice(bytes)
                    .map_err(|_| "The language ownership receipt is corrupt; it was not replaced.")?;
                if receipt.schema_version != SCHEMA
                    || receipt.owner != OWNER
                    || !supported_version(&receipt.host_version)
                    || receipt.canonical_home != canonical
                    || receipt.applied != CHINESE
                    || receipt.original.as_deref() == Some(CHINESE)
                    || (receipt.original.is_some() && receipt.legacy_original.is_some())
                    || (receipt.original.is_none()
                        && receipt.legacy_original.as_deref() == Some(CHINESE))
                {
                    return Err("The language receipt has an unsupported owner, version, home or value.".into());
                }
                for text in [&receipt.original, &receipt.legacy_original].into_iter().flatten() {
                    validate_locale(text)?;
                }
                Ok(receipt)
            })
            .transpose()?;
        let config = read_config_for_write(&home.join(CONFIG))?;
        let config_bytes = managed_files::read_optional(&home.join(CONFIG))?;
        if config_bytes.as_deref().unwrap_or_default() != config.as_bytes() {
            return Err("Configuration changed during language preflight; retry after writers stop.".into());
        }
        let document = config
            .parse::<DocumentMut>()
            .map_err(|_| "Cannot parse config.toml for the language operation.")?;
        let current = locale(&document)?;
        let legacy = if current.is_none() {
            legacy_locale(home)?
        } else {
            None
        };
        Ok(Self {
            home: home.to_path_buf(),
            canonical,
            config,
            config_bytes,
            receipt_bytes,
            receipt,
            current,
            legacy,
        })
    }

    fn unchanged(&self) -> Result<(), String> {
        if canonical_home(&self.home)? != self.canonical
            || managed_files::read_optional(&self.home.join(CONFIG))? != self.config_bytes
            || managed_files::read_optional(&self.home.join(RECEIPT))? != self.receipt_bytes
            || (self.current.is_none() && legacy_locale(&self.home)? != self.legacy)
        {
            return Err("Language inputs changed after preflight; no pending change was committed.".into());
        }
        Ok(())
    }

    fn matches_original(&self, receipt: &Receipt) -> bool {
        self.current == receipt.original
            && (self.current.is_some() || self.legacy == receipt.legacy_original)
    }

    fn status(&self) -> LanguageStatus {
        let applied = self.current.as_ref().or(self.legacy.as_ref()).map(String::as_str)
            == Some(CHINESE);
        let pending = self.receipt.as_ref().is_some_and(|receipt| receipt.phase != Phase::Applied);
        let conflict = self.receipt.as_ref().is_some_and(|receipt| match receipt.phase {
            Phase::PendingApply => {
                !self.matches_original(receipt) && self.current.as_deref() != Some(CHINESE)
            }
            Phase::PendingRestore => {
                self.current != receipt.original && self.current.as_deref() != Some(CHINESE)
            }
            Phase::Applied => false,
        });
        let message = match self.receipt.as_ref().map(|receipt| receipt.phase) {
            Some(_) if conflict => "待执行语言设置与后来的修改冲突，未覆盖用户选择。",
            Some(Phase::PendingApply) => "中文设置已准备，等待桌面及后端退出后保存。",
            Some(Phase::PendingRestore) => "原语言恢复已准备，等待桌面及后端退出后保存。",
            Some(Phase::Applied) if self.current.as_deref() == Some(CHINESE) => {
                "中文设置已保存，实际界面尚待确认。"
            }
            Some(Phase::Applied) => "语言已被用户另行修改，本工具不会覆盖。",
            None if applied => "当前已选择中文，非本工具修改；实际界面尚待确认。",
            None => "本工具尚未修改语言，中文界面未验证。",
        };
        LanguageStatus {
            supported: self.receipt.is_some(),
            applied,
            verified: false,
            pending,
            managed: self.receipt.is_some(),
            message: message.into(),
        }
    }
}

fn commit(state: &State, config: Option<&str>, receipt: Option<&Receipt>) -> Result<(), String> {
    let rendered_receipt = receipt
        .map(|receipt| {
            serde_json::to_string_pretty(receipt)
                .map(|text| format!("{text}\n"))
                .map_err(|_| "Cannot serialize the language receipt.".to_string())
        })
        .transpose()?;
    state.unchanged()?;
    let names: &[&str] = if config.is_some() { &[CONFIG, RECEIPT] } else { &[RECEIPT] };
    let snapshot = managed_files::Snapshot::capture(&state.home, names)?;
    if config.is_some() {
        backup_file(&state.home.join(CONFIG))?;
    }
    backup_file(&state.home.join(RECEIPT))?;
    // Do not roll back someone else's edit detected before the first write.
    state.unchanged()?;
    let result = (|| -> Result<(), String> {
        if let Some(config) = config {
            write_private_atomic(&state.home.join(CONFIG), config.as_bytes())
                .map_err(|_| "Cannot persist the native language setting.")?;
            if read_config_for_write(&state.home.join(CONFIG))? != config {
                return Err("Native language configuration readback did not match.".into());
            }
        }
        let receipt_path = state.home.join(RECEIPT);
        match &rendered_receipt {
            Some(text) => write_private_atomic(&receipt_path, text.as_bytes())
                .map_err(|_| "Cannot persist the language receipt.")?,
            None => fs::remove_file(&receipt_path).map_err(|_| "Cannot remove the language receipt.")?,
        }
        if managed_files::text(&receipt_path)?.as_deref() != rendered_receipt.as_deref() {
            return Err("Language receipt readback did not match.".into());
        }
        if let Some(config) = config {
            if read_config_for_write(&state.home.join(CONFIG))? != config {
                return Err("Native language configuration changed during commit.".into());
            }
        }
        Ok(())
    })();
    result.map_err(|cause| snapshot.rollback(cause))
}

/// Unchecking withdraws an uncommitted apply, but never restores an applied locale.
pub fn prepare(
    home: &Path,
    enable: bool,
    host_version: Option<&str>,
) -> Result<LanguageStatus, String> {
    let _guard = MUTATION.lock().map_err(|_| "Language transaction lock is unavailable.")?;
    let state = State::load(home)?;
    let mut status = state.status();
    status.supported = host_version.is_some_and(supported_version);
    if !enable {
        if let Some(receipt) = &state.receipt {
            if receipt.phase == Phase::PendingApply {
                if state.matches_original(receipt) {
                    commit(&state, None, None)?;
                    return Ok(State::load(home)?.status());
                }
                if state.current.as_deref() == Some(CHINESE) {
                    let mut applied = receipt.clone();
                    applied.phase = Phase::Applied;
                    commit(&state, None, Some(&applied))?;
                    return Ok(State::load(home)?.status());
                }
                return Err("The pending language value changed; no user edit was overwritten.".into());
            }
        }
        return Ok(status);
    }
    if !status.supported {
        status.message = "当前版本尚不支持此语言适配；中文界面未应用，供应商与图片配置仍可使用。".into();
        return Ok(status);
    }
    if let Some(receipt) = &state.receipt {
        match receipt.phase {
            Phase::PendingRestore => return Err("Finish the pending language restore before applying Chinese again.".into()),
            Phase::Applied if state.current.as_deref() != Some(CHINESE) => {
                return Err("The user changed the owned language value; refusing to overwrite it.".into());
            }
            Phase::PendingApply
                if !state.matches_original(receipt) && state.current.as_deref() != Some(CHINESE) =>
            {
                return Err("The saved language changed after staging; refusing to overwrite it.".into());
            }
            _ => return Ok(status),
        }
    }
    if status.applied {
        status.message = "原本已选择中文，未创建语言恢复记录；实际界面尚待确认。".into();
        return Ok(status);
    }
    let receipt = Receipt {
        schema_version: SCHEMA,
        owner: OWNER.into(),
        host_version: host_version.ok_or("Missing desktop version.")?.into(),
        canonical_home: state.canonical.clone(),
        original: state.current.clone(),
        legacy_original: state.legacy.clone(),
        applied: CHINESE.into(),
        phase: Phase::PendingApply,
    };
    commit(&state, None, Some(&receipt))?;
    Ok(State::load(home)?.status())
}

/// Only an exact owned native value may be scheduled for restoration.
pub fn prepare_restore(home: &Path) -> Result<LanguageStatus, String> {
    let _guard = MUTATION.lock().map_err(|_| "Language transaction lock is unavailable.")?;
    let state = State::load(home)?;
    let Some(mut receipt) = state.receipt.clone() else {
        return Ok(state.status());
    };
    if state.current.as_deref() != Some(CHINESE) {
        return Err("The current native language is not the owned Chinese value; nothing was staged.".into());
    }
    if receipt.phase != Phase::PendingRestore {
        receipt.phase = Phase::PendingRestore;
        commit(&state, None, Some(&receipt))?;
    }
    Ok(State::load(home)?.status())
}

/// Must run AFTER the caller confirms desktop/backend exit and revalidates the
/// live host version. The saved version is an allowlist check, not live detection.
pub fn apply_pending(home: &Path) -> Result<(), String> {
    let _guard = MUTATION.lock().map_err(|_| "Language transaction lock is unavailable.")?;
    let state = State::load(home)?;
    let Some(mut receipt) = state.receipt.clone() else {
        return Ok(());
    };
    match receipt.phase {
        Phase::Applied => Ok(()),
        Phase::PendingApply => {
            let config = if state.current.as_deref() == Some(CHINESE) {
                // Recover a crash after config replacement but before receipt finalization.
                None
            } else if state.matches_original(&receipt) {
                Some(render_locale(&state.config, Some(CHINESE))?)
            } else {
                return Err("The language changed after staging; refusing to apply the pending value.".into());
            };
            receipt.phase = Phase::Applied;
            commit(&state, config.as_deref(), Some(&receipt))
        }
        Phase::PendingRestore => {
            let config = if state.current == receipt.original {
                // A prior restore wrote the original field but did not retire its receipt.
                None
            } else if state.current.as_deref() == Some(CHINESE) {
                Some(render_locale(&state.config, receipt.original.as_deref())?)
            } else {
                return Err("The language changed after staging; refusing to restore over it.".into());
            };
            commit(&state, config.as_deref(), None)
        }
    }
}

pub fn apply_pending_checked(home: &Path, host_version: Option<&str>) -> Result<(), String> {
    let state = State::load(home)?;
    if state.receipt.as_ref().is_some_and(|receipt| receipt.phase != Phase::Applied)
        && !host_version.is_some_and(supported_version)
    {
        return Err("The launch target is no longer a supported desktop version; pending language was not applied.".into());
    }
    apply_pending(home)
}

pub fn status(home: &Path) -> Result<LanguageStatus, String> {
    let _guard = MUTATION.lock().map_err(|_| "Language transaction lock is unavailable.")?;
    if !home.exists() {
        return Ok(LanguageStatus { supported: false, applied: false, verified: false,
            pending: false, managed: false, message: "中文界面尚未配置。".into() });
    }
    Ok(State::load(home)?.status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const VERSION: &str = "26.917.9434.0";

    struct Home(PathBuf);

    impl Home {
        fn new() -> Self {
            static SEQUENCE: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "oceanway-language-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn write(&self, name: &str, content: &str) {
            fs::write(self.0.join(name), content).unwrap();
        }
        fn read(&self, name: &str) -> String {
            fs::read_to_string(self.0.join(name)).unwrap()
        }
        fn locale(&self) -> Option<String> {
            locale(&self.read(CONFIG).parse::<DocumentMut>().unwrap()).unwrap()
        }
        fn stage(&self) -> LanguageStatus {
            prepare(&self.0, true, Some(VERSION)).unwrap()
        }
        fn apply(&self) {
            self.stage();
            apply_pending(&self.0).unwrap();
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            // Only remove the exact, exclusively created fixture directory.
            if let (Ok(actual), Ok(temporary)) = (
                fs::canonicalize(&self.0),
                fs::canonicalize(std::env::temp_dir()),
            ) {
                if actual.parent() == Some(temporary.as_path())
                    && actual.file_name().is_some_and(|name| {
                        name.to_string_lossy().starts_with("oceanway-language-")
                    })
                {
                    let _ = fs::remove_dir_all(actual);
                }
            }
        }
    }

    #[test]
    fn version_gate_is_bounded_and_unknown_versions_do_not_write() {
        for version in [Some("26.916.1.0"), Some("26.917evil"), Some("26.917"),
            Some("26.917.1-beta"), Some("126.917.1"), Some("26.918.1.0"), None] {
            let home = Home::new();
            let result = prepare(&home.0, true, version).unwrap();
            assert!(!result.supported && !result.pending && !result.verified);
            assert_eq!(fs::read_dir(&home.0).unwrap().count(), 0);
        }
        assert!(supported_version(VERSION));
        assert!(supported_version("26.917.9434"));
    }

    #[test]
    fn staging_and_unchecked_option_never_write_native_config() {
        let home = Home::new();
        home.write(CONFIG, "[desktop]\nlocaleOverride = 'en-US'\n");
        let original = home.read(CONFIG);
        let staged = home.stage();
        assert!(staged.pending && staged.managed && staged.supported);
        assert!(!staged.applied && !staged.verified);
        assert_eq!(home.read(CONFIG), original);
        prepare(&home.0, false, Some(VERSION)).unwrap();
        assert_eq!(home.read(CONFIG), original);
        assert!(!home.0.join(RECEIPT).exists());
        apply_pending(&home.0).unwrap();
        assert_eq!(home.read(CONFIG), original);
    }

    #[test]
    fn repeat_apply_preserves_first_original_and_unchecked_keeps_chinese() {
        let home = Home::new();
        home.write(CONFIG, "[desktop]\nlocaleOverride = 'fr-FR'\n");
        home.stage();
        let staged = home.read(RECEIPT);
        home.stage();
        assert_eq!(home.read(RECEIPT), staged);
        apply_pending(&home.0).unwrap();
        let applied = home.read(RECEIPT);
        home.apply();
        prepare(&home.0, false, Some(VERSION)).unwrap();
        assert_eq!(home.read(RECEIPT), applied);
        let receipt: Receipt = serde_json::from_str(&applied).unwrap();
        assert_eq!(receipt.original.as_deref(), Some("fr-FR"));
        let status = status(&home.0).unwrap();
        assert!(status.applied && status.managed && !status.pending && !status.verified);
    }

    #[test]
    fn restore_only_changes_locale_and_preserves_later_settings() {
        let home = Home::new();
        home.write(CONFIG, "model = 'before'\n[desktop]\nlocaleOverride = 'de-DE'\n");
        home.apply();
        home.write(CONFIG, "# Later user edit\nmodel = 'after'\n[desktop]\nlocaleOverride = 'zh-CN'\ntheme = 'dark'\n");
        let current = home.read(CONFIG);
        let result = prepare_restore(&home.0).unwrap();
        assert!(result.pending && result.applied && !result.verified);
        assert_eq!(home.read(CONFIG), current);
        apply_pending(&home.0).unwrap();
        assert_eq!(home.locale().as_deref(), Some("de-DE"));
        assert!(home.read(CONFIG).contains("# Later user edit"));
        assert!(home.read(CONFIG).contains("model = 'after'"));
        assert!(home.read(CONFIG).contains("theme = 'dark'"));
        assert!(!home.0.join(RECEIPT).exists());
        assert!(!prepare_restore(&home.0).unwrap().managed);
    }

    #[test]
    fn originally_missing_locale_restores_absence_not_the_desktop_snapshot() {
        let home = Home::new();
        home.apply();
        home.write(CONFIG, "[desktop]\nlocaleOverride = 'zh-CN'\nfontSize = 17\n");
        prepare_restore(&home.0).unwrap();
        apply_pending(&home.0).unwrap();
        assert_eq!(home.locale(), None);
        assert!(home.read(CONFIG).contains("fontSize = 17"));
        assert!(!home.0.join(RECEIPT).exists());
    }

    #[test]
    fn existing_chinese_native_or_legacy_is_not_claimed() {
        for native in [true, false] {
            let home = Home::new();
            if native {
                home.write(CONFIG, "[desktop]\nlocaleOverride = 'zh-CN'\n");
            } else {
                home.write(".codex-global-state.json", r#"{"localeOverride":"zh-CN","other":{"ignored":true}}"#);
            }
            let result = home.stage();
            assert!(result.applied && result.supported && !result.managed && !result.pending);
            assert!(!home.0.join(RECEIPT).exists());
        }
    }

    #[test]
    fn native_key_wins_over_legacy_and_neither_legacy_nor_auth_is_modified() {
        let home = Home::new();
        home.write(CONFIG, "[desktop]\nlocaleOverride = 'ja-JP'\n");
        let legacy = r#"{"localeOverride":"zh-CN","unrelated":"DO_NOT_COPY"}"#;
        home.write(".codex-global-state.json", legacy);
        home.write("auth.json", "deliberately-not-json");
        home.apply();
        assert!(!home.read(RECEIPT).contains("DO_NOT_COPY"));
        prepare_restore(&home.0).unwrap();
        apply_pending(&home.0).unwrap();
        assert_eq!(home.locale().as_deref(), Some("ja-JP"));
        assert_eq!(home.read(".codex-global-state.json"), legacy);
        assert_eq!(home.read("auth.json"), "deliberately-not-json");
    }

    #[test]
    fn legacy_only_previous_locale_is_not_copied_into_native_setting_on_restore() {
        let home = Home::new();
        let legacy = r#"{"localeOverride":"es-ES","unrelated":"DO_NOT_COPY"}"#;
        home.write(".codex-global-state.json", legacy);
        home.apply();
        let receipt: Receipt = serde_json::from_str(&home.read(RECEIPT)).unwrap();
        assert_eq!(receipt.original, None);
        assert_eq!(receipt.legacy_original.as_deref(), Some("es-ES"));
        assert!(!home.read(RECEIPT).contains("DO_NOT_COPY"));
        prepare_restore(&home.0).unwrap();
        apply_pending(&home.0).unwrap();
        assert_eq!(home.locale(), None);
        assert_eq!(home.read(".codex-global-state.json"), legacy);
    }

    #[test]
    fn later_user_change_blocks_prepare_and_restore() {
        let home = Home::new();
        home.apply();
        home.write(CONFIG, "[desktop]\nlocaleOverride = 'it-IT'\n");
        let config = home.read(CONFIG);
        let receipt = home.read(RECEIPT);
        assert!(prepare(&home.0, true, Some(VERSION)).is_err());
        assert!(prepare_restore(&home.0).is_err());
        apply_pending(&home.0).unwrap();
        assert_eq!(home.read(CONFIG), config);
        assert_eq!(home.read(RECEIPT), receipt);
        assert!(!status(&home.0).unwrap().applied);
    }

    #[test]
    fn changes_after_staging_are_not_overwritten() {
        for restore in [true, false] {
            let home = Home::new();
            if restore {
                home.apply();
                prepare_restore(&home.0).unwrap();
            } else {
                home.stage();
            }
            home.write(CONFIG, "[desktop]\nlocaleOverride = 'it-IT'\n");
            let receipt = home.read(RECEIPT);
            assert!(apply_pending(&home.0).is_err());
            assert_eq!(home.locale().as_deref(), Some("it-IT"));
            assert_eq!(home.read(RECEIPT), receipt);
        }
    }

    #[test]
    fn legacy_change_after_staging_is_not_overwritten() {
        let home = Home::new();
        home.stage();
        home.write(".codex-global-state.json", r#"{"localeOverride":"fr-FR"}"#);
        assert!(apply_pending(&home.0).is_err());
        assert!(!home.0.join(CONFIG).exists());
    }

    #[test]
    fn receipt_is_bound_to_canonical_home() {
        let first = Home::new();
        let second = Home::new();
        first.stage();
        second.write(RECEIPT, &first.read(RECEIPT));
        assert!(status(&second.0).is_err());
        assert!(apply_pending(&second.0).is_err());
        assert!(!second.0.join(CONFIG).exists());
    }

    #[test]
    fn linked_configuration_and_receipt_are_rejected_before_writes() {
        for name in [CONFIG, RECEIPT, ".codex-global-state.json"] {
            let home = Home::new();
            if name == RECEIPT {
                home.stage();
            } else if name == CONFIG {
                home.write(name, "[desktop]\nlocaleOverride = 'en-US'\n");
            } else {
                home.write(name, r#"{"localeOverride":"en-US"}"#);
            }
            let original = home.read(name);
            fs::hard_link(home.0.join(name), home.0.join("linked-fixture")).unwrap();
            assert!(prepare(&home.0, true, Some(VERSION)).is_err());
            assert_eq!(home.read(name), original);
            if name != RECEIPT {
                assert!(!home.0.join(RECEIPT).exists());
            }
        }
    }

    #[test]
    fn corrupt_receipt_or_wrong_schema_never_gets_replaced() {
        for receipt in ["{", "{}", r#"{"schemaVersion":999}"#] {
            let home = Home::new();
            home.write(RECEIPT, receipt);
            assert!(prepare(&home.0, true, Some(VERSION)).is_err());
            assert!(prepare_restore(&home.0).is_err());
            assert!(apply_pending(&home.0).is_err());
            assert_eq!(home.read(RECEIPT), receipt);
            assert!(!home.0.join(CONFIG).exists());
        }
        let home = Home::new();
        home.stage();
        let mut receipt: serde_json::Value = serde_json::from_str(&home.read(RECEIPT)).unwrap();
        receipt.as_object_mut().unwrap().remove("original");
        home.write(RECEIPT, &receipt.to_string());
        assert!(apply_pending(&home.0).is_err());
    }

    #[test]
    fn invalid_native_or_legacy_data_fails_closed() {
        for config in ["[desktop", "desktop = 3", "[desktop]\nlocaleOverride = true"] {
            let home = Home::new();
            home.write(CONFIG, config);
            assert!(prepare(&home.0, true, Some(VERSION)).is_err());
            assert_eq!(home.read(CONFIG), config);
            assert!(!home.0.join(RECEIPT).exists());
        }
        for legacy in ["{", r#"["zh-CN"]"#, r#"{"localeOverride":42}"#,
            r#"{"localeOverride":"en-US","localeOverride":"zh-CN"}"#] {
            let home = Home::new();
            home.write(".codex-global-state.json", legacy);
            assert!(prepare(&home.0, true, Some(VERSION)).is_err());
            assert!(!home.0.join(RECEIPT).exists());
        }
    }

    #[test]
    fn inline_desktop_table_round_trips_without_losing_other_fields() {
        let original = "desktop = { localeOverride = 'en-US', theme = 'dark' }\n";
        let applied = render_locale(original, Some(CHINESE)).unwrap();
        assert!(applied.contains("theme = 'dark'"));
        let restored = render_locale(&applied, None).unwrap();
        assert!(restored.contains("theme = 'dark'"));
        assert_eq!(locale(&restored.parse::<DocumentMut>().unwrap()).unwrap(), None);
    }

    #[test]
    fn interrupted_apply_and_restore_finalize_without_overwriting_other_fields() {
        let home = Home::new();
        home.write(CONFIG, "[desktop]\nlocaleOverride = 'en-US'\n");
        home.stage();
        home.write(CONFIG, "[desktop]\nlocaleOverride = 'zh-CN'\ntheme = 'dark'\n");
        let config = home.read(CONFIG);
        apply_pending(&home.0).unwrap();
        assert_eq!(home.read(CONFIG), config);
        assert!(!status(&home.0).unwrap().pending);
        prepare_restore(&home.0).unwrap();
        home.write(CONFIG, "[desktop]\nlocaleOverride = 'en-US'\ntheme = 'light'\n");
        let config = home.read(CONFIG);
        apply_pending(&home.0).unwrap();
        assert_eq!(home.read(CONFIG), config);
        assert!(!home.0.join(RECEIPT).exists());
    }

    #[test]
    fn status_serialization_is_camel_case_and_never_claims_ui_verification() {
        let home = Home::new();
        home.apply();
        let status = serde_json::to_value(status(&home.0).unwrap()).unwrap();
        assert_eq!(status["verified"], false);
        assert_eq!(status["applied"], true);
        assert_eq!(status["pending"], false);
        assert_eq!(status["managed"], true);
    }
}
