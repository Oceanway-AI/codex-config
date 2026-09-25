use super::*;
#[test]
fn logged_in_keyring_user_gets_file_api_auth_and_keeps_original_tokens() {
    let auth = render_auth_json_content(
        r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"access_token":"fake-account-token"},"keep":true}"#,
        "fake-provider-key", ProviderAuthStrategy::ApiKey,
    ).unwrap();
    let parsed: Value = serde_json::from_str(&auth).unwrap();
    assert_eq!(parsed["OPENAI_API_KEY"], "fake-provider-key");
    assert!(parsed.get("auth_mode").is_none());
    assert_eq!(parsed["tokens"]["access_token"], "fake-account-token");
    assert_eq!(parsed["keep"], true);
    let config = merge_config(
        "cli_auth_credentials_store = 'keyring'\nforced_login_method = 'chatgpt'\ndeveloper_instructions = '''Keep [my instructions].'''\n",
        PROVIDER_ID, "https://example.invalid/v1", "gpt-5.6-sol", None, ProviderAuthStrategy::ApiKey,
    ).unwrap();
    assert_eq!(read_root_string(&config, "cli_auth_credentials_store").as_deref(), Some("file"));
    assert_eq!(read_root_string(&config, "forced_login_method").as_deref(), Some("api"));
    assert_eq!(read_provider_bool(&config, PROVIDER_ID, "requires_openai_auth"), Some(true));
    assert!(!config.contains("experimental_bearer_token"));
    assert!(!config.contains("local-image-extension"));
    assert_eq!(read_root_string(&config, "developer_instructions").as_deref(), Some("Keep [my instructions]."));
}

#[test]
fn broken_agent_rules_do_not_write_config_or_auth() {
    let dir = fixture("broken-agent-rules");
    let config = dir.join("config.toml");
    let auth = dir.join("auth.json");
    fs::write(&config, "model='old'\n").unwrap();
    fs::write(&auth, "{\"OPENAI_API_KEY\":\"fake-old\"}").unwrap();
    fs::write(dir.join("AGENTS.md"), "<!-- OCEANWAY:DIRECT-IMAGE-API:BEGIN -->").unwrap();
    assert!(write_config_toml(&config, PROVIDER_ID, "https://example.invalid", "new", None,
        ProviderAuthStrategy::ApiKey, Some("fake-new")).is_err());
    assert_eq!(fs::read_to_string(config).unwrap(), "model='old'\n");
    assert_eq!(fs::read_to_string(auth).unwrap(), "{\"OPENAI_API_KEY\":\"fake-old\"}");
}

#[test]
fn logged_in_user_full_configuration_repeat_key_rotation_and_restore() {
    let dir = fixture("existing-login-full");
    let original_config = "model='gpt-5.6-sol'\ndeveloper_instructions='Keep my developer rules'\n";
    let original_auth = r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"access_token":"fake-original"}}"#;
    fs::write(dir.join("config.toml"), original_config).unwrap();
    fs::write(dir.join("auth.json"), original_auth).unwrap();
    fs::write(dir.join("AGENTS.override.md"), "Original user rules").unwrap();
    for key in ["fake-first", "fake-first", "fake-rotated"] {
        let result = configure_provider_in_home(&dir, key.into(), "https://example.invalid/v1".into()).unwrap();
        assert!(result.direct_image_configured);
        assert_eq!(read_auth_api_key(&dir.join("auth.json")).as_deref(), Some(key));
        let config = fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(mcp_config::status(&dir, &config).configured);
        assert!(agent_rules::configured(&dir));
        assert_eq!(config.matches("OCEANWAY:DIRECT-IMAGE-API:BEGIN").count(), 0);
        assert_eq!(config.matches("[mcp_servers.oceanway_images]").count(), 1);
        assert!(!config.contains("shell_environment_policy"));
        assert!(read_root_string(&config, "developer_instructions").unwrap().contains("Keep my developer rules"));
    }
    let good_config = fs::read(dir.join("config.toml")).unwrap();
    let good_auth = fs::read(dir.join("auth.json")).unwrap();
    let good_agents = fs::read(dir.join("AGENTS.override.md")).unwrap();
    fs::write(dir.join("AGENTS.override.md"), "<!-- OCEANWAY:DIRECT-IMAGE-API:BEGIN -->").unwrap();
    assert!(restore_defaults_in_home(&dir).is_err());
    assert_eq!(fs::read(dir.join("config.toml")).unwrap(), good_config);
    assert_eq!(fs::read(dir.join("auth.json")).unwrap(), good_auth);
    fs::write(dir.join("AGENTS.override.md"), good_agents).unwrap();
    restore_defaults_in_home(&dir).unwrap();
    assert_eq!(fs::read_to_string(dir.join("config.toml")).unwrap(), original_config);
    assert_eq!(fs::read_to_string(dir.join("auth.json")).unwrap(), original_auth);
    assert_eq!(fs::read_to_string(dir.join("AGENTS.override.md")).unwrap(), "Original user rules");
}

#[test]
fn corrupt_config_is_never_replaced() {
    for bytes in [vec![0xff, 0xfe], b"[broken".to_vec()] {
        let dir = fixture("corrupt-config");
        let path = dir.join("config.toml");
        fs::write(&path, &bytes).unwrap();
        assert!(write_config_toml(
            &path,
            PROVIDER_ID,
            "https://example.invalid",
            "test",
            None,
            ProviderAuthStrategy::ApiKey,
            Some("fake")
        )
        .is_err());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn atomic_write_failure_keeps_destination_and_removes_staging_file() {
    let dir = fixture("atomic-failure");
    let destination = dir.join("occupied");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("sentinel"), "keep").unwrap();
    assert!(write_private_atomic(&destination, b"fake").is_err());
    assert_eq!(
        fs::read_to_string(destination.join("sentinel")).unwrap(),
        "keep"
    );
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
}

#[test]
fn atomic_write_replaces_existing_content_privately() {
    let dir = fixture("atomic-replace");
    let path = dir.join("auth.json");
    fs::write(&path, "old").unwrap();
    write_private_atomic(&path, b"new").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(fs::read_dir(dir).unwrap().count(), 1);
}
fn fixture(label: &str) -> PathBuf {
    let p = env::temp_dir().join(format!(
        "oceanway-deep-{label}-{}",
        Local::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn acceptance_real_write_repeat_restore_and_permissions() {
    let dir = fixture("roundtrip");
    let config = dir.join("config.toml");
    let auth = dir.join("auth.json");
    let original = "model = \"original\"\n[unrelated]\nvalue = \"keep\"\n";
    fs::write(&config, original).unwrap();
    fs::write(&auth, "{\"existing\":true}").unwrap();
    ensure_restore_snapshot(&dir, &config, &auth).unwrap();
    write_auth_json(&auth, "fake-test", ProviderAuthStrategy::ApiKey).unwrap();
    write_config_toml(
        &config,
        PROVIDER_ID,
        "https://example.invalid",
        "original",
        None,
        ProviderAuthStrategy::ApiKey,
        Some("fake-test"),
    )
    .unwrap();
    let first = fs::read_to_string(&config).unwrap();
    assert!(first.contains("value = \"keep\""));
    assert!(mcp_config::status(&dir, &first).configured);
    assert!(!first.contains("shell_environment_policy"));
    write_config_toml(
        &config,
        PROVIDER_ID,
        "https://example.invalid",
        "original",
        None,
        ProviderAuthStrategy::ApiKey,
        Some("fake-test"),
    )
    .unwrap();
    let repeated = fs::read_to_string(&config).unwrap();
    assert!(mcp_config::status(&dir, &repeated).configured);
    assert_eq!(repeated.matches("[model_providers.OceanWay]").count(), 1);
    assert_eq!(repeated.matches("[unrelated]").count(), 1);
    assert!(repeated.contains("value = \"keep\""));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&config).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    restore_defaults_in_home(&dir).unwrap();
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    assert_eq!(fs::read_to_string(&auth).unwrap(), "{\"existing\":true}");
}
#[test]
fn acceptance_backups_do_not_overwrite_same_second() {
    let dir = fixture("backup");
    let p = dir.join("auth.json");
    fs::write(&p, "first").unwrap();
    let first = backup_file(&p).unwrap().unwrap();
    fs::write(&p, "second").unwrap();
    let second = backup_file(&p).unwrap().unwrap();
    assert_ne!(
        first, second,
        "same-second backup overwrites previous backup"
    );
    assert_eq!(fs::read_to_string(first).unwrap(), "first");
}
#[test]
fn acceptance_invalid_auth_is_not_silently_replaced() {
    let dir = fixture("bad-auth");
    let p = dir.join("auth.json");
    fs::write(&p, "{broken authentication").unwrap();
    let result = write_auth_json(&p, "fake-test", ProviderAuthStrategy::ApiKey);
    assert!(
        result.is_err(),
        "malformed auth is silently replaced instead of reporting error"
    );
    assert_eq!(fs::read_to_string(&p).unwrap(), "{broken authentication");
}

#[test]
fn invalid_base_url_stops_before_file_creation() {
    for url in [
        "not a url",
        "ftp://example.invalid",
        "https://u:p@example.invalid",
        "https://example.invalid?key=fake",
        "https://example.invalid/#x",
    ] {
        let dir = fixture("bad-url");
        let path = dir.join("config.toml");
        assert!(write_config_toml(
            &path,
            PROVIDER_ID,
            url,
            "test",
            None,
            ProviderAuthStrategy::ApiKey,
            Some("fake")
        )
        .is_err());
        assert!(!path.exists());
        assert!(!dir.join(BACKUP_DIR_NAME).exists());
    }
}

#[test]
fn backup_exclusive_copy_cannot_replace_existing_file() {
    let dir = fixture("exclusive-copy");
    let source = dir.join("source");
    let destination = dir.join("destination");
    fs::write(&source, "new").unwrap();
    fs::write(&destination, "original").unwrap();
    assert!(copy_private_new(&source, &destination).is_err());
    assert_eq!(fs::read_to_string(destination).unwrap(), "original");
}

#[test]
fn non_object_auth_is_rejected() {
    for content in ["null", "[]", "123", "\"text\""] {
        assert!(render_auth_json_content(content, "fake", ProviderAuthStrategy::ApiKey).is_err());
    }
}
#[test]
fn acceptance_snapshot_secrets_have_private_permissions() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = fixture("snapshot-mode");
        let config = dir.join("config.toml");
        let auth = dir.join("auth.json");
        fs::write(&config, "model = \"original\"").unwrap();
        fs::write(&auth, "{\"OPENAI_API_KEY\":\"fake-test\"}").unwrap();
        fs::set_permissions(&auth, fs::Permissions::from_mode(0o644)).unwrap();
        ensure_restore_snapshot(&dir, &config, &auth).unwrap();
        assert_eq!(
            fs::metadata(dir.join(BACKUP_DIR_NAME).join("auth.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600,
            "snapshot retains world-readable source mode"
        );
    }
}

#[test]
fn restore_preserves_later_mcp_user_rules_language_and_outputs() {
    let dir = fixture("restore-independent");
    fs::write(dir.join("config.toml"), "model='original'\n").unwrap();
    fs::write(dir.join("AGENTS.md"), "Existing rules").unwrap();
    configure_provider_in_home(&dir, "fake-key".into(), "https://example.invalid/v1".into()).unwrap();
    let path = dir.join("config.toml");
    let mut current = read_config_for_write(&path).unwrap().parse::<DocumentMut>().unwrap();
    current["developer_instructions"] = value("Later developer rules");
    current["desktop"] = Item::Table(Table::new());
    current["desktop"]["localeOverride"] = value("zh-CN");
    current["mcp_servers"]["other"] = Item::Table(Table::new());
    current["mcp_servers"]["other"]["command"] = value("keep-user-tool");
    fs::write(&path, current.to_string()).unwrap();
    let agents = fs::read_to_string(dir.join("AGENTS.md")).unwrap();
    fs::write(dir.join("AGENTS.md"), format!("{agents}\nLater rules")).unwrap();
    fs::create_dir(dir.join("sessions")).unwrap();
    fs::write(dir.join("sessions/keep.jsonl"), "not touched").unwrap();
    fs::create_dir(dir.join("output")).unwrap();
    fs::write(dir.join("output/image.png"), "not touched").unwrap();
    restore_defaults_in_home(&dir).unwrap();
    let restored = read_config_for_write(&path).unwrap();
    assert_eq!(read_root_string(&restored, "model").as_deref(), Some("original"));
    assert!(restored.contains("keep-user-tool"));
    assert!(restored.contains("zh-CN"));
    assert!(restored.contains("Later developer rules"));
    assert!(!restored.contains("oceanway_images"));
    assert_eq!(fs::read_to_string(dir.join("AGENTS.md")).unwrap(), "Existing rules\nLater rules");
    assert!(!dir.join(mcp_config::RECEIPT).exists());
    assert_eq!(fs::read_to_string(dir.join("sessions/keep.jsonl")).unwrap(), "not touched");
    assert_eq!(fs::read_to_string(dir.join("output/image.png")).unwrap(), "not touched");
}

#[test]
fn owned_legacy_rules_migrate_without_duplicate_credentials_or_instructions() {
    let dir = fixture("legacy-migration");
    let old = merge_config("", PROVIDER_ID, "https://example.invalid/v1", "keep-model",
        None, ProviderAuthStrategy::ApiKey).unwrap();
    let old = direct_image_config::merge(&merge_direct_http_environment(
        &old, "fake-old", "https://example.invalid/v1").unwrap()).unwrap();
    fs::write(dir.join("config.toml"), old).unwrap();
    fs::write(dir.join("auth.json"), r#"{"OPENAI_API_KEY":"fake-old"}"#).unwrap();
    fs::write(dir.join("AGENTS.md"), format!("{}\n\nKeep user rules", direct_image_config::RULES.trim_end())).unwrap();
    configure_provider_in_home(&dir, "fake-new".into(), "https://example.invalid/v1".into()).unwrap();
    let config = read_config_for_write(&dir.join("config.toml")).unwrap();
    assert!(!config.contains("fake-old"));
    assert!(!config.contains("shell_environment_policy"));
    assert!(!config.contains("OCEANWAY:DIRECT-IMAGE-API"));
    let instructions = fs::read_to_string(dir.join("AGENTS.md")).unwrap();
    assert!(instructions.starts_with(agent_rules::RULES.trim_end()));
    assert!(instructions.ends_with("Keep user rules"));
    assert_eq!(instructions.matches("OCEANWAY:IMAGE-MCP-ROUTING:BEGIN").count(), 1);
}

#[test]
fn foreign_mcp_and_linked_auth_stop_configuration_before_any_write() {
    let dir = fixture("foreign-mcp");
    let config = "[mcp_servers.oceanway_images]\ncommand='user-managed'\n";
    fs::write(dir.join("config.toml"), config).unwrap();
    assert!(configure_provider_in_home(&dir, "fake".into(), DEFAULT_BASE_URL.into()).is_err());
    assert_eq!(read_config_for_write(&dir.join("config.toml")).unwrap(), config);
    assert!(!dir.join("auth.json").exists());
    let linked = fixture("linked-auth");
    fs::write(linked.join("source.json"), "{}").unwrap();
    fs::hard_link(linked.join("source.json"), linked.join("auth.json")).unwrap();
    assert!(configure_provider_in_home(&linked, "fake".into(), DEFAULT_BASE_URL.into()).is_err());
    assert_eq!(fs::read_to_string(linked.join("source.json")).unwrap(), "{}");
}
