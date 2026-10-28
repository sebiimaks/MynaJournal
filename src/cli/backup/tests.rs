use super::*;

#[test]
fn manifest_creation_obeys_the_restore_size_limit() {
    let mut manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        backend: BackendType::default(),
        created_at: "2026-10-28T00:00:00Z".into(),
        directories: BTreeSet::new(),
        files: BTreeMap::new(),
    };
    manifest.directories.insert(String::new());
    let overhead = manifest.encode().unwrap().len();
    manifest.directories.clear();
    manifest
        .directories
        .insert("a".repeat(MAX_MANIFEST_BYTES as usize - overhead));
    let encoded = manifest.encode().unwrap();
    assert_eq!(encoded.len() as u64, MAX_MANIFEST_BYTES);
    assert!(serde_json::from_slice::<Manifest>(&encoded).is_ok());
    manifest.directories.insert("b".into());
    assert!(
        manifest
            .encode()
            .unwrap_err()
            .to_string()
            .contains("manifest exceeds the 16 MiB limit")
    );
}

struct Fixture {
    root: tempfile::TempDir,
    config: PathBuf,
    settings: Settings,
}

impl Fixture {
    fn new(backend: BackendType) -> Self {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let config = original.join("config");
        fs::create_dir_all(config.join("themes")).unwrap();
        fs::create_dir_all(original.join("state")).unwrap();
        fs::write(config.join("config.toml"), "# preserve this comment\neditor_word_wrap = false\nshow_entry_priority = false\nunknown_preference = 'retain me'\n").unwrap();
        fs::write(
            config.join("themes/theme_custom.toml"),
            "# exact custom theme\n",
        )
        .unwrap();
        fs::write(config.join("themes.toml"), "# legacy theme\n").unwrap();
        fs::write(
            original.join("state/state.json"),
            "{\"full_screen\":false,\"sorter\":{\"criteria\":[],\"order\":\"Descending\"}}",
        )
        .unwrap();
        let mut settings = Settings {
            backend_type: Some(backend),
            app_state_dir: Some(original.join("state")),
            ..Default::default()
        };
        #[cfg(feature = "json")]
        {
            settings.json_backend.file_path = Some(original.join("data/entries.json"));
        }
        #[cfg(feature = "sqlite")]
        {
            settings.sqlite_backend.file_path = Some(original.join("data/entries.db"));
        }
        #[cfg(feature = "file")]
        {
            settings.file_backend.directory = Some(original.join("data/entries"));
        }
        #[cfg(feature = "vjournal")]
        {
            settings.vjournal_backend.directory = Some(original.join("data/journal"));
        }
        Self {
            root,
            config,
            settings,
        }
    }

    fn backup(&self) -> PathBuf {
        self.root.path().join("backup")
    }
    fn restored(&self) -> PathBuf {
        self.root.path().join("restored")
    }
    fn source(&self) -> Source {
        source_data(&self.settings).unwrap()
    }

    fn native_data(&self) {
        let source = self.source();
        if source.directory {
            fs::create_dir_all(source.path.join("nested/empty")).unwrap();
            fs::write(source.path.join("note"), "native metadata\n-----BEGIN AGE ENCRYPTED FILE-----\nciphertext\n-----END AGE ENCRYPTED FILE-----\n").unwrap();
            fs::write(
                source.path.join("nested/unknown.ics"),
                "BEGIN:VCALENDAR\nX-UNKNOWN:retain\nEND:VCALENDAR\n",
            )
            .unwrap();
        } else {
            fs::create_dir_all(source.path.parent().unwrap()).unwrap();
            fs::write(
                source.path,
                "{\"native\":\"metadata\",\"ciphertext\":\"-----BEGIN AGE ENCRYPTED FILE-----\"}",
            )
            .unwrap();
        }
    }
}

fn file_backends() -> Vec<BackendType> {
    vec![
        #[cfg(feature = "json")]
        BackendType::Json,
        #[cfg(feature = "file")]
        BackendType::File,
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal,
    ]
}

#[tokio::test]
async fn native_backups_restore_ciphertext_metadata_themes_and_independent_paths() {
    for backend in file_backends() {
        let fixture = Fixture::new(backend);
        fixture.native_data();
        let source = fixture.source();
        let before = inventory_source(&source).unwrap();
        create(&fixture.settings, Some(&fixture.config), &fixture.backup())
            .await
            .unwrap();
        assert!(inventory_source(&source).unwrap().same_contents(&before));
        restore(&fixture.backup(), &fixture.restored()).unwrap();
        let restored = Source {
            path: fixture.restored().join(&source.destination),
            ..source
        };
        assert!(inventory_source(&restored).unwrap().same_contents(&before));
        let config = fs::read_to_string(fixture.restored().join(CONFIG)).unwrap();
        assert!(config.contains("# preserve this comment"));
        assert!(config.contains("unknown_preference = 'retain me'"));
        assert!(config.contains("editor_word_wrap = false"));
        assert!(config.contains("show_entry_priority = false"));
        let parsed: toml::Table = toml::from_str(&config).unwrap();
        assert_eq!(
            parsed["backend_type"].as_str().unwrap(),
            backend_name(backend)
        );
        assert!(
            Path::new(parsed["app_state_dir"].as_str().unwrap())
                .starts_with(fs::canonicalize(fixture.restored()).unwrap())
        );
        for (table, field) in [
            ("json_backend", "file_path"),
            ("sqlite_backend", "file_path"),
            ("file_backend", "directory"),
            ("vjournal_backend", "directory"),
        ] {
            assert!(
                Path::new(parsed[table][field].as_str().unwrap())
                    .starts_with(fs::canonicalize(fixture.restored()).unwrap())
            );
        }
        assert_eq!(
            fs::read(fixture.config.join("config.toml")).unwrap(),
            fs::read(fixture.backup().join(ORIGINAL_CONFIG)).unwrap()
        );
        assert_eq!(
            fs::read(fixture.config.join("themes/theme_custom.toml")).unwrap(),
            fs::read(fixture.restored().join("config/themes/theme_custom.toml")).unwrap()
        );
        assert_eq!(
            fs::read(fixture.config.join("themes.toml")).unwrap(),
            fs::read(fixture.restored().join("config/themes.toml")).unwrap()
        );
    }
}

#[tokio::test]
async fn backup_of_an_unused_store_does_not_create_source_data() {
    let fixture = Fixture::new(BackendType::default());
    let source = fixture.source().path;
    assert!(!source.exists());
    create(&fixture.settings, Some(&fixture.config), &fixture.backup())
        .await
        .unwrap();
    assert!(!source.exists());
    assert!(!source.parent().unwrap().exists());
    restore(&fixture.backup(), &fixture.restored()).unwrap();
}

#[tokio::test]
async fn backup_and_restore_refuse_existing_destinations_and_source_subdirectories() {
    let fixture = Fixture::new(BackendType::default());
    fs::create_dir(fixture.backup()).unwrap();
    fs::write(fixture.backup().join("keep"), "existing").unwrap();
    assert!(
        create(&fixture.settings, Some(&fixture.config), &fixture.backup())
            .await
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(fixture.backup().join("keep")).unwrap(),
        "existing"
    );
    assert!(
        create(
            &fixture.settings,
            Some(&fixture.config),
            &fixture.config.join("backup")
        )
        .await
        .is_err()
    );
    assert!(!fixture.config.join("backup").exists());
    let other = fixture.root.path().join("other-backup");
    create(&fixture.settings, Some(&fixture.config), &other)
        .await
        .unwrap();
    assert!(restore(&other, &fixture.backup()).is_err());
    assert_eq!(
        fs::read_to_string(fixture.backup().join("keep")).unwrap(),
        "existing"
    );
    assert!(restore(&other, &other.join("restored")).is_err());
}

#[tokio::test]
async fn restore_rejects_tampering_extra_files_and_traversal_before_creating_destination() {
    for mutation in [
        "tamper",
        "extra",
        "traversal",
        "absolute",
        "unknown-version",
    ] {
        let fixture = Fixture::new(BackendType::default());
        create(&fixture.settings, Some(&fixture.config), &fixture.backup())
            .await
            .unwrap();
        match mutation {
            "tamper" => fs::write(fixture.backup().join(CONFIG), "changed").unwrap(),
            "extra" => fs::write(fixture.backup().join("unlisted"), "extra").unwrap(),
            other => {
                let path = fixture.backup().join(MANIFEST);
                let mut manifest: Manifest =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                if other == "unknown-version" {
                    manifest.version += 1;
                } else {
                    manifest.files.insert(
                        if other == "absolute" {
                            "/outside"
                        } else {
                            "../outside"
                        }
                        .into(),
                        FileRecord {
                            bytes: 0,
                            sha256: String::new(),
                        },
                    );
                }
                fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            }
        }
        assert!(
            restore(&fixture.backup(), &fixture.restored()).is_err(),
            "{mutation}"
        );
        assert!(!fixture.restored().exists());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn backup_and_restore_reject_descendant_symlinks() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new(BackendType::default());
    symlink(
        fixture.config.join("config.toml"),
        fixture.config.join("themes/alias"),
    )
    .unwrap();
    assert!(
        create(&fixture.settings, Some(&fixture.config), &fixture.backup())
            .await
            .is_err()
    );
    assert!(!fixture.backup().exists());
    fs::remove_file(fixture.config.join("themes/alias")).unwrap();
    create(&fixture.settings, Some(&fixture.config), &fixture.backup())
        .await
        .unwrap();
    let config = fixture.backup().join(CONFIG);
    fs::remove_file(&config).unwrap();
    symlink(fixture.config.join("config.toml"), &config).unwrap();
    assert!(restore(&fixture.backup(), &fixture.restored()).is_err());
    assert!(!fixture.restored().exists());
}

#[cfg(unix)]
#[tokio::test]
async fn backup_and_restore_create_only_private_files_and_directories() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new(BackendType::default());
    create(&fixture.settings, Some(&fixture.config), &fixture.backup())
        .await
        .unwrap();
    restore(&fixture.backup(), &fixture.restored()).unwrap();
    for root in [fixture.backup(), fixture.restored()] {
        let inventory = inventory_tree(&root, false).unwrap();
        for relative in inventory.files.keys() {
            assert_eq!(
                fs::metadata(root.join(relative))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        for relative in inventory
            .directories
            .iter()
            .chain(std::iter::once(&PathBuf::new()))
        {
            assert_eq!(
                fs::metadata(root.join(relative))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }
}

#[cfg(feature = "json")]
#[tokio::test]
async fn backup_refuses_an_active_json_store_without_creating_output() {
    let fixture = Fixture::new(BackendType::Json);
    fixture.native_data();
    let mut lock_path = fixture.source().path.as_os_str().to_os_string();
    lock_path.push(".lock");
    let lock = File::create(PathBuf::from(lock_path)).unwrap();
    lock.lock().unwrap();
    assert!(
        create(&fixture.settings, Some(&fixture.config), &fixture.backup())
            .await
            .is_err()
    );
    assert!(!fixture.backup().exists());
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_backup_includes_committed_wal_records_and_native_metadata() {
    use sqlx::{
        Connection, SqliteConnection,
        sqlite::{SqliteConnectOptions, SqliteJournalMode},
    };
    let fixture = Fixture::new(BackendType::Sqlite);
    let source = fixture.source().path;
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(&source)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);
    let mut live = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query("CREATE TABLE native_metadata (value TEXT)")
        .execute(&mut live)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO native_metadata VALUES ('protected ciphertext and unknown native fields')",
    )
    .execute(&mut live)
    .await
    .unwrap();
    assert!(source.with_file_name("entries.db-wal").exists());
    create(&fixture.settings, Some(&fixture.config), &fixture.backup())
        .await
        .unwrap();
    restore(&fixture.backup(), &fixture.restored()).unwrap();
    let mut restored = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(fixture.restored().join("data/entries.db"))
            .read_only(true),
    )
    .await
    .unwrap();
    let value: String = sqlx::query_scalar("SELECT value FROM native_metadata")
        .fetch_one(&mut restored)
        .await
        .unwrap();
    assert_eq!(value, "protected ciphertext and unknown native fields");
    live.close().await.unwrap();
    restored.close().await.unwrap();
}

#[tokio::test]
async fn restore_command_bypasses_invalid_current_configuration_and_parses_cli() {
    use clap::Parser;
    let fixture = Fixture::new(BackendType::default());
    create(&fixture.settings, Some(&fixture.config), &fixture.backup())
        .await
        .unwrap();
    fs::write(fixture.config.join("config.toml"), "invalid [ TOML").unwrap();
    let cli = crate::cli::Cli::try_parse_from([
        "mjournal",
        "--config",
        fixture.config.to_str().unwrap(),
        "backup",
        "restore",
        fixture.backup().to_str().unwrap(),
        "--to",
        fixture.restored().to_str().unwrap(),
    ])
    .unwrap();
    assert!(cli.execute_backup().await.unwrap());
    assert!(fixture.restored().join(CONFIG).is_file());
    assert_eq!(
        fs::read_to_string(fixture.config.join("config.toml")).unwrap(),
        "invalid [ TOML"
    );
}

#[cfg(feature = "json")]
#[tokio::test]
async fn cli_backup_honors_backend_override_without_initializing_a_missing_store() {
    use clap::Parser;
    let fixture = Fixture::new(BackendType::Json);
    let path = fixture.root.path().join("unused/new/notes.json");
    let cli = crate::cli::Cli::try_parse_from([
        "mjournal",
        "--config",
        fixture.config.to_str().unwrap(),
        "--json-file-path",
        path.to_str().unwrap(),
        "backup",
        "create",
        fixture.backup().to_str().unwrap(),
    ])
    .unwrap();
    assert!(cli.execute_backup().await.unwrap());
    assert!(!path.parent().unwrap().exists());
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(fixture.backup().join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(manifest.backend, BackendType::Json);
}
