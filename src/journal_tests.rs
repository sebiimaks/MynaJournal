use std::{fs, future::Future, path::Path};

use backend::{DataProvider, Entry, EntryDraft, JournalPrompt, Mood};
use chrono::{TimeZone, Utc};
use clap::Parser;

use crate::{
    app::{state::AppState, ui::themes::ThemeStore},
    cli::{Cli, backup},
    journals::JournalStore,
    settings::{BackendType, Settings, document::ConfigDocument},
};

fn personal_settings(root: &Path, backend: BackendType) -> Settings {
    let mut settings = Settings {
        backend_type: Some(backend),
        app_state_dir: Some(root.join("personal/state")),
        ..Default::default()
    };
    settings.export.default_path = Some(root.join("personal/exports"));
    #[cfg(feature = "json")]
    {
        settings.json_backend.file_path = Some(root.join("personal/entries.json"));
    }
    #[cfg(feature = "sqlite")]
    {
        settings.sqlite_backend.file_path = Some(root.join("personal/entries.db"));
    }
    #[cfg(feature = "vjournal")]
    {
        settings.vjournal_backend.directory = Some(root.join("personal/vjournal"));
    }
    #[cfg(feature = "file")]
    {
        settings.file_backend.directory = Some(root.join("personal/files"));
    }
    settings
}

fn draft(journal: &str) -> EntryDraft {
    let mut draft = EntryDraft::new(
        Utc.with_ymd_and_hms(2026, 11, 8, 12, 0, 0).unwrap(),
        "Same title".into(),
        vec![journal.into()],
        None,
    )
    .with_content(format!("Private {journal} contents"));
    draft.mood = Some(Mood::Average);
    draft.journal_prompt = Some(JournalPrompt {
        id: "average_001".into(),
        mood: Mood::Average,
        topic: "Reflection".into(),
        text: "What mattered today?".into(),
    });
    draft
}

async fn read_one(mut provider: impl DataProvider) -> Entry {
    let mut entries = provider.load_all_entries().await.unwrap();
    assert_eq!(entries.len(), 1);
    entries.remove(0)
}

async fn separate_profiles<P, F, Fut>(backend: BackendType, open: F)
where
    P: DataProvider,
    F: Fn(Settings) -> Fut,
    Fut: Future<Output = P>,
{
    let temporary = tempfile::tempdir().unwrap();
    let config = temporary.path().join("config.toml");
    let personal = personal_settings(temporary.path(), backend);
    let original_config = toml::to_string(&personal).unwrap();
    fs::write(&config, &original_config).unwrap();
    let store = JournalStore::new(config.clone()).unwrap();
    let work = store.create("Work", &personal).unwrap();
    let travel = store.create("Travel", &personal).unwrap();
    let work_settings = Settings::new(Some(work.settings_path.clone()))
        .await
        .unwrap();
    let travel_settings = store.load_settings(&travel).unwrap();
    let work_lease = store.acquire(&work).unwrap();

    let mut personal_provider = open(personal.clone()).await;
    let mut work_provider = open(work_settings.clone()).await;
    let mut travel_provider = open(travel_settings.clone()).await;
    let original = personal_provider
        .add_entry(draft("Personal"))
        .await
        .unwrap();
    let mut work_entry = work_provider.add_entry(draft("Work")).await.unwrap();
    let travel_entry = travel_provider.add_entry(draft("Travel")).await.unwrap();
    assert_eq!(original.id, work_entry.id);
    assert_eq!(original.id, travel_entry.id);
    work_entry.content = "Edited Work contents".into();
    work_entry = work_provider.update_entry(work_entry).await.unwrap();
    travel_provider.remove_entry(travel_entry.id).await.unwrap();
    assert_eq!(
        personal_provider.load_all_entries().await.unwrap(),
        std::slice::from_ref(&original)
    );
    assert_eq!(
        work_provider.load_all_entries().await.unwrap(),
        std::slice::from_ref(&work_entry)
    );
    assert!(travel_provider.load_all_entries().await.unwrap().is_empty());
    drop((personal_provider, work_provider, travel_provider));

    let backup_path = temporary.path().join("work-backup");
    let profile = work.settings_path.parent().unwrap();
    let command = Cli::parse_from([
        "mjournal",
        "--config",
        profile.to_str().unwrap(),
        "backup",
        "create",
        backup_path.to_str().unwrap(),
    ]);
    assert!(command.execute_backup().await.is_err());
    assert!(!backup_path.exists());
    assert!(store.archive(&work).is_err());
    drop(work_lease);
    assert!(command.execute_backup().await.unwrap());

    let restored = temporary.path().join("restored-work");
    backup::restore(&backup_path, &restored).unwrap();
    let restored_settings = Settings::new(Some(restored.join("config"))).await.unwrap();
    assert_eq!(read_one(open(restored_settings).await).await, work_entry);
    assert_eq!(read_one(open(personal.clone()).await).await, original);
    assert!(
        open(travel_settings.clone())
            .await
            .load_all_entries()
            .await
            .unwrap()
            .is_empty()
    );

    let archived = store.archive(&work).unwrap();
    assert!(!work.settings_path.exists());
    assert!(archived.join("config.toml").is_file());
    assert!(
        store
            .list()
            .unwrap()
            .iter()
            .all(|journal| journal.id != work.id)
    );
    assert_eq!(read_one(open(personal).await).await, original);
    fs::rename(&archived, profile).unwrap();
    let reopened = JournalStore::new(config.clone()).unwrap();
    assert_eq!(
        read_one(open(reopened.load_settings(&work).unwrap()).await).await,
        work_entry
    );
    assert_eq!(fs::read_to_string(config).unwrap(), original_config);
}

#[cfg(feature = "json")]
#[tokio::test]
async fn json_journals_keep_notes_backups_and_recovered_archives_separate() {
    separate_profiles(BackendType::Json, |settings| async move {
        backend::JsonDataProvide::new(settings.json_backend.file_path.unwrap())
    })
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_journals_keep_notes_backups_and_recovered_archives_separate() {
    separate_profiles(BackendType::Sqlite, |settings| async move {
        backend::SqliteDataProvide::from_file(settings.sqlite_backend.file_path.unwrap())
            .await
            .unwrap()
    })
    .await;
}

#[cfg(feature = "vjournal")]
#[tokio::test]
async fn vjournal_journals_keep_notes_backups_and_recovered_archives_separate() {
    separate_profiles(BackendType::Vjournal, |settings| async move {
        backend::VjournalDataProvide::new(settings.vjournal_backend.directory.unwrap())
    })
    .await;
}

#[cfg(feature = "file")]
#[tokio::test]
async fn file_journals_keep_notes_backups_and_recovered_archives_separate() {
    separate_profiles(BackendType::File, |settings| async move {
        backend::FileDataProvider::new(
            settings.file_backend.directory.unwrap(),
            settings.file_backend.format,
        )
    })
    .await;
}

#[tokio::test]
async fn resetting_managed_storage_defaults_keeps_backend_and_state_paths_private() {
    let temporary = tempfile::tempdir().unwrap();
    let config = temporary.path().join("config.toml");
    let personal = personal_settings(temporary.path(), BackendType::default());
    fs::write(&config, toml::to_string(&personal).unwrap()).unwrap();
    let store = JournalStore::new(config).unwrap();
    let journal = store.create("Work", &personal).unwrap();
    let before = Settings::new(Some(journal.settings_path.clone()))
        .await
        .unwrap();
    let mut document = ConfigDocument::load(journal.settings_path.clone()).unwrap();
    for keys in [
        vec!["json_backend", "file_path"],
        vec!["sqlite_backend", "file_path"],
        vec!["vjournal_backend", "directory"],
        vec!["file_backend", "directory"],
        vec!["app_state_dir"],
        vec!["export", "default_path"],
    ] {
        document.set(&keys, None).unwrap();
    }
    document.save().unwrap();
    let after = Settings::new(Some(journal.settings_path.clone()))
        .await
        .unwrap();
    assert_eq!(
        toml::to_string(&after).unwrap(),
        toml::to_string(&before).unwrap()
    );
    let defaults = Settings::from_config_text("", &journal.settings_path).unwrap();
    assert_eq!(defaults.app_state_dir, before.app_state_dir);
    assert_ne!(defaults.app_state_dir, personal.app_state_dir);
    #[cfg(feature = "json")]
    assert_eq!(
        defaults.json_backend.file_path,
        before.json_backend.file_path
    );
    #[cfg(feature = "sqlite")]
    assert_eq!(
        defaults.sqlite_backend.file_path,
        before.sqlite_backend.file_path
    );
    #[cfg(feature = "vjournal")]
    assert_eq!(
        defaults.vjournal_backend.directory,
        before.vjournal_backend.directory
    );
    #[cfg(feature = "file")]
    assert_eq!(
        defaults.file_backend.directory,
        before.file_backend.directory
    );

    let mut personal_state = AppState::default();
    personal_state.set_entries_list_percentage(20);
    personal_state.save(&personal).unwrap();
    let mut work_state = AppState::default();
    work_state.set_entries_list_percentage(55);
    work_state.save(&after).unwrap();
    assert_eq!(
        AppState::load(&personal).unwrap().entries_list_percentage(),
        20
    );
    assert_eq!(
        AppState::load(&after).unwrap().entries_list_percentage(),
        55
    );
}

#[test]
fn managed_config_file_and_directory_use_the_same_collection_themes() {
    let temporary = tempfile::tempdir().unwrap();
    let config = temporary.path().join("config.toml");
    fs::write(&config, "").unwrap();
    let themes = temporary.path().join("custom-collection-themes");
    let store = JournalStore::new(config)
        .unwrap()
        .with_themes_path(themes.clone())
        .unwrap();
    let journal = store.create("Work", &Settings::default()).unwrap();
    let file_store = ThemeStore::new(Some(&journal.settings_path)).unwrap();
    let profile = journal.settings_path.parent().unwrap().to_path_buf();
    let directory_store = ThemeStore::new(Some(&profile)).unwrap();
    assert_eq!(file_store.directory(), themes);
    assert_eq!(directory_store.directory(), themes);
    file_store.initialize().unwrap();
    assert!(themes.join("theme_tui.toml").is_file());
    assert!(!profile.join("themes").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn managed_config_alias_preserves_reset_defaults_themes_and_backup_leases() {
    let temporary = tempfile::tempdir().unwrap();
    let config = temporary.path().join("config.toml");
    let personal = personal_settings(temporary.path(), BackendType::default());
    fs::write(&config, toml::to_string(&personal).unwrap()).unwrap();
    let themes = temporary.path().join("collection-themes");
    let store = JournalStore::new(config)
        .unwrap()
        .with_themes_path(themes.clone())
        .unwrap();
    let journal = store.create("Work", &personal).unwrap();
    let expected = store.load_settings(&journal).unwrap();
    let aliases = [
        temporary.path().join("work.toml"),
        temporary.path().join("alias/config.toml"),
    ];
    fs::create_dir(aliases[1].parent().unwrap()).unwrap();
    for alias in &aliases {
        std::os::unix::fs::symlink(&journal.settings_path, alias).unwrap();
    }

    let mut document = ConfigDocument::load(aliases[0].clone()).unwrap();
    for keys in [
        vec!["json_backend", "file_path"],
        vec!["sqlite_backend", "file_path"],
        vec!["vjournal_backend", "directory"],
        vec!["file_backend", "directory"],
        vec!["app_state_dir"],
        vec!["export", "default_path"],
    ] {
        document.set(&keys, None).unwrap();
    }
    let preview = Settings::from_config_text(&document.text(), document.path()).unwrap();
    assert_eq!(
        toml::to_string(&preview).unwrap(),
        toml::to_string(&expected).unwrap()
    );
    document.save().unwrap();
    let _lease = store.acquire(&journal).unwrap();
    let backup_path = temporary.path().join("backup");
    for alias in &aliases {
        assert!(fs::symlink_metadata(alias).unwrap().is_symlink());
        let loaded = Settings::new(Some(alias.clone())).await.unwrap();
        assert_eq!(
            toml::to_string(&loaded).unwrap(),
            toml::to_string(&expected).unwrap()
        );
        assert_eq!(ThemeStore::new(Some(alias)).unwrap().directory(), themes);
        let command = Cli::parse_from([
            "mjournal",
            "--config",
            alias.to_str().unwrap(),
            "backup",
            "create",
            backup_path.to_str().unwrap(),
        ]);
        let error = command.execute_backup().await.unwrap_err();
        assert!(error.to_string().contains("open in another window"));
        assert!(!backup_path.exists());
    }
}
