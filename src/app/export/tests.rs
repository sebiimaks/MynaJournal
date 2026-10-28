use super::*;
use crate::app::test::create_default_app;

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
