use super::*;
#[cfg(target_os = "macos")]
#[test]
fn restart_exit_check_detects_either_host_or_backend_and_fails_closed() {
    for command in [
        "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
        "/Applications/ChatGPT.app/Contents/Resources/codex app-server",
        "/Users/test/Library/Application Support/OpenAI/Codex/bin/hash/codex app-server",
    ] {
        assert!(macos_target_present(Some(command), MacosCodexHost::ChatGpt).unwrap());
    }
    assert!(!macos_target_present(Some(""), MacosCodexHost::ChatGpt).unwrap());
    assert!(macos_target_present(None, MacosCodexHost::ChatGpt).is_err());
}
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
fn restore_preserves_later_mcp_user_rules_desktop_preferences_and_outputs() {
    let dir = fixture("restore-independent");
    fs::write(dir.join("config.toml"), "model='original'\n").unwrap();
    fs::write(dir.join("AGENTS.md"), "Existing rules").unwrap();
    configure_provider_in_home(&dir, "fake-key".into(), "https://example.invalid/v1".into()).unwrap();
    let path = dir.join("config.toml");
    let mut current = read_config_for_write(&path).unwrap().parse::<DocumentMut>().unwrap();
    current["developer_instructions"] = value("Later developer rules");
    current["desktop"] = Item::Table(Table::new());
    current["desktop"]["fontSize"] = value(17);
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
    let parsed = restored.parse::<DocumentMut>().unwrap();
    assert_eq!(parsed["desktop"]["fontSize"].as_integer(), Some(17));
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

fn transaction_bytes(home: &Path) -> Vec<Option<Vec<u8>>> {
    CONFIG_TRANSACTION_FILES.iter()
        .map(|name| managed_files::read_optional(&home.join(name)).unwrap()).collect()
}

fn fallback_restore_fixture(label: &str) -> PathBuf {
    let home = fixture(label);
    fs::write(home.join("AGENTS.md"), "Keep plain rules").unwrap();
    fs::write(home.join("AGENTS.override.md"), "Keep active rules").unwrap();
    let config = merge_config("", PROVIDER_ID, "https://example.invalid/v1", "test",
        None, ProviderAuthStrategy::ApiKey).unwrap();
    let config = mcp_config::install(&home, &config).unwrap();
    agent_rules::write_with_config(&home, &home.join("config.toml"), &config).unwrap();
    fs::write(home.join("auth.json"),
        r#"{"OPENAI_API_KEY":"fake","auth_mode":"chatgpt","tokens":{"access_token":"fake-login"},"keep":true}"#,
    ).unwrap();
    assert!(mcp_config::status(&home, &config).runtime_verified);
    assert!(!home.join(BACKUP_DIR_NAME).exists());
    home
}

#[test]
fn snapshot_failed_stages_never_block_retry_or_replace_first_originals() {
    for failed_member in ["config.toml", "auth.json", "meta.json"] {
        let home = fixture("snapshot-stage-retry");
        let config = home.join("config.toml");
        let auth = home.join("auth.json");
        let original_config = b"model='original'\n";
        let original_auth = br#"{"tokens":{"access_token":"fake-original"}}"#;
        fs::write(&config, original_config).unwrap();
        fs::write(&auth, original_auth).unwrap();
        let before = transaction_bytes(&home);
        let result = ensure_restore_snapshot_with_writer(&home, &config, &auth, |path, bytes| {
            if path.file_name().and_then(|name| name.to_str()) == Some(failed_member) {
                // Leave a partial file, as a failed write or interrupted process would.
                fs::write(path, &bytes[..bytes.len() / 2])?;
                return Err(std::io::Error::other("injected staging failure"));
            }
            write_private_atomic(path, bytes)
        });
        assert!(result.is_err());
        assert_eq!(transaction_bytes(&home), before);
        assert!(!home.join(BACKUP_DIR_NAME).exists());
        let stage = fs::read_dir(&home).unwrap().map(|entry| entry.unwrap().path())
            .find(|path| path.file_name().unwrap().to_string_lossy()
                .starts_with(".oceanway-ai-backup.pending-")).unwrap();
        let partial = fs::read(stage.join(failed_member)).unwrap();

        ensure_restore_snapshot(&home, &config, &auth).unwrap();
        assert_eq!(fs::read(stage.join(failed_member)).unwrap(), partial);
        let committed = home.join(BACKUP_DIR_NAME);
        let meta = fs::read(committed.join("meta.json")).unwrap();
        fs::write(&config, "model='later'\n").unwrap();
        fs::write(&auth, r#"{"tokens":{"access_token":"fake-later"}}"#).unwrap();
        ensure_restore_snapshot(&home, &config, &auth).unwrap();
        assert_eq!(fs::read(committed.join("config.toml")).unwrap().as_slice(), original_config.as_slice());
        assert_eq!(fs::read(committed.join("auth.json")).unwrap().as_slice(), original_auth.as_slice());
        assert_eq!(fs::read(committed.join("meta.json")).unwrap(), meta);
    }
}

#[test]
fn complete_legacy_snapshot_remains_compatible_and_immutable() {
    let home = fixture("legacy-snapshot");
    let config = home.join("config.toml");
    let auth = home.join("auth.json");
    fs::write(&config, "model='current'\n").unwrap();
    fs::write(&auth, r#"{"OPENAI_API_KEY":"fake-current"}"#).unwrap();
    let snapshot = home.join(BACKUP_DIR_NAME);
    fs::create_dir(&snapshot).unwrap();
    let original_config = "# Original formatting\nmodel = 'original'\n";
    let original_auth = "{ \"tokens\": { \"access_token\": \"fake-original\" }, \"keep\": true }\n";
    let original_meta = "{\n  \"config_existed\": true,\n  \"auth_existed\": true,\n  \"created_at\": \"2026-09-01T12:00:00+08:00\"\n}\n";
    fs::write(snapshot.join("config.toml"), original_config).unwrap();
    fs::write(snapshot.join("auth.json"), original_auth).unwrap();
    fs::write(snapshot.join("meta.json"), original_meta).unwrap();
    let before = transaction_bytes(&home);
    ensure_restore_snapshot(&home, &config, &auth).unwrap();
    assert_eq!(transaction_bytes(&home), before);
    assert_eq!(fs::read_to_string(snapshot.join("config.toml")).unwrap(), original_config);
    assert_eq!(fs::read_to_string(snapshot.join("auth.json")).unwrap(), original_auth);
    assert_eq!(fs::read_to_string(snapshot.join("meta.json")).unwrap(), original_meta);
    assert!(restore_from_snapshot(&home, &config, &auth).unwrap());
    assert_eq!(fs::read_to_string(&config).unwrap(), original_config);
    assert_eq!(fs::read_to_string(&auth).unwrap(), original_auth);
}

#[test]
fn incomplete_old_snapshots_block_configuration_and_restore_without_overwriting() {
    for case in ["missing-meta", "truncated-meta", "missing-auth", "bad-config", "bad-auth", "wrong-flags"] {
        let home = fixture("old-snapshot-partial");
        fs::write(home.join("config.toml"), "model='current'\n").unwrap();
        fs::write(home.join("auth.json"), r#"{"tokens":{"access_token":"fake-current"}}"#).unwrap();
        fs::write(home.join("AGENTS.md"), format!("{}\nKeep user rules", agent_rules::RULES)).unwrap();
        let snapshot = home.join(BACKUP_DIR_NAME);
        fs::create_dir(&snapshot).unwrap();
        fs::write(snapshot.join("config.toml"),
            if case == "bad-config" { "[broken" } else { "model='first'\n" }).unwrap();
        if case != "missing-auth" {
            fs::write(snapshot.join("auth.json"),
                if case == "bad-auth" { "[]" } else { r#"{"tokens":{"access_token":"fake-first"}}"# }).unwrap();
        }
        if case != "missing-meta" {
            let meta = if case == "truncated-meta" { "{".into() } else {
                serde_json::to_string(&RestoreSnapshotMeta {
                    config_existed: true,
                    auth_existed: case != "wrong-flags",
                    created_at: Local::now().to_rfc3339(),
                }).unwrap()
            };
            fs::write(snapshot.join("meta.json"), meta).unwrap();
        }
        let originals = ["config.toml", "auth.json", "meta.json"].map(|name|
            managed_files::read_optional(&snapshot.join(name)).unwrap());
        let before = transaction_bytes(&home);
        assert!(configure_provider_in_home(&home, "fake-new".into(), DEFAULT_BASE_URL.into()).is_err());
        assert_eq!(transaction_bytes(&home), before);
        assert!(restore_defaults_in_home(&home).is_err());
        assert_eq!(transaction_bytes(&home), before);
        for (name, bytes) in ["config.toml", "auth.json", "meta.json"].iter().zip(originals) {
            assert_eq!(managed_files::read_optional(&snapshot.join(name)).unwrap(), bytes);
        }
    }
}

#[test]
fn incomplete_snapshot_cannot_fall_through_to_owned_mcp_restore() {
    let home = fallback_restore_fixture("mcp-partial-snapshot");
    let snapshot = home.join(BACKUP_DIR_NAME);
    fs::create_dir(&snapshot).unwrap();
    fs::write(snapshot.join("config.toml"), "model='first'\n").unwrap();
    let before = transaction_bytes(&home);
    assert!(restore_defaults_in_home(&home).is_err());
    assert_eq!(transaction_bytes(&home), before);
    assert_eq!(fs::read_to_string(snapshot.join("config.toml")).unwrap(), "model='first'\n");
    assert!(!snapshot.join("meta.json").exists());
}

#[test]
fn snapshot_staging_readback_mismatch_is_not_committed() {
    let home = fixture("snapshot-readback");
    let config = home.join("config.toml");
    let auth = home.join("auth.json");
    fs::write(&config, "model='original'\n").unwrap();
    fs::write(&auth, "{}").unwrap();
    let before = transaction_bytes(&home);
    assert!(ensure_restore_snapshot_with_writer(&home, &config, &auth, |path, bytes| {
        let bytes = if path.file_name().and_then(|name| name.to_str()) == Some("config.toml") {
            b"model='wrong'\n".as_slice()
        } else { bytes };
        write_private_atomic(path, bytes)
    }).is_err());
    assert_eq!(transaction_bytes(&home), before);
    assert!(!home.join(BACKUP_DIR_NAME).exists());
    ensure_restore_snapshot(&home, &config, &auth).unwrap();
}

#[test]
fn snapshot_source_change_during_staging_requires_retry() {
    let home = fixture("snapshot-source-change");
    let config = home.join("config.toml");
    let auth = home.join("auth.json");
    fs::write(&config, "model='original'\n").unwrap();
    fs::write(&auth, "{}").unwrap();
    assert!(ensure_restore_snapshot_with_writer(&home, &config, &auth, |path, bytes| {
        write_private_atomic(path, bytes)?;
        if path.file_name().and_then(|name| name.to_str()) == Some("meta.json") {
            fs::write(&config, "model='user-changed'\n")?;
        }
        Ok(())
    }).is_err());
    assert!(!home.join(BACKUP_DIR_NAME).exists());
    assert_eq!(fs::read_to_string(&config).unwrap(), "model='user-changed'\n");
    ensure_restore_snapshot(&home, &config, &auth).unwrap();
    assert_eq!(fs::read_to_string(home.join(BACKUP_DIR_NAME).join("config.toml")).unwrap(),
        "model='user-changed'\n");
}

#[test]
fn snapshot_commit_never_replaces_a_directory_that_appears_during_staging() {
    let home = fixture("snapshot-commit-race");
    let config = home.join("config.toml");
    let auth = home.join("auth.json");
    fs::write(&config, "model='original'\n").unwrap();
    let snapshot = home.join(BACKUP_DIR_NAME);
    assert!(ensure_restore_snapshot_with_writer(&home, &config, &auth, |path, bytes| {
        write_private_atomic(path, bytes)?;
        if path.file_name().and_then(|name| name.to_str()) == Some("meta.json") {
            fs::create_dir(&snapshot)?;
            fs::write(snapshot.join("config.toml"), "model='first-owner'\n")?;
        }
        Ok(())
    }).is_err());
    assert_eq!(fs::read_to_string(snapshot.join("config.toml")).unwrap(), "model='first-owner'\n");
    assert!(!snapshot.join("meta.json").exists());
    assert!(ensure_restore_snapshot(&home, &config, &auth).is_err());
}

#[test]
fn fallback_restore_rejects_invalid_auth_and_rolls_back_every_managed_file() {
    let home = fallback_restore_fixture("fallback-bad-auth");
    for bytes in [b"{broken".to_vec(), vec![0xff, 0xfe], b"[]".to_vec(), b"null".to_vec()] {
        fs::write(home.join("auth.json"), &bytes).unwrap();
        let before = transaction_bytes(&home);
        assert!(restore_defaults_in_home(&home).is_err());
        assert_eq!(transaction_bytes(&home), before);
        assert!(!home.join(BACKUP_DIR_NAME).exists());
    }
}

#[test]
fn fallback_auth_write_failure_and_bad_readback_roll_back_the_whole_restore() {
    let home = fallback_restore_fixture("fallback-auth-write");
    for failure in ["before-write", "after-write", "readback"] {
        let before = transaction_bytes(&home);
        let result = restore_defaults_in_home_with_auth_remover(&home, |auth_path| {
            remove_api_key_from_auth_with_writer(auth_path, |path, bytes| {
                if failure == "before-write" {
                    return Err(std::io::Error::other("injected write failure"));
                }
                write_private_atomic(path, if failure == "readback" { b"{}" } else { bytes })?;
                if failure == "after-write" {
                    return Err(std::io::Error::other("injected post-write failure"));
                }
                Ok(())
            })
        });
        assert!(result.is_err());
        assert_eq!(transaction_bytes(&home), before);
    }
}

#[test]
fn fallback_restore_removes_only_api_key_and_keeps_existing_login() {
    let home = fallback_restore_fixture("fallback-login");
    let auth = home.join("auth.json");
    let mut expected: Value = serde_json::from_slice(&fs::read(&auth).unwrap()).unwrap();
    expected.as_object_mut().unwrap().remove(CODEX_AUTH_KEY);
    restore_defaults_in_home(&home).unwrap();
    let actual: Value = serde_json::from_slice(&fs::read(&auth).unwrap()).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(fs::read_to_string(home.join("AGENTS.md")).unwrap(), "Keep plain rules");
    assert_eq!(fs::read_to_string(home.join("AGENTS.override.md")).unwrap(), "Keep active rules");
    assert!(!home.join(mcp_config::RECEIPT).exists());
}

#[test]
fn auth_removal_without_a_key_is_an_exact_noop() {
    let home = fixture("auth-remove-no-key");
    let path = home.join("auth.json");
    remove_api_key_from_auth(&path).unwrap();
    assert!(!path.exists());
    let original = "{ \"tokens\": { \"access_token\": \"fake-login\" }, \"keep\": true }\n";
    fs::write(&path, original).unwrap();
    remove_api_key_from_auth_with_writer(&path, |_, _| panic!("must not rewrite auth without a key")).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}
