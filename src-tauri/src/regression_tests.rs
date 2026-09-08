use super::*;
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
    assert!(has_matching_imagegen_cli_environment(
        &first,
        Some("fake-test"),
        Some("https://example.invalid")
    ));
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
    assert!(has_matching_imagegen_cli_environment(
        &repeated,
        Some("fake-test"),
        Some("https://example.invalid")
    ));
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
    assert!(restore_from_snapshot(&dir, &config, &auth).unwrap());
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
