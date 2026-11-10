use super::*;
use crate::app::test::create_default_app;

#[test]
fn exports_cannot_overwrite_another_backend_or_journals_key_header() {
    let directory = tempfile::tempdir().unwrap();
    let app = create_default_app();
    let names = [
        "inactive.json.myna-lock.json",
        "other-journal.myna-lock.pending.json",
        "other-journal.myna-lock.lease",
        "inactive.json.MYNA-LOCK.JSON",
        "other-journal.MyNa-Lock.Pending.Json",
        "other-journal.MYNA-LOCK.LEASE",
    ];
    for name in names {
        let path = directory.path().join(name);
        fs::write(&path, b"original journal protection").unwrap();
        let confirmed = ExportTarget::inspect(path.clone()).unwrap();
        assert!(app.validate_export_target(&confirmed).is_err());
        assert_eq!(fs::read(path).unwrap(), b"original journal protection");
    }
}

#[cfg(all(unix, feature = "file"))]
#[test]
fn exports_cannot_bypass_native_directory_guards_with_case_aliases() {
    use std::os::unix::fs::MetadataExt;
    let directory = tempfile::tempdir().unwrap();
    let native = directory.path().join("entries");
    fs::create_dir(&native).unwrap();
    let alias = directory.path().join("ENTRIES");
    let original = fs::metadata(&native).unwrap();
    if !fs::metadata(&alias)
        .is_ok_and(|other| original.dev() == other.dev() && original.ino() == other.ino())
    {
        return;
    }
    fs::write(native.join("entry"), "original journal data").unwrap();
    let mut app = create_default_app();
    app.settings.backend_type = Some(BackendType::File);
    app.settings.file_backend.directory = Some(native.clone());
    for path in [alias.join("entry"), alias.join("new/subdirectory/entry")] {
        assert!(
            app.validate_export_target(&ExportTarget::from(path))
                .is_err()
        );
    }
    assert_eq!(
        fs::read_to_string(native.join("entry")).unwrap(),
        "original journal data"
    );
}

#[test]
fn journal_collection_exports_cannot_replace_metadata_archives_or_other_journals() {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::journals::JournalStore::new(directory.path().join("config.toml")).unwrap();
    let work = store.create("Work", &Settings::default()).unwrap();
    let travel = store.create("Travel", &Settings::default()).unwrap();
    let profile = work.settings_path.parent().unwrap();
    let archived = store.archive(&travel).unwrap();
    let mut app = create_default_app();
    app.settings = store.load_settings(&work).unwrap();
    app.config_context = Some(crate::settings::ConfigContext {
        return_to_journals: false,
        storage_overridden: false,
        journal_name: Some(work.name.clone()),
        settings_path: work.settings_path.clone(),
        themes_path: directory.path().join("themes"),
        active_theme_id: None,
        theme_warning: None,
        launch_details: Vec::new(),
    });
    for path in [
        profile.join("journal.toml"),
        profile.join("config.toml"),
        profile.join("data/inactive-backend"),
        profile.join("state/state.json"),
        store.root().join(".catalog.lock"),
        store.root().join(".leases/personal.lock"),
        travel.settings_path,
        archived.join("journal.toml"),
    ] {
        assert!(
            app.validate_export_target(&ExportTarget::from(path.clone()))
                .is_err(),
            "{}",
            path.display()
        );
    }
    for path in [
        profile.join("exports/note.txt"),
        directory.path().join("note.txt"),
    ] {
        assert!(
            app.validate_export_target(&ExportTarget::from(path))
                .is_ok()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let alias = store.root().with_file_name("JOURNALS");
        let original = fs::metadata(store.root()).unwrap();
        if fs::metadata(&alias)
            .is_ok_and(|other| original.dev() == other.dev() && original.ino() == other.ino())
        {
            let profile_alias = alias.join(profile.strip_prefix(store.root()).unwrap());
            for path in [
                profile_alias.join("data/inactive-backend"),
                alias.join("other-journal/data/entry"),
                alias.join(".leases/personal.lock"),
            ] {
                assert!(
                    app.validate_export_target(&ExportTarget::from(path))
                        .is_err()
                );
            }
            assert!(
                app.validate_export_target(&ExportTarget::from(
                    profile_alias.join("exports/note.txt")
                ))
                .is_ok()
            );
        }
        fs::remove_dir(profile.join("exports")).unwrap();
        std::os::unix::fs::symlink(store.root().join(".leases"), profile.join("exports")).unwrap();
        assert!(
            app.validate_export_target(&ExportTarget::from(profile.join("exports/personal.lock")))
                .is_err()
        );
    }
}

#[test]
fn export_requires_confirmation_and_preserves_previous_file_on_write_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.txt");
    fs::write(&path, "original").unwrap();
    let unconfirmed = ExportTarget::from(path.clone());
    assert!(
        unconfirmed
            .write(|file| Ok(file.write_all(b"replacement")?))
            .is_err()
    );
    let confirmed = ExportTarget::inspect(path.clone()).unwrap();
    assert!(confirmed.replaces_file());
    assert!(
        confirmed
            .write(|file| {
                file.write_all(b"partial")?;
                anyhow::bail!("simulated write failure")
            })
            .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    confirmed
        .write(|file| Ok(file.write_all(b"complete")?))
        .unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "complete");
}

#[test]
fn export_rejects_destination_changed_since_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing");
    fs::write(&path, "original").unwrap();
    let confirmed = ExportTarget::inspect(path.clone()).unwrap();
    fs::write(&path, "changed outside the app").unwrap();
    assert!(
        confirmed
            .write(|file| Ok(file.write_all(b"replacement")?))
            .is_err()
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "changed outside the app");
    let path = dir.path().join("new");
    let target = ExportTarget::inspect(path.clone()).unwrap();
    fs::write(&path, "created outside the app").unwrap();
    assert!(
        target
            .write(|file| Ok(file.write_all(b"replacement")?))
            .is_err()
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "created outside the app");
}

#[cfg(unix)]
#[test]
fn exports_and_new_directories_are_private_without_widening_existing_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new/private/note");
    ExportTarget::from(path.clone())
        .write(|file| Ok(file.write_all(b"private")?))
        .unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    ExportTarget::inspect(path.clone())
        .unwrap()
        .write(|file| Ok(file.write_all(b"changed")?))
        .unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o400
    );
}

#[cfg(unix)]
#[test]
fn export_rejects_symbolic_link_destinations() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let alias = dir.path().join("alias");
    fs::write(&target, "keep").unwrap();
    symlink(&target, &alias).unwrap();
    assert!(ExportTarget::inspect(alias.clone()).is_err());
    assert!(
        ExportTarget::from(alias)
            .write(|file| Ok(file.write_all(b"replace")?))
            .is_err()
    );
    assert_eq!(fs::read_to_string(target).unwrap(), "keep");
}

#[cfg(unix)]
#[test]
fn export_refuses_replaced_or_newly_appeared_ancestors_after_inspection() {
    use std::os::unix::fs::symlink;
    for (existing, symlink_replacement) in [(true, true), (false, true), (true, false)] {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output");
        let previous = dir.path().join("previous-output");
        let journal = dir.path().join("journal");
        fs::create_dir(&journal).unwrap();
        if existing {
            fs::create_dir(&output).unwrap();
        }
        let target = ExportTarget::inspect(output.join("nested/export")).unwrap();
        if existing {
            fs::rename(&output, &previous).unwrap();
        }
        if symlink_replacement {
            symlink(&journal, &output).unwrap();
        } else {
            fs::create_dir(&output).unwrap();
        }
        assert!(
            target
                .write(|file| Ok(file.write_all(b"private draft")?))
                .is_err()
        );
        assert!(!journal.join("nested").exists());
        assert!(!output.join("nested").exists());
        assert!(!previous.join("nested").exists());
    }
}

#[cfg(feature = "sqlite")]
#[test]
fn active_database_sidecars_state_and_configuration_are_not_export_targets() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = create_default_app();
    app.settings.backend_type = Some(BackendType::Sqlite);
    let database = dir.path().join("entries.db");
    app.settings.sqlite_backend.file_path = Some(database.clone());
    app.settings.app_state_dir = Some(dir.path().join("state"));
    app.config_context = Some(crate::settings::ConfigContext {
        journal_name: None,
        return_to_journals: false,
        storage_overridden: false,
        settings_path: dir.path().join("config.toml"),
        themes_path: dir.path().join("themes"),
        active_theme_id: None,
        theme_warning: None,
        launch_details: Vec::new(),
    });
    for path in [
        database,
        dir.path().join("entries.db-journal"),
        dir.path().join("entries.db-wal"),
        dir.path().join("entries.db-shm"),
        dir.path().join("state/state.json"),
        dir.path().join("config.toml"),
        dir.path().join("themes/theme_tui.toml"),
        dir.path().join("themes.toml"),
    ] {
        assert!(
            app.validate_export_target(&ExportTarget::from(path.clone()))
                .is_err(),
            "{}",
            path.display()
        );
    }
    assert!(
        app.validate_export_target(&ExportTarget::from(dir.path().join("export.json")))
            .is_ok()
    );
}

#[cfg(all(unix, feature = "json"))]
#[test]
fn active_store_aliases_are_rejected() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    fs::create_dir(&store_dir).unwrap();
    let store = store_dir.join("entries.json");
    fs::write(&store, "[]").unwrap();
    let mut app = create_default_app();
    app.settings.backend_type = Some(BackendType::Json);
    app.settings.json_backend.file_path = Some(store.clone());
    let alias_dir = dir.path().join("alias-directory");
    symlink(&store_dir, &alias_dir).unwrap();
    assert!(
        app.validate_export_target(&ExportTarget::from(alias_dir.join("entries.json")))
            .is_err()
    );
    let hardlink = dir.path().join("other-name");
    fs::hard_link(&store, &hardlink).unwrap();
    assert!(
        app.validate_export_target(&ExportTarget::from(hardlink))
            .is_err()
    );
    assert!(
        app.validate_export_target(&ExportTarget::from(store_dir.join("entries.json.lock")))
            .is_err()
    );
}

#[cfg(feature = "file")]
#[test]
fn directory_stores_cannot_receive_exports_inside_their_tree() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = create_default_app();
    app.settings.backend_type = Some(BackendType::File);
    app.settings.file_backend.directory = Some(dir.path().join("entries"));
    for path in ["entries/note", "entries/subfolder/export.json"] {
        assert!(
            app.validate_export_target(&ExportTarget::from(dir.path().join(path)))
                .is_err()
        );
    }
    assert!(
        app.validate_export_target(&ExportTarget::from(
            dir.path().join("entries-other/export.json")
        ))
        .is_ok()
    );
}
