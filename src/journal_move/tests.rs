use super::*;
use backend::{
    DataProvider, Entry, EntryDraft,
    protection::{self, SecretString},
};

struct Fixture {
    root: tempfile::TempDir,
    settings: Settings,
    context: ConfigContext,
    document: ConfigDocument,
    original: Entry,
}

fn backends() -> Vec<BackendType> {
    vec![
        #[cfg(feature = "json")]
        BackendType::Json,
        #[cfg(feature = "sqlite")]
        BackendType::Sqlite,
        #[cfg(feature = "file")]
        BackendType::File,
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal,
    ]
}

fn password() -> SecretString {
    "move test password".to_owned().into()
}

async fn fixture(backend_type: BackendType, protection: u8) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    fs::create_dir(&profile).unwrap();
    let mut settings: Settings =
        toml::from_str("auto_title = 'Daily title'\ncustom_title = 'Remembered title'\n").unwrap();
    settings.backend_type = Some(backend_type);
    settings.app_state_dir = Some(profile.join("state"));
    settings.export.default_path = Some(profile.join("exports"));
    #[cfg(feature = "json")]
    {
        settings.json_backend.file_path = Some(root.path().join("data/entries.json"));
    }
    #[cfg(feature = "sqlite")]
    {
        settings.sqlite_backend.file_path = Some(root.path().join("data/entries.db"));
    }
    #[cfg(feature = "file")]
    {
        settings.file_backend.directory = Some(root.path().join("data/entries"));
    }
    #[cfg(feature = "vjournal")]
    {
        settings.vjournal_backend.directory = Some(root.path().join("data/calendar"));
    }
    let storage = crate::journal_storage::backend(&settings).unwrap();
    let mut provider = JournalProvider::open(storage).await.unwrap();
    let entry = provider
        .add_entry(
            EntryDraft::new(
                chrono::DateTime::parse_from_rfc3339("2026-10-28T02:03:04Z")
                    .unwrap()
                    .to_utc(),
                "Original title".into(),
                vec!["original-tag".into()],
                Some(2),
            )
            .with_content("PRIVATE_MOVE_BODY\nSecond line.".into()),
        )
        .await
        .unwrap();
    if protection == 1 {
        let native = provider.protection_metadata(entry.id).await.unwrap();
        let envelope = protection::protect(&entry, &password(), native).unwrap();
        provider
            .update_entry(entry.with_protection(envelope))
            .await
            .unwrap();
    } else if protection == 2 {
        provider
            .enable_journal_protection(password(), None)
            .await
            .unwrap();
    }
    let original = provider.load_all_entries().await.unwrap().remove(0);
    provider.close_for_move().await.unwrap();
    drop(provider);
    let context = ConfigContext {
        journal_name: Some("Personal".into()),
        return_to_journals: true,
        storage_overridden: false,
        settings_path: profile.join("config.toml"),
        themes_path: profile.join("themes"),
        active_theme_id: Some("tui".into()),
        theme_warning: None,
        launch_details: Vec::new(),
    };
    let text = format!(
        "# Keep my settings\n{}\n[future_backend]\noption = 'keep'\n",
        settings.get_as_text().unwrap()
    );
    fs::write(&context.settings_path, text).unwrap();
    let document = ConfigDocument::load(context.settings_path.clone()).unwrap();
    Fixture {
        root,
        settings,
        context,
        document,
        original,
    }
}

fn plan(fixture: &Fixture) -> MoveRequest {
    prepare(
        &fixture.settings,
        &fixture.context,
        &fixture.document,
        &fixture.root.path().join("moved"),
    )
    .unwrap()
}

fn same_entry(mut actual: Entry, expected: &Entry) {
    actual.id = expected.id;
    assert_eq!(&actual, expected);
}

#[tokio::test]
async fn every_backend_moves_plain_and_protected_journals_with_recoverable_originals() {
    for backend in backends() {
        for protected in [false, true] {
            let fixture = fixture(backend, if protected { 2 } else { 0 }).await;
            let source_backend = crate::journal_storage::backend(&fixture.settings).unwrap();
            let header = fs::read(source_backend.header_path()).ok();
            let mut source = JournalProvider::open(source_backend).await.unwrap();
            if protected {
                source.unlock(password()).await.unwrap();
            }
            same_entry(
                source.load_all_entries().await.unwrap().remove(0),
                &fixture.original,
            );
            let request = plan(&fixture);
            let original_path = request.source.clone();
            source.close_for_move().await.unwrap();
            drop(source);
            let report = execute(request).await.unwrap();
            assert!(report.warning.is_none(), "{:?}", report.warning);
            assert!(!original_path.exists());
            assert!(
                report
                    .recovery_path
                    .join(original_path.file_name().unwrap())
                    .exists()
            );
            let saved = fs::read_to_string(&fixture.context.settings_path).unwrap();
            assert!(saved.contains("# Keep my settings"));
            assert!(saved.contains("[future_backend]"));
            let settings =
                Settings::from_config_text(&saved, &fixture.context.settings_path).unwrap();
            assert_eq!(settings.auto_title, fixture.settings.auto_title);
            assert_eq!(settings.custom_title, fixture.settings.custom_title);
            assert_eq!(settings.app_state_dir, fixture.settings.app_state_dir);
            let moved_backend = crate::journal_storage::backend(&settings).unwrap();
            assert_eq!(moved_backend.path(), report.destination);
            assert_eq!(fs::read(moved_backend.header_path()).ok(), header);
            let mut provider = JournalProvider::open(moved_backend).await.unwrap();
            assert_eq!(provider.is_locked(), protected);
            if protected {
                provider.unlock(password()).await.unwrap();
            }
            same_entry(
                provider.load_all_entries().await.unwrap().remove(0),
                &fixture.original,
            );
            provider.close_for_move().await.unwrap();
            drop(provider);
            let mut recovery_settings = settings;
            set_storage(
                &mut recovery_settings,
                report
                    .recovery_path
                    .join(original_path.file_name().unwrap()),
            );
            let mut recovery =
                JournalProvider::open(crate::journal_storage::backend(&recovery_settings).unwrap())
                    .await
                    .unwrap();
            if protected {
                recovery.unlock(password()).await.unwrap();
            }
            same_entry(
                recovery.load_all_entries().await.unwrap().remove(0),
                &fixture.original,
            );
            recovery.close_for_move().await.unwrap();
        }
    }
}

#[tokio::test]
async fn moving_keeps_individual_entry_ciphertext_and_passwords() {
    for backend in backends() {
        let fixture = fixture(backend, 1).await;
        let report = execute(plan(&fixture)).await.unwrap();
        let mut settings = fixture.settings.clone();
        set_storage(&mut settings, report.destination);
        let mut provider =
            JournalProvider::open(crate::journal_storage::backend(&settings).unwrap())
                .await
                .unwrap();
        assert!(!provider.is_journal_protected());
        let entry = provider.load_all_entries().await.unwrap().remove(0);
        assert_eq!(entry.protection, fixture.original.protection);
        assert_eq!(
            protection::unlock(&entry, &password()).unwrap().content,
            "PRIVATE_MOVE_BODY\nSecond line."
        );
        provider.close_for_move().await.unwrap();
    }
}

#[cfg(feature = "file")]
#[tokio::test]
async fn legacy_file_documents_and_unrelated_files_are_copied_without_rewriting() {
    let mut fixture = fixture(BackendType::File, 0).await;
    let source = crate::journal_storage::backend(&fixture.settings).unwrap();
    let filename = fs::read_dir(source.path())
        .unwrap()
        .map(|item| item.unwrap().path())
        .find(|path| path.file_name().unwrap() != ".tui-journal.lock")
        .unwrap();
    let text = "---\ntitle: Legacy entry\ndate: 2026-10-01T02:00:00Z\ncreated: 2026-10-01T12:00:00+10:00\ntags: [legacy]\ntui_journal:\n  version: 1\n  id: 11e02e5c-7cbf-4828-8a46-8c8712461e00\nextra_property: preserve me\n---\nLegacy body\n";
    fs::write(&filename, text).unwrap();
    fs::write(
        source.path().join(".sync.ffs_db"),
        "Keep this unrelated metadata",
    )
    .unwrap();
    fs::create_dir(source.path().join("attachments")).unwrap();
    fs::write(source.path().join("attachments/extra.bin"), [0, 1, 255]).unwrap();
    fixture.document = ConfigDocument::load(fixture.context.settings_path.clone()).unwrap();
    let report = execute(plan(&fixture)).await.unwrap();
    assert_eq!(
        fs::read_to_string(report.destination.join(filename.file_name().unwrap())).unwrap(),
        text
    );
    assert_eq!(
        fs::read(report.destination.join("attachments/extra.bin")).unwrap(),
        [0, 1, 255]
    );
    assert_eq!(
        fs::read_to_string(report.destination.join(".sync.ffs_db")).unwrap(),
        "Keep this unrelated metadata"
    );
    assert_eq!(
        fs::read_to_string(
            report
                .recovery_path
                .join(source.path().file_name().unwrap())
                .join(filename.file_name().unwrap())
        )
        .unwrap(),
        text
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(report.destination.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(report.destination.join(filename.file_name().unwrap()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(report.recovery_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[tokio::test]
async fn occupied_destinations_and_changed_configuration_leave_originals_active() {
    let fixture = fixture(backends()[0], 0).await;
    let request = plan(&fixture);
    let source = request.source.clone();
    let original_config = fixture.document.text();
    fs::create_dir(&request.destination).unwrap();
    fs::write(
        request.destination.join("user-file"),
        "Unrelated destination data",
    )
    .unwrap();
    assert!(execute(request).await.is_err());
    assert!(source.exists());
    assert_eq!(
        fs::read_to_string(&fixture.context.settings_path).unwrap(),
        original_config
    );
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("moved/user-file")).unwrap(),
        "Unrelated destination data"
    );

    let request = prepare(
        &fixture.settings,
        &fixture.context,
        &fixture.document,
        &fixture.root.path().join("another"),
    )
    .unwrap();
    fs::write(
        &fixture.context.settings_path,
        format!("{original_config}\n# external change\n"),
    )
    .unwrap();
    assert!(execute(request).await.is_err());
    assert!(source.exists());
    assert!(!fixture.root.path().join("another").exists());
    assert!(
        fs::read_to_string(&fixture.context.settings_path)
            .unwrap()
            .contains("external change")
    );
}

#[tokio::test]
async fn busy_journals_and_pending_conversions_are_not_moved() {
    for kind in backends() {
        let fixture = fixture(kind, 0).await;
        let request = plan(&fixture);
        let source = request.source.clone();
        let backend = crate::journal_storage::backend(&fixture.settings).unwrap();
        let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
        assert!(execute(request).await.is_err());
        assert!(source.exists());
        provider.close_for_move().await.unwrap();
        drop(provider);
        fs::write(backend.pending_path(), "incomplete conversion").unwrap();
        assert!(
            prepare(
                &fixture.settings,
                &fixture.context,
                &fixture.document,
                &fixture.root.path().join("moved")
            )
            .is_err()
        );
        assert!(!fixture.root.path().join("moved").exists());
    }
}

#[cfg(feature = "file")]
#[tokio::test]
async fn source_changes_after_confirmation_preserve_the_changed_original() {
    let fixture = fixture(BackendType::File, 0).await;
    let request = plan(&fixture);
    fs::write(request.source.join("new-file"), "External addition").unwrap();
    let path = request.source.clone();
    assert!(execute(request).await.is_err());
    assert_eq!(
        fs::read_to_string(path.join("new-file")).unwrap(),
        "External addition"
    );
    assert_eq!(
        fs::read_to_string(&fixture.context.settings_path).unwrap(),
        fixture.document.text()
    );
    assert!(!fixture.root.path().join("moved").exists());
}

#[tokio::test]
async fn unsafe_and_ambiguous_locations_are_rejected_without_creating_files() {
    let fixture = fixture(backends()[0], 0).await;
    let mut context = fixture.context.clone();
    context.storage_overridden = true;
    let target = fixture.root.path().join("moved");
    assert!(prepare(&fixture.settings, &context, &fixture.document, &target).is_err());
    let mut dirty = fixture.document.clone();
    dirty.set(&["auto_title"], Some("Unsaved".into())).unwrap();
    assert!(prepare(&fixture.settings, &fixture.context, &dirty, &target).is_err());
    assert!(
        prepare(
            &fixture.settings,
            &fixture.context,
            &fixture.document,
            Path::new("relative")
        )
        .is_err()
    );
    for name in ["themes", "state", "exports"] {
        let parent = fixture.context.settings_path.parent().unwrap().join(name);
        fs::create_dir(&parent).unwrap();
        assert!(
            prepare(
                &fixture.settings,
                &fixture.context,
                &fixture.document,
                &parent.join("moved")
            )
            .is_err()
        );
    }
    let mut changed = fixture.settings.clone();
    set_storage(&mut changed, fixture.root.path().join("different-source"));
    assert!(prepare(&changed, &fixture.context, &fixture.document, &target).is_err());
    assert!(!target.exists());
}

#[tokio::test]
async fn redirected_storage_cannot_move_another_managed_journals_files() {
    for backend in backends() {
        let mut fixture = fixture(backend, 0).await;
        let store =
            crate::journals::JournalStore::new(fixture.context.settings_path.clone()).unwrap();
        let owner = store.create("Owner", &fixture.settings).unwrap();
        let owner_settings = store.load_settings(&owner).unwrap();
        let owner_backend = crate::journal_storage::backend(&owner_settings).unwrap();
        let mut provider = JournalProvider::open(owner_backend.clone()).await.unwrap();
        provider
            .add_entry(EntryDraft::from_entry(fixture.original.clone()))
            .await
            .unwrap();
        provider.close_for_move().await.unwrap();
        drop(provider);
        let original_owner_config = fs::read(&owner.settings_path).unwrap();
        let (owner_data, _) = sources(owner_backend.path(), &owner_settings).unwrap();
        let original_owner_data = Snapshot::read(&owner_data).unwrap();

        set_storage(&mut fixture.settings, owner_backend.path().to_path_buf());
        fs::write(
            &fixture.context.settings_path,
            fixture.settings.get_as_text().unwrap(),
        )
        .unwrap();
        fixture.document = ConfigDocument::load(fixture.context.settings_path.clone()).unwrap();
        let destination = fixture.root.path().join("moved");
        let error = prepare(
            &fixture.settings,
            &fixture.context,
            &fixture.document,
            &destination,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("belongs to another journal"),
            "{error}"
        );
        assert!(!destination.exists());
        assert_eq!(
            fs::read(&owner.settings_path).unwrap(),
            original_owner_config
        );
        original_owner_data.verify(&owner_data).unwrap();
        assert_eq!(
            fs::read_to_string(&fixture.context.settings_path).unwrap(),
            fixture.document.text()
        );

        let mut owner_context = fixture.context.clone();
        owner_context.settings_path = owner.settings_path.clone();
        owner_context.journal_name = Some(owner.name);
        let owner_document = ConfigDocument::load(owner.settings_path).unwrap();
        assert!(
            prepare(
                &owner_settings,
                &owner_context,
                &owner_document,
                &destination
            )
            .is_ok()
        );
    }
}

#[cfg(all(feature = "file", unix))]
#[tokio::test]
async fn aliases_and_symlinks_cannot_move_storage_into_itself_or_copy_external_files() {
    use std::os::unix::fs::symlink;
    let fixture = fixture(BackendType::File, 0).await;
    let source = crate::journal_storage::backend(&fixture.settings).unwrap();
    let alias = fixture.root.path().join("alias");
    symlink(source.path(), &alias).unwrap();
    assert!(
        prepare(
            &fixture.settings,
            &fixture.context,
            &fixture.document,
            &alias.join("nested")
        )
        .is_err()
    );
    let unrelated = fixture.root.path().join("private-file");
    fs::write(&unrelated, "Never copy through a symlink").unwrap();
    symlink(&unrelated, source.path().join("link")).unwrap();
    assert!(
        prepare(
            &fixture.settings,
            &fixture.context,
            &fixture.document,
            &fixture.root.path().join("moved")
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(unrelated).unwrap(),
        "Never copy through a symlink"
    );
}

#[test]
fn atomic_publication_refuses_even_an_empty_existing_destination() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("stage");
    let destination = root.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("data"), "Keep source").unwrap();
    fs::create_dir(&destination).unwrap();
    assert!(rename_noclobber(&source, &destination).is_err());
    assert!(destination.is_dir());
    assert_eq!(
        fs::read_to_string(source.join("data")).unwrap(),
        "Keep source"
    );
    assert!(fs::read_dir(destination).unwrap().next().is_none());
}

#[cfg(all(feature = "file", unix))]
#[tokio::test]
async fn reserved_directory_aliases_inside_source_cannot_be_stranded_by_a_move() {
    use std::os::unix::fs::symlink;
    for name in ["themes", "state", "exports"] {
        let fixture = fixture(BackendType::File, 0).await;
        let source = crate::journal_storage::backend(&fixture.settings).unwrap();
        let target = source.path().join(name);
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), "Reserved application data").unwrap();
        let alias = fixture.context.settings_path.parent().unwrap().join(name);
        symlink(&target, &alias).unwrap();
        let destination = fixture.root.path().join("moved");
        let original_config = fs::read(&fixture.context.settings_path).unwrap();
        let error = prepare(
            &fixture.settings,
            &fixture.context,
            &fixture.document,
            &destination,
        )
        .unwrap_err();
        assert!(error.to_string().contains("contains configuration"));
        assert_eq!(
            fs::read(&fixture.context.settings_path).unwrap(),
            original_config
        );
        assert_eq!(
            fs::read_to_string(alias.join("keep")).unwrap(),
            "Reserved application data"
        );
        assert!(source.path().is_dir());
        assert!(!destination.exists());
    }
}

#[cfg(all(feature = "file", unix))]
#[tokio::test]
async fn configuration_alias_into_source_is_rejected_before_copying() {
    use std::os::unix::fs::symlink;
    let mut fixture = fixture(BackendType::File, 0).await;
    let source = crate::journal_storage::backend(&fixture.settings).unwrap();
    let original_config = fs::read(&fixture.context.settings_path).unwrap();
    let target = source.path().join("config.toml");
    fs::rename(&fixture.context.settings_path, &target).unwrap();
    symlink(&target, &fixture.context.settings_path).unwrap();
    fixture.document = ConfigDocument::load(fixture.context.settings_path.clone()).unwrap();
    let destination = fixture.root.path().join("moved");
    let error = prepare(
        &fixture.settings,
        &fixture.context,
        &fixture.document,
        &destination,
    )
    .unwrap_err();
    assert!(error.to_string().contains("contains configuration"));
    assert_eq!(
        fs::read(&fixture.context.settings_path).unwrap(),
        original_config
    );
    assert_eq!(fs::read(target).unwrap(), original_config);
    assert!(source.path().is_dir());
    assert!(!destination.exists());
}

#[cfg(all(feature = "file", feature = "json"))]
#[tokio::test]
async fn existing_inactive_backend_inside_source_cannot_be_stranded_by_a_move() {
    let mut fixture = fixture(BackendType::File, 0).await;
    let source = crate::journal_storage::backend(&fixture.settings).unwrap();
    let inactive = source.path().join("entries.json");
    fs::write(&inactive, "[]").unwrap();
    fixture.settings.json_backend.file_path = Some(inactive.clone());
    fs::write(
        &fixture.context.settings_path,
        fixture.settings.get_as_text().unwrap(),
    )
    .unwrap();
    fixture.document = ConfigDocument::load(fixture.context.settings_path.clone()).unwrap();
    let error = prepare(
        &fixture.settings,
        &fixture.context,
        &fixture.document,
        &fixture.root.path().join("moved"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("another backend"));
    assert_eq!(fs::read_to_string(inactive).unwrap(), "[]");
    assert!(!fixture.root.path().join("moved").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn configuration_symlink_is_preserved_and_its_real_target_is_updated() {
    use std::os::unix::fs::symlink;
    let mut fixture = fixture(backends()[0], 0).await;
    let target_parent = fixture.root.path().join("actual-config");
    fs::create_dir(&target_parent).unwrap();
    let target = target_parent.join("config.toml");
    fs::rename(&fixture.context.settings_path, &target).unwrap();
    symlink(&target, &fixture.context.settings_path).unwrap();
    fixture.document = ConfigDocument::load(fixture.context.settings_path.clone()).unwrap();
    let report = execute(plan(&fixture)).await.unwrap();
    assert!(report.warning.is_none(), "{:?}", report.warning);
    assert!(
        fs::symlink_metadata(&fixture.context.settings_path)
            .unwrap()
            .is_symlink()
    );
    let saved = Settings::from_config_text(
        &fs::read_to_string(target).unwrap(),
        &fixture.context.settings_path,
    )
    .unwrap();
    assert_eq!(
        crate::journal_storage::backend(&saved).unwrap().path(),
        report.destination
    );
    assert_eq!(saved.auto_title, fixture.settings.auto_title);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_move_reservation_excludes_other_writers_until_closed() {
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    let fixture = fixture(BackendType::Sqlite, 0).await;
    let source = crate::journal_storage::backend(&fixture.settings).unwrap();
    let guard = SqliteMoveGuard::acquire(source.path(), BackendType::Sqlite)
        .await
        .unwrap();
    let options = SqliteConnectOptions::new()
        .filename(source.path())
        .create_if_missing(false)
        .busy_timeout(std::time::Duration::ZERO);
    let mut other = SqliteConnection::connect_with(&options).await.unwrap();
    assert!(
        sqlx::query("UPDATE entries SET title = 'Concurrent edit'")
            .execute(&mut other)
            .await
            .is_err()
    );
    guard.close().await.unwrap();
    sqlx::query("UPDATE entries SET title = 'Concurrent edit'")
        .execute(&mut other)
        .await
        .unwrap();
    other.close().await.unwrap();
}
