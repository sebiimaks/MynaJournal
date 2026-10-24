use backend::*;
use chrono::{TimeZone, Utc};

async fn create_provider_with_two_entries() -> SqliteDataProvide {
    let mut provider = create_provider().await;

    let mut entry_draft_1 = EntryDraft::new(
        Utc::now(),
        String::from("Title 1"),
        vec![String::from("Tag_1"), String::from("Tag_2")],
        None,
    );
    entry_draft_1.content.push_str("Content entry 1");
    let mut entry_draft_2 = EntryDraft::new(
        Utc.with_ymd_and_hms(2023, 3, 23, 1, 1, 1).unwrap(),
        String::from("Title 2"),
        Vec::new(),
        Some(1),
    );
    entry_draft_2.content.push_str("Content entry 2");

    provider.add_entry(entry_draft_1).await.unwrap();
    provider.add_entry(entry_draft_2).await.unwrap();

    provider
}

#[inline]
async fn create_provider() -> SqliteDataProvide {
    SqliteDataProvide::create("sqlite::memory:").await.unwrap()
}

#[tokio::test]
async fn create_provider_with_default_entries() {
    let mut provider = create_provider_with_two_entries().await;

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, 1);
    assert_eq!(entries[1].id, 2);
    assert_eq!(entries[0].title, String::from("Title 1"));
    assert_eq!(entries[1].title, String::from("Title 2"));
    assert_eq!(entries[0].priority, None);
    assert_eq!(entries[1].priority, Some(1));
}

#[tokio::test]
async fn add_entry() {
    let mut provider = create_provider_with_two_entries().await;

    let mut entry_draft = EntryDraft::new(
        Utc.with_ymd_and_hms(2023, 3, 23, 1, 1, 1).unwrap(),
        String::from("Title added"),
        vec![String::from("Tag_1"), String::from("Tag_3")],
        Some(1),
    );
    entry_draft.content.push_str("Content entry added");

    provider.add_entry(entry_draft).await.unwrap();

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[2].id, 3);
    assert_eq!(entries[2].title, String::from("Title added"));
    assert_eq!(entries[2].content, String::from("Content entry added"));
    assert_eq!(entries[2].priority, Some(1));
    assert_eq!(
        entries[2].tags,
        vec![String::from("Tag_1"), String::from("Tag_3")]
    );
}

#[tokio::test]
async fn remove_entry() {
    let mut provider = create_provider_with_two_entries().await;

    provider.remove_entry(1).await.unwrap();

    let entries = provider.load_all_entries().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, 2);
}

#[tokio::test]
async fn restore_entry_preserves_id() {
    let mut provider = create_provider_with_two_entries().await;
    let restored_entry = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.id == 1)
        .unwrap();

    provider.remove_entry(restored_entry.id).await.unwrap();
    let added = provider
        .add_entry(EntryDraft::new(
            Utc.with_ymd_and_hms(2024, 5, 6, 7, 8, 9).unwrap(),
            String::from("Added"),
            vec![],
            None,
        ))
        .await
        .unwrap();
    assert_ne!(added.id, restored_entry.id);

    let restored = provider
        .restore_entry(restored_entry.clone())
        .await
        .unwrap();

    assert_eq!(restored, restored_entry);

    let entries = provider.load_all_entries().await.unwrap();
    assert!(entries.iter().any(|entry| entry == &restored_entry));
}

#[tokio::test]
async fn update_entry() {
    let mut provider = create_provider_with_two_entries().await;

    let mut entries = provider.load_all_entries().await.unwrap();

    entries[0].content = String::from("Updated Content");
    entries[0].tags.pop().unwrap();
    entries[0].priority = Some(2);
    entries[1].title = String::from("Updated Title");
    entries[1].tags.push(String::from("Tag_4"));
    entries[1].priority = None;

    provider.update_entry(entries.pop().unwrap()).await.unwrap();
    provider.update_entry(entries.pop().unwrap()).await.unwrap();

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].content, String::from("Updated Content"));
    assert_eq!(entries[0].tags.len(), 1);
    assert_eq!(entries[0].priority, Some(2));
    assert_eq!(entries[1].title, String::from("Updated Title"));
    assert!(entries[1].tags.contains(&String::from("Tag_4")));
    assert_eq!(entries[1].priority, None);
}

#[tokio::test]
async fn export_import() {
    let mut provider_source = create_provider_with_two_entries().await;

    let created_ids = [1, 2];

    let dto_source = provider_source
        .get_export_object(&created_ids)
        .await
        .unwrap();

    assert_eq!(dto_source.entries.len(), created_ids.len());

    let mut provider_dist = create_provider().await;

    provider_dist
        .import_entries(dto_source.clone())
        .await
        .unwrap();

    let dto_dist = provider_dist.get_export_object(&created_ids).await.unwrap();

    assert_eq!(dto_source, dto_dist);
}

#[tokio::test]
async fn assign_priority() {
    let mut provider = create_provider_with_two_entries().await;

    provider.assign_priority_to_entries(3).await.unwrap();

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries[0].priority, Some(3));
    assert_eq!(entries[1].priority, Some(1));
}

fn protected_entry() -> Entry {
    static ENTRY: std::sync::LazyLock<Entry> = std::sync::LazyLock::new(|| {
        let entry = Entry::new(
            1,
            Utc.with_ymd_and_hms(2026, 10, 24, 1, 2, 3).unwrap(),
            "Public title".into(),
            "PRIVATE_NOTE_BODY_sqlite".into(),
            vec!["PRIVATE_NOTE_TAG_sqlite".into()],
            Some(17),
        );
        let envelope = backend::protection::protect(
            &entry,
            &backend::protection::SecretString::from("test passphrase".to_owned()),
            None,
        )
        .unwrap();
        entry.with_protection(envelope)
    });
    ENTRY.clone()
}

#[tokio::test]
async fn protection_removes_plaintext_and_preserves_ciphertext_on_export() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.sqlite");
    let mut provider = SqliteDataProvide::from_file(path.clone()).await.unwrap();
    let mut original = protected_entry();
    original.protection = None;
    original.content = "PRIVATE_NOTE_BODY_sqlite".into();
    original.tags = vec!["PRIVATE_NOTE_TAG_sqlite".into()];
    original.priority = Some(17);
    let plain = provider
        .add_entry(EntryDraft::from_entry(original))
        .await
        .unwrap();
    let mut protected = protected_entry();
    protected.id = plain.id;
    provider.update_entry(protected.clone()).await.unwrap();
    provider.assign_priority_to_entries(3).await.unwrap();
    assert_eq!(
        provider.load_all_entries().await.unwrap(),
        vec![protected.clone()]
    );
    let export = provider.get_export_object(&[protected.id]).await.unwrap();
    assert_eq!(
        export.entries,
        vec![EntryDraft::from_entry(protected.clone())]
    );
    let mut destination = create_provider().await;
    destination.import_entries(export).await.unwrap();
    assert_eq!(
        destination.load_all_entries().await.unwrap(),
        vec![protected]
    );
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes
            .windows(b"PRIVATE_NOTE_BODY_sqlite".len())
            .any(|part| part == b"PRIVATE_NOTE_BODY_sqlite")
    );
    assert!(
        !bytes
            .windows(b"PRIVATE_NOTE_TAG_sqlite".len())
            .any(|part| part == b"PRIVATE_NOTE_TAG_sqlite")
    );
    assert!(!path.with_extension("sqlite-journal").exists());
    assert!(!path.with_extension("sqlite-wal").exists());
}

#[tokio::test]
async fn failed_tag_insert_rolls_back_add_restore_and_update() {
    let mut provider = create_provider().await;
    let valid = provider
        .add_entry(
            EntryDraft::new(Utc::now(), "Original".into(), vec!["tag".into()], None)
                .with_content("Original content".into()),
        )
        .await
        .unwrap();
    let mut invalid = valid.clone();
    invalid.content = "Must not survive failed update".into();
    invalid.tags = vec!["duplicate".into(), "duplicate".into()];
    assert!(provider.update_entry(invalid.clone()).await.is_err());
    assert_eq!(
        provider.load_all_entries().await.unwrap(),
        vec![valid.clone()]
    );
    invalid.id = valid.id + 5;
    assert!(provider.restore_entry(invalid.clone()).await.is_err());
    assert!(
        provider
            .add_entry(EntryDraft::from_entry(invalid))
            .await
            .is_err()
    );
    assert_eq!(provider.load_all_entries().await.unwrap(), vec![valid]);
}

#[tokio::test]
async fn private_fields_cannot_be_written_beside_ciphertext() {
    let mut provider = create_provider().await;
    let entry = provider
        .add_entry(EntryDraft::from_entry(protected_entry()))
        .await
        .unwrap();
    for field in 0..3 {
        let mut invalid = entry.clone();
        match field {
            0 => invalid.content = "PRIVATE_ACCIDENTAL_PLAINTEXT".into(),
            1 => invalid.tags = vec!["PRIVATE_ACCIDENTAL_TAG".into()],
            _ => invalid.priority = Some(7),
        }
        assert!(provider.update_entry(invalid).await.is_err());
        assert_eq!(
            provider.load_all_entries().await.unwrap(),
            vec![entry.clone()]
        );
    }
}

#[tokio::test]
async fn concurrent_changes_cannot_be_overwritten_by_stale_update_or_delete() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.sqlite");
    let mut first = SqliteDataProvide::from_file(path.clone()).await.unwrap();
    let entry = first
        .add_entry(EntryDraft::new(Utc::now(), "Original".into(), vec![], None))
        .await
        .unwrap();
    let mut second = SqliteDataProvide::from_file(path).await.unwrap();
    let mut updated = second.load_all_entries().await.unwrap().remove(0);
    updated.content = "Other app changed this".into();
    second.update_entry(updated.clone()).await.unwrap();
    assert!(first.update_entry(entry.clone()).await.is_err());
    assert!(first.remove_entry(entry.id).await.is_err());
    assert_eq!(first.load_all_entries().await.unwrap(), vec![updated]);
    first.remove_entry(entry.id).await.unwrap();
}

#[tokio::test]
async fn migration_prevents_old_clients_opening_protected_database() {
    use sqlx::migrate::{MigrateError, Migration, MigrationType, Migrator};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.sqlite");
    let _provider = SqliteDataProvide::from_file(path.clone()).await.unwrap();
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let migrator = Migrator {
        migrations: vec![
            Migration::new(
                20230509041613,
                "init entries".into(),
                MigrationType::Simple,
                include_str!("../../src/sqlite/migrations/20230509041613_init_entries.sql").into(),
                false,
            ),
            Migration::new(
                20230613041405,
                "add tags".into(),
                MigrationType::Simple,
                include_str!("../../src/sqlite/migrations/20230613041405_add_tags.sql").into(),
                false,
            ),
            Migration::new(
                20240104160740,
                "entries priorities".into(),
                MigrationType::Simple,
                include_str!("../../src/sqlite/migrations/20240104160740_entries_priorities.sql")
                    .into(),
                false,
            ),
        ]
        .into(),
        ..Migrator::DEFAULT
    };
    assert!(matches!(
        migrator.run(&pool).await,
        Err(MigrateError::VersionMissing(20261024000000))
    ));
}

#[tokio::test]
async fn comma_in_tag_is_preserved_across_storage() {
    let mut provider = create_provider().await;
    let entry = provider
        .add_entry(EntryDraft::new(
            Utc::now(),
            "Title".into(),
            vec!["single,tag".into()],
            None,
        ))
        .await
        .unwrap();
    assert_eq!(provider.load_all_entries().await.unwrap(), vec![entry]);
}

#[tokio::test]
async fn tag_storage_order_does_not_trigger_a_false_conflict() {
    let mut provider = create_provider().await;
    let mut entry = provider
        .add_entry(EntryDraft::new(
            Utc::now(),
            "Title".into(),
            vec!["zebra".into(), "apple".into()],
            None,
        ))
        .await
        .unwrap();
    entry.content = "Changed".into();
    provider.update_entry(entry.clone()).await.unwrap();
    provider.remove_entry(entry.id).await.unwrap();
    assert!(provider.load_all_entries().await.unwrap().is_empty());
}

#[tokio::test]
async fn assigning_priority_does_not_accept_unrelated_external_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.sqlite");
    let mut first = SqliteDataProvide::from_file(path.clone()).await.unwrap();
    let entry = first
        .add_entry(EntryDraft::new(
            Utc::now(),
            "Original".into(),
            vec![],
            Some(2),
        ))
        .await
        .unwrap();
    let mut second = SqliteDataProvide::from_file(path).await.unwrap();
    let mut changed = second.load_all_entries().await.unwrap().remove(0);
    changed.content = "External edit".into();
    second.update_entry(changed.clone()).await.unwrap();
    first.assign_priority_to_entries(1).await.unwrap();
    assert!(first.update_entry(entry).await.is_err());
    assert_eq!(first.load_all_entries().await.unwrap(), vec![changed]);
}
