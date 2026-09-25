use super::*;

const BEGIN: &str = "<!-- OCEANWAY:DIRECT-IMAGE-API:BEGIN -->";
const END: &str = "<!-- OCEANWAY:DIRECT-IMAGE-API:END -->";
pub(super) const RULES: &str = include_str!("direct-image-instructions.md");

fn without_managed_block(instructions: &str) -> Result<String, String> {
    match (instructions.find(BEGIN), instructions.find(END)) {
        (None, None) => Ok(instructions.to_string()),
        (Some(start), Some(end)) if end > start => {
            let finish = end + END.len();
            if instructions[start + BEGIN.len()..end].contains(BEGIN)
                || instructions[finish..].contains(BEGIN) || instructions[finish..].contains(END) {
                return Err("发现重复的直接图片规则标记，未修改原始指令。".into());
            }
            // Only the marker-delimited block belongs to this application.
            Ok(format!("{}{}", &instructions[..start], &instructions[finish..]))
        }
        _ => Err("直接图片规则标记不完整，未修改原始指令。".into()),
    }
}

pub(super) fn merge(content: &str) -> Result<String, String> {
    let mut doc = content.parse::<DocumentMut>()
        .map_err(|_| "config.toml 无法解析，未写入直接图片规则。".to_string())?;
    let existing = match doc.get("developer_instructions") {
        Some(item) => item.as_str()
            .ok_or("developer_instructions 不是字符串，未覆盖现有值。")?,
        None => "",
    };
    let next = if let (Some(start), Some(end)) = (existing.find(BEGIN), existing.find(END)) {
        without_managed_block(existing)?;
        format!("{}{}{}", &existing[..start], RULES.trim_end(), &existing[end + END.len()..])
    } else {
        without_managed_block(existing)?;
        let separator = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
        format!("{existing}{separator}{}", RULES.trim_end())
    };
    doc["developer_instructions"] = value(next);
    remove_owned_legacy_header(&mut doc)?;
    Ok(doc.to_string())
}

fn remove_owned_legacy_header(doc: &mut DocumentMut) -> Result<(), String> {
    let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) else {
        return Ok(());
    };
    let Some(provider) = providers.get_mut(PROVIDER_ID).and_then(Item::as_table_mut) else {
        return Ok(());
    };
    if let Some(headers) = provider.get_mut("http_headers").and_then(Item::as_table_like_mut) {
        if headers.get("x-openai-actor-authorization").and_then(Item::as_str)
            == Some(IMAGE_EXTENSION_ACTOR_AUTHORIZATION)
        {
            headers.remove("x-openai-actor-authorization");
        }
        if headers.is_empty() { provider.remove("http_headers"); }
    }
    Ok(())
}

pub(super) fn configured(content: &str) -> bool {
    read_root_string(content, "developer_instructions")
        .is_some_and(|text| text.contains(RULES.trim_end()))
}

pub(super) fn remove(content: &str) -> Result<String, String> {
    let mut doc = content.parse::<DocumentMut>()
        .map_err(|_| "config.toml 无法解析，未撤销规则。".to_string())?;
    if let Some(item) = doc.get("developer_instructions") {
        let existing = item.as_str().ok_or("developer_instructions 不是字符串，未覆盖现有值。")?;
        let remaining = without_managed_block(existing)?;
        if remaining != existing {
            if remaining.is_empty() { doc.remove("developer_instructions"); }
            else { doc["developer_instructions"] = value(remaining); }
        }
    }
    remove_owned_legacy_header(&mut doc)?;
    Ok(doc.to_string())
}

pub(super) fn migrate(content: &str, auth_key: Option<&str>) -> Result<String, String> {
    let token = read_provider_bearer_token(content, PROVIDER_ID);
    let base = read_provider_base_url(content, PROVIDER_ID);
    let owned = configured(content);
    let cleaned = if owned && has_matching_direct_http_environment(
        content, token.as_deref().or(auth_key), base.as_deref(),
    ) {
        remove_direct_http_environment(content)?
    } else { content.to_owned() };
    remove(&cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_rules_preserve_multiline_toml_and_are_idempotent() {
        let original = "developer_instructions = '''Keep this.\n[fake.table]\nmodel = \"not-real\"\n'''\nmodel = \"real\"\n";
        let first = merge(original).unwrap();
        let second = merge(&first).unwrap();
        assert_eq!(first, second);
        assert!(configured(&second));
        assert_eq!(read_current_model_from_content(&second).as_deref(), Some("real"));
        let restored = remove(&second).unwrap();
        assert_eq!(read_root_string(&restored, "developer_instructions"),
            read_root_string(original, "developer_instructions"));
    }

    #[test]
    fn broken_rules_or_wrong_type_are_not_overwritten() {
        assert!(merge("developer_instructions = 42").is_err());
        assert!(merge(&format!("developer_instructions = '{}'", BEGIN)).is_err());
        assert!(merge("[invalid").is_err());
    }

    #[test]
    fn migration_only_removes_owned_legacy_header() {
        let original = "[model_providers.OceanWay]\nhttp_headers = { x-openai-actor-authorization = 'local-image-extension', other = 'keep' }\n";
        let rendered = merge(original).unwrap();
        assert!(!rendered.contains("local-image-extension"));
        assert!(rendered.contains("keep"));
        let custom = original.replace("local-image-extension", "custom");
        assert!(merge(&custom).unwrap().contains("custom"));
    }

    #[test]
    fn updated_credentials_do_not_enter_rules() {
        let config = merge(&merge_direct_http_environment("", "fake-test-secret", DEFAULT_BASE_URL).unwrap()).unwrap();
        let instructions = read_root_string(&config, "developer_instructions").unwrap();
        assert!(!instructions.contains("fake-test-secret"));
        assert!(instructions.contains("gpt-image-2"));
        assert!(instructions.contains("image[]"));
        assert!(instructions.contains("at most two"));
    }
}
