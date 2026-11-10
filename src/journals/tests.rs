use super::*;

fn fixture() -> (tempfile::TempDir, JournalStore) {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    fs::write(&config, "auto_title = 'Daily'\neditor_word_wrap = false\n").unwrap();
    let store = JournalStore::new(config).unwrap();
    (directory, store)
}

fn assert_isolated(settings: &Settings, profile: &Path) {
    #[cfg(feature = "json")]
    assert_eq!(
        settings.json_backend.file_path.as_deref(),
        Some(profile.join("data/entries.json").as_path())
    );
    #[cfg(feature = "sqlite")]
    assert_eq!(
        settings.sqlite_backend.file_path.as_deref(),
        Some(profile.join("data/entries.db").as_path())
    );
    #[cfg(feature = "vjournal")]
    assert_eq!(
        settings.vjournal_backend.directory.as_deref(),
        Some(profile.join("data/journal").as_path())
    );
    #[cfg(feature = "file")]
    assert_eq!(
        settings.file_backend.directory.as_deref(),
        Some(profile.join("data/entries").as_path())
    );
    assert_eq!(
        settings.app_state_dir.as_deref(),
        Some(profile.join("state").as_path())
    );
    assert_eq!(
        settings.export.default_path.as_deref(),
        Some(profile.join("exports").as_path())
    );
}

#[test]
fn personal_keeps_existing_paths_and_listing_does_not_create_files() {
    let (_directory, store) = fixture();
    let journals = store.list().unwrap();
    assert_eq!(journals.len(), 1);
    let personal = &journals[0];
    assert_eq!(personal.name, "Personal");
    assert!(personal.is_personal);
    assert!(!store.root().exists());
    let settings = store.load_settings(personal).unwrap();
    assert_eq!(settings.app_state_dir, None);
    assert_eq!(settings.export.default_path, None);
    #[cfg(feature = "json")]
    assert_eq!(settings.json_backend.file_path, None);
    #[cfg(feature = "sqlite")]
    assert_eq!(settings.sqlite_backend.file_path, None);
    #[cfg(feature = "vjournal")]
    assert_eq!(settings.vjournal_backend.directory, None);
    #[cfg(feature = "file")]
    assert_eq!(settings.file_backend.directory, None);
    assert!(store.archive(personal).is_err());
    assert!(personal.settings_path.exists());
}

#[test]
fn new_journals_clone_preferences_and_isolate_all_storage_and_state() {
    let (_directory, store) = fixture();
    let base = store.load_settings(&store.list().unwrap()[0]).unwrap();
    let first = store.create(" Work ", &base).unwrap();
    let second = store.create("Travel", &base).unwrap();
    assert_eq!(first.name, "Work");
    assert_ne!(first.id, second.id);
    let first_settings = store.load_settings(&first).unwrap();
    let second_settings = store.load_settings(&second).unwrap();
    assert_isolated(&first_settings, first.settings_path.parent().unwrap());
    assert_isolated(&second_settings, second.settings_path.parent().unwrap());
    let text = toml::to_string(&first_settings).unwrap();
    assert!(text.contains("auto_title = \"Daily\""));
    assert!(!first_settings.editor_word_wrap);
    assert_eq!(
        store
            .list()
            .unwrap()
            .iter()
            .map(|journal| journal.name.as_str())
            .collect::<Vec<_>>(),
        ["Personal", "Travel", "Work"]
    );
    let (detected_store, detected_journal) =
        journal_for_config(&first.settings_path).unwrap().unwrap();
    assert_eq!(detected_store.root(), store.root());
    assert_eq!(detected_journal, first);
}

#[test]
fn new_journals_inherit_remembered_custom_text_without_enabling_it() {
    let (_directory, store) = fixture();
    let base: Settings = toml::from_str("custom_title = 'Daily journal'\n").unwrap();
    let work = store.create("Work", &base).unwrap();
    let travel = store.create("Travel", &base).unwrap();
    let mut work_settings = store.load_settings(&work).unwrap();
    assert_eq!(work_settings.custom_title.as_deref(), Some("Daily journal"));
    assert_eq!(work_settings.auto_title, None);
    work_settings.custom_title = Some("Work note".into());
    fs::write(&work.settings_path, work_settings.get_as_text().unwrap()).unwrap();
    assert_eq!(
        store.load_settings(&travel).unwrap().custom_title,
        base.custom_title
    );
    assert_eq!(
        store
            .load_settings(&store.list().unwrap()[0])
            .unwrap()
            .custom_title,
        None
    );
}

#[test]
fn reset_config_retains_profile_defaults_and_explicit_overrides_remain_explicit() {
    let (directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    fs::write(&journal.settings_path, "").unwrap();
    let settings = store.load_settings(&journal).unwrap();
    assert_isolated(&settings, journal.settings_path.parent().unwrap());
    let mut settings = Settings {
        app_state_dir: Some(PathBuf::from("relative-state")),
        ..Default::default()
    };
    settings.export.default_path = Some(directory.path().join("elsewhere"));
    assert!(apply_profile_defaults(&mut settings, &journal.settings_path).unwrap());
    assert_eq!(
        settings.app_state_dir,
        Some(PathBuf::from("relative-state"))
    );
    assert_eq!(
        settings.export.default_path,
        Some(directory.path().join("elsewhere"))
    );
    assert!(!apply_profile_defaults(&mut settings, &store.settings_path).unwrap());
    fs::remove_file(&journal.settings_path).unwrap();
    assert!(store.load_settings(&journal).is_err());
}

#[test]
fn config_files_in_one_directory_have_independent_collections() {
    let (directory, store) = fixture();
    let other = JournalStore::new(directory.path().join("other.toml")).unwrap();
    let third = JournalStore::new(directory.path().join("third.toml")).unwrap();
    assert_ne!(store.root(), other.root());
    assert_ne!(third.root(), other.root());
    other.create("Other", &Settings::default()).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(third.list().unwrap().len(), 1);
}

#[test]
fn direct_profiles_retain_the_collections_shared_theme_catalog() {
    let (directory, store) = fixture();
    let themes = directory.path().join("shared-themes");
    let store = store.with_themes_path(themes.clone()).unwrap();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let (direct_store, _) = journal_for_config(&journal.settings_path).unwrap().unwrap();
    assert_eq!(store.themes_path(), themes);
    assert_eq!(direct_store.themes_path(), themes);
}

#[test]
fn unrelated_metadata_does_not_turn_an_ordinary_config_into_a_profile() {
    let (directory, store) = fixture();
    for text in [
        "name = 'Unrelated document'\n",
        "format = 'other-application'\n",
        "This is not TOML",
    ] {
        fs::write(directory.path().join(METADATA_FILE), text).unwrap();
        assert!(journal_for_config(&store.settings_path).unwrap().is_none());
        let mut settings = Settings::default();
        assert!(!apply_profile_defaults(&mut settings, &store.settings_path).unwrap());
        assert!(settings.app_state_dir.is_none());
    }
    fs::write(
        directory.path().join(METADATA_FILE),
        format!("format = {PROFILE_FORMAT:?}\n"),
    )
    .unwrap();
    assert!(journal_for_config(&store.settings_path).is_err());
}

#[test]
fn moved_and_malformed_profiles_never_fall_back_to_personal_defaults() {
    let (directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let profile = journal.settings_path.parent().unwrap();
    let renamed = directory.path().join("renamed-profile");
    fs::rename(profile, &renamed).unwrap();
    assert!(journal_for_config(&renamed.join("config.toml")).is_err());
    fs::rename(&renamed, profile).unwrap();
    for text in ["name = 'Unrelated document'\n", "This is not TOML"] {
        fs::write(profile.join(METADATA_FILE), text).unwrap();
        assert!(journal_for_config(&journal.settings_path).is_err());
    }
}

#[cfg(unix)]
#[test]
fn config_aliases_resolve_to_the_managed_profile_and_share_its_lease() {
    use std::os::unix::fs::symlink;

    let (directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let aliases = directory.path().join("aliases");
    fs::create_dir(&aliases).unwrap();
    for name in ["work.toml", "config.toml"] {
        let alias = aliases.join(name);
        symlink(&journal.settings_path, &alias).unwrap();
        let (owner, found) = journal_for_config(&alias).unwrap().unwrap();
        assert_eq!(owner.root(), store.root());
        assert_eq!(found, journal);
        let mut settings = Settings::default();
        assert!(apply_profile_defaults(&mut settings, &alias).unwrap());
        assert_isolated(&settings, journal.settings_path.parent().unwrap());
        let lease = owner.acquire(&found).unwrap();
        assert!(store.acquire(&journal).is_err());
        assert!(store.archive(&journal).is_err());
        drop(lease);
    }
    let chained_alias = aliases.join("chained.toml");
    symlink("work.toml", &chained_alias).unwrap();
    assert_eq!(
        journal_for_config(&chained_alias).unwrap().unwrap().1,
        journal
    );
    let archive = store.archive(&journal).unwrap();
    let archived_alias = aliases.join("archived.toml");
    symlink(archive.join("config.toml"), &archived_alias).unwrap();
    assert!(journal_for_config(&archived_alias).is_err());
}

#[cfg(unix)]
#[test]
fn managed_config_cannot_be_redirected_to_an_ordinary_config() {
    use std::os::unix::fs::symlink;

    let (directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    fs::remove_file(&journal.settings_path).unwrap();
    symlink(&store.settings_path, &journal.settings_path).unwrap();
    assert!(journal_for_config(&journal.settings_path).is_err());
    let alias = directory.path().join("alias.toml");
    symlink(&journal.settings_path, &alias).unwrap();
    assert!(journal_for_config(&alias).is_err());
}

#[test]
fn names_are_validated_and_never_used_as_paths() {
    let (directory, store) = fixture();
    for name in [
        "",
        "  ",
        "\nWork",
        "Work\r",
        "Work\0Private",
        "\u{1b}[31mRed",
    ] {
        assert!(store.create(name, &Settings::default()).is_err());
    }
    assert!(store.create(&"x".repeat(81), &Settings::default()).is_err());
    for name in ["personal", " PERSONAL "] {
        assert!(store.create(name, &Settings::default()).is_err());
    }
    store.create("Work", &Settings::default()).unwrap();
    assert!(store.create("work", &Settings::default()).is_err());
    let journal = store.create("../../outside", &Settings::default()).unwrap();
    assert!(journal.settings_path.starts_with(store.root()));
    assert!(valid_id(&journal.id));
    assert!(!directory.path().join("outside").exists());
    let mut forged = journal.clone();
    forged.id = "../outside".into();
    assert!(store.acquire(&forged).is_err());
    assert!(store.archive(&forged).is_err());
    forged = journal.clone();
    forged.settings_path = store.settings_path.clone();
    assert!(store.load_settings(&forged).is_err());
    assert!(store.archive(&forged).is_err());
    assert!(journal.settings_path.exists());
}

#[test]
fn catalog_guard_releases_the_lock_while_a_duplicate_descriptor_remains_open() {
    let (_directory, store) = fixture();
    let guard = store.catalog_guard().unwrap();
    // A subprocess can briefly inherit this descriptor before exec closes it.
    let inherited = guard.lock.try_clone().unwrap();
    assert!(store.catalog_guard().is_err());
    drop(guard);
    let next = store.catalog_guard().unwrap();
    assert!(store.catalog_guard().is_err());
    drop((next, inherited));
}

#[test]
fn journal_lease_releases_the_lock_while_a_duplicate_descriptor_remains_open() {
    let (_directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let lease = store.acquire(&journal).unwrap();
    let inherited = lease.lock.try_clone().unwrap();
    assert!(store.acquire(&journal).is_err());
    drop(lease);
    let next = store.acquire(&journal).unwrap();
    assert!(store.acquire(&journal).is_err());
    drop((next, inherited));
}

#[test]
fn leases_prevent_opening_or_archiving_busy_journals_and_are_released_on_drop() {
    let (_directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let lease = store.acquire(&journal).unwrap();
    assert!(store.acquire(&journal).is_err());
    assert!(store.archive(&journal).is_err());
    assert!(journal.settings_path.exists());
    let (direct_store, direct_journal) =
        journal_for_config(&journal.settings_path).unwrap().unwrap();
    assert!(direct_store.acquire(&direct_journal).is_err());
    drop(lease);
    let archived = store.archive(&journal).unwrap();
    assert!(archived.is_dir());
    assert_eq!(store.list().unwrap().len(), 1);
    assert!(store.acquire(&journal).is_err());
}

#[test]
fn archive_preserves_all_files_and_can_be_restored_without_data_changes() {
    let (_directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let profile = journal.settings_path.parent().unwrap();
    let files = [
        ("data/notes", b"Private words".as_slice()),
        ("state/state.json", b"state".as_slice()),
        ("exports/saved.txt", b"export".as_slice()),
    ];
    for (path, bytes) in &files {
        fs::write(profile.join(path), bytes).unwrap();
    }
    let config = fs::read(&journal.settings_path).unwrap();
    let archived = store.archive(&journal).unwrap();
    assert!(archived.starts_with(store.root().join(DELETED_DIRECTORY)));
    assert!(!profile.exists());
    for (path, bytes) in &files {
        assert_eq!(fs::read(archived.join(path)).unwrap(), *bytes);
    }
    assert_eq!(fs::read(archived.join("config.toml")).unwrap(), config);
    assert!(journal_for_config(&archived.join("config.toml")).is_err());
    fs::rename(&archived, profile).unwrap();
    assert_eq!(store.list().unwrap()[1], journal);
    assert_isolated(&store.load_settings(&journal).unwrap(), profile);
}

#[test]
fn external_storage_is_never_archived_even_for_a_disabled_backend() {
    let (directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let outside = directory.path().join("outside.json");
    fs::write(&outside, "keep this data").unwrap();
    fs::write(
        &journal.settings_path,
        format!("[json_backend]\nfile_path = {:?}\n", outside),
    )
    .unwrap();
    assert!(
        store
            .archive(&journal)
            .unwrap_err()
            .to_string()
            .contains("outside its managed folder")
    );
    assert_eq!(fs::read_to_string(&outside).unwrap(), "keep this data");
    assert!(journal.settings_path.exists());
}

#[test]
fn metadata_changes_cannot_redirect_a_selected_journal() {
    let (_directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let metadata = journal.settings_path.with_file_name(METADATA_FILE);
    let mut record = read_record(&metadata).unwrap();
    record.id = "../outside".into();
    fs::write(&metadata, toml::to_string(&record).unwrap()).unwrap();
    assert!(store.list().is_err());
    assert!(store.acquire(&journal).is_err());
    assert!(store.archive(&journal).is_err());
    assert!(apply_profile_defaults(&mut Settings::default(), &journal.settings_path).is_err());
}

#[cfg(unix)]
#[test]
fn managed_directories_metadata_and_lock_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;
    for component in [
        "profile", "metadata", "config", "leases", "catalog", "deleted", "data",
    ] {
        let (directory, store) = fixture();
        let journal = store.create("Work", &Settings::default()).unwrap();
        let profile = journal.settings_path.parent().unwrap();
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), "untouched").unwrap();
        match component {
            "profile" => {
                fs::rename(profile, outside.join("original")).unwrap();
                symlink(outside.join("original"), profile).unwrap();
            }
            "metadata" | "config" => {
                let name = if component == "metadata" {
                    METADATA_FILE
                } else {
                    "config.toml"
                };
                fs::rename(profile.join(name), outside.join(name)).unwrap();
                symlink(outside.join(name), profile.join(name)).unwrap();
            }
            "leases" => symlink(&outside, store.root().join(".leases")).unwrap(),
            "catalog" => {
                fs::remove_file(store.root().join(".catalog.lock")).unwrap();
                symlink(outside.join("keep"), store.root().join(".catalog.lock")).unwrap();
            }
            "deleted" => symlink(&outside, store.root().join(DELETED_DIRECTORY)).unwrap(),
            "data" => {
                fs::remove_dir(profile.join("data")).unwrap();
                symlink(&outside, profile.join("data")).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            store.archive(&journal).is_err(),
            "Accepted {component} symlink"
        );
        assert_eq!(
            fs::read_to_string(outside.join("keep")).unwrap(),
            "untouched"
        );
    }
    let (directory, store) = fixture();
    let outside = directory.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, store.root()).unwrap();
    assert!(JournalStore::new(store.settings_path.clone()).is_err());
    assert!(store.create("Work", &Settings::default()).is_err());
    assert!(fs::read_dir(outside).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn new_profile_files_and_directories_have_private_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (_directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let profile = journal.settings_path.parent().unwrap();
    for directory in [
        store.root().to_owned(),
        profile.to_owned(),
        profile.join("data"),
        profile.join("state"),
        profile.join("exports"),
    ] {
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    for file in [
        journal.settings_path.clone(),
        profile.join(METADATA_FILE),
        store.root().join(".catalog.lock"),
    ] {
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

fn available_backends() -> Vec<BackendType> {
    vec![
        #[cfg(feature = "json")]
        BackendType::Json,
        #[cfg(feature = "sqlite")]
        BackendType::Sqlite,
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal,
        #[cfg(feature = "file")]
        BackendType::File,
    ]
}

fn select_storage(settings: &mut Settings, backend: BackendType, path: PathBuf) {
    settings.backend_type = Some(backend);
    match backend {
        #[cfg(feature = "json")]
        BackendType::Json => settings.json_backend.file_path = Some(path),
        #[cfg(feature = "sqlite")]
        BackendType::Sqlite => settings.sqlite_backend.file_path = Some(path),
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal => settings.vjournal_backend.directory = Some(path),
        #[cfg(feature = "file")]
        BackendType::File => settings.file_backend.directory = Some(path),
        #[allow(unreachable_patterns)]
        _ => panic!("Unavailable test backend"),
    }
}

#[test]
fn direct_storage_paths_acquire_the_same_journal_lease_for_every_backend() {
    let (_directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let storage = journal
        .settings_path
        .parent()
        .unwrap()
        .join("data/not-created/deep/store");
    for backend in available_backends() {
        let mut settings = Settings::default();
        select_storage(&mut settings, backend, storage.clone());
        let (owner, found) = journal_for_storage(&settings).unwrap().unwrap();
        assert_eq!(owner.root(), store.root());
        assert_eq!(found, journal);
        let lease = owner.acquire(&found).unwrap();
        assert!(store.archive(&journal).is_err());
        drop(lease);
        assert!(!storage.parent().unwrap().exists());
    }
}

#[test]
fn storage_detection_ignores_inactive_backends_and_unrelated_metadata() {
    let (directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let ordinary = directory.path().join("ordinary");
    fs::create_dir(&ordinary).unwrap();
    fs::write(ordinary.join(METADATA_FILE), "unrelated = true\n").unwrap();
    for backend in available_backends() {
        let mut settings = store.load_settings(&journal).unwrap();
        select_storage(&mut settings, backend, ordinary.join("missing/data"));
        assert!(journal_for_storage(&settings).unwrap().is_none());
        assert!(!ordinary.join("missing").exists());
    }
}

#[test]
fn default_backend_paths_can_be_inspected_without_a_configured_path() {
    for backend in available_backends() {
        let settings = Settings {
            backend_type: Some(backend),
            ..Default::default()
        };
        let before = toml::to_string(&settings).unwrap();
        journal_for_storage(&settings).unwrap();
        assert_eq!(toml::to_string(&settings).unwrap(), before);
    }
}

#[cfg(unix)]
#[test]
fn storage_detection_resolves_symlink_ancestors_before_finding_the_owner() {
    use std::os::unix::fs::symlink;
    let (directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let alias = directory.path().join("alias");
    let data = journal.settings_path.parent().unwrap().join("data");
    symlink(&data, &alias).unwrap();
    for backend in available_backends() {
        let mut settings = Settings::default();
        select_storage(&mut settings, backend, alias.join("missing/store"));
        assert_eq!(journal_for_storage(&settings).unwrap().unwrap().1, journal);
        assert!(!data.join("missing").exists());
    }
}

#[test]
fn archived_and_stale_original_storage_paths_fail_without_recreating_files() {
    let (_directory, store) = fixture();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let original = journal.settings_path.parent().unwrap();
    let archived = store.archive(&journal).unwrap();
    for path in [
        original.to_owned(),
        journal.settings_path.clone(),
        archived.clone(),
        archived.join("config.toml"),
    ] {
        assert!(journal_for_config(&path).is_err());
    }
    assert!(!original.exists());
    for backend in available_backends() {
        for root in [original, archived.as_path()] {
            let mut settings = Settings::default();
            select_storage(&mut settings, backend, root.join("data/missing/store"));
            let error = match journal_for_storage(&settings) {
                Err(error) => error,
                Ok(_) => panic!("Deleted journal storage was accepted"),
            };
            assert!(
                error
                    .to_string()
                    .contains("restore it from Deleted Journals")
            );
            assert!(!root.join("data/missing").exists());
        }
        assert!(!original.exists());
    }
    fs::create_dir_all(original.join("data")).unwrap();
    for backend in available_backends() {
        let mut settings = Settings::default();
        select_storage(&mut settings, backend, original.join("data/store"));
        assert!(journal_for_storage(&settings).is_err());
        assert!(
            fs::read_dir(original.join("data"))
                .unwrap()
                .next()
                .is_none()
        );
    }
    assert!(archived.join("config.toml").exists());
}
