use backend::*;
use chrono::{TimeZone, Utc};
use tempfile::{Builder, NamedTempFile};

async fn create_provide_with_two_entries(temp_file: &NamedTempFile) -> JsonDataProvide {
    let mut json_provide = JsonDataProvide::new(temp_file.path().to_path_buf());
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

    json_provide.add_entry(entry_draft_1).await.unwrap();
    json_provide.add_entry(entry_draft_2).await.unwrap();

    json_provide
}

#[tokio::test]
async fn create_provider_with_default_entries() {
    let temp_file = Builder::new()
        .prefix("json_create_default")
        .tempfile()
        .unwrap();
    let mut provider = create_provide_with_two_entries(&temp_file).await;

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, 0);
    assert_eq!(entries[1].id, 1);
    assert_eq!(entries[0].title, String::from("Title 1"));
    assert_eq!(entries[1].title, String::from("Title 2"));
    assert_eq!(entries[0].priority, None);
    assert_eq!(entries[1].priority, Some(1));
}

#[tokio::test]
async fn add_entry() {
    let temp_file = Builder::new().prefix("json_add_entry").tempfile().unwrap();
    let mut provider = create_provide_with_two_entries(&temp_file).await;

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
    assert_eq!(entries[2].id, 2);
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
    let temp_file = Builder::new()
        .prefix("json_remove_entry")
        .tempfile()
        .unwrap();
    let mut provider = create_provide_with_two_entries(&temp_file).await;

    provider.remove_entry(1).await.unwrap();

    let entries = provider.load_all_entries().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, 0);
}

#[tokio::test]
async fn restore_entry_preserves_id() {
    let temp_file = Builder::new()
        .prefix("json_restore_entry")
        .tempfile()
        .unwrap();
    let mut provider = create_provide_with_two_entries(&temp_file).await;

    let restored_entry = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.id == 0)
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
    let temp_file = Builder::new()
        .prefix("json_update_entry")
        .tempfile()
        .unwrap();
    let mut provider = create_provide_with_two_entries(&temp_file).await;

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
    let temp_file_source = Builder::new()
        .prefix("json_export_source")
        .tempfile()
        .unwrap();
    let mut provider_source = create_provide_with_two_entries(&temp_file_source).await;

    let created_ids = [0, 1];

    let dto_source = provider_source
        .get_export_object(&created_ids)
        .await
        .unwrap();

    assert_eq!(dto_source.entries.len(), created_ids.len());

    let temp_file_dist = Builder::new()
        .prefix("json_export_dist")
        .tempfile()
        .unwrap();
    let mut provider_dist = JsonDataProvide::new(temp_file_dist.path().to_path_buf());

    provider_dist
        .import_entries(dto_source.clone())
        .await
        .unwrap();

    let dto_dist = provider_dist.get_export_object(&created_ids).await.unwrap();

    assert_eq!(dto_source, dto_dist);
}

#[tokio::test]
async fn assign_priority() {
    let temp_file = Builder::new()
        .prefix("json_assign_priority")
        .tempfile()
        .unwrap();
    let mut provider = create_provide_with_two_entries(&temp_file).await;

    provider.assign_priority_to_entries(3).await.unwrap();

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries[0].priority, Some(3));
    assert_eq!(entries[1].priority, Some(1));
}

fn protected_entry() -> Entry {
    static ENTRY: std::sync::LazyLock<Entry> = std::sync::LazyLock::new(|| {
        let entry = Entry::new(
            0,
            Utc.with_ymd_and_hms(2026, 10, 24, 1, 2, 3).unwrap(),
            "Public title".into(),
            "PRIVATE_NOTE_BODY_json".into(),
            vec!["PRIVATE_NOTE_TAG_json".into()],
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
async fn protected_entries_use_versioned_store_and_export_ciphertext() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.json");
    let mut provider = JsonDataProvide::new(path.clone());
    let expected = provider
        .add_entry(EntryDraft::from_entry(protected_entry()))
        .await
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(serde_json::from_slice::<Vec<Entry>>(&bytes).is_err());
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("PRIVATE_NOTE_BODY_json"));
    assert!(!text.contains("PRIVATE_NOTE_TAG_json"));
    assert!(text.contains("Public title"));
    assert!(text.contains("AGE ENCRYPTED FILE"));
    provider.assign_priority_to_entries(3).await.unwrap();
    assert_eq!(
        provider.load_all_entries().await.unwrap(),
        vec![expected.clone()]
    );
    let exported = provider.get_export_object(&[expected.id]).await.unwrap();
    assert_eq!(
        exported.entries,
        vec![EntryDraft::from_entry(expected.clone())]
    );
    let mut destination = JsonDataProvide::new(directory.path().join("import.json"));
    destination.import_entries(exported).await.unwrap();
    assert_eq!(
        destination.load_all_entries().await.unwrap(),
        vec![expected]
    );
}

#[tokio::test]
async fn protected_store_never_reverts_to_legacy_after_removal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.json");
    let mut provider = JsonDataProvide::new(path.clone());
    let entry = provider
        .add_entry(EntryDraft::from_entry(protected_entry()))
        .await
        .unwrap();
    provider.remove_entry(entry.id).await.unwrap();
    drop(provider);
    let mut reopened = JsonDataProvide::new(path.clone());
    reopened
        .add_entry(EntryDraft::new(
            Utc::now(),
            "Plain note".into(),
            vec![],
            None,
        ))
        .await
        .unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert!(serde_json::from_slice::<Vec<Entry>>(&bytes).is_err());
    let store: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(store["version"], 2);
}

#[tokio::test]
async fn rejects_legacy_array_containing_protection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.json");
    let bytes = serde_json::to_vec(&vec![protected_entry()]).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let mut provider = JsonDataProvide::new(path.clone());
    assert!(provider.load_all_entries().await.is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[tokio::test]
async fn invalid_protected_update_preserves_original_store() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.json");
    let mut provider = JsonDataProvide::new(path.clone());
    let mut entry = provider
        .add_entry(EntryDraft::from_entry(protected_entry()))
        .await
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    entry.content = "PRIVATE_ACCIDENTAL_PLAINTEXT".into();
    assert!(provider.update_entry(entry).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let files: Vec<_> = std::fs::read_dir(directory.path()).unwrap().collect();
    assert_eq!(files.len(), 2);
}

#[tokio::test]
async fn external_changes_cannot_be_overwritten_by_stale_update_or_delete() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.json");
    let mut provider = JsonDataProvide::new(path.clone());
    let entry = provider
        .add_entry(EntryDraft::new(Utc::now(), "Original".into(), vec![], None))
        .await
        .unwrap();
    let mut external = entry.clone();
    external.content = "Externally changed".into();
    let replacement = serde_json::to_vec(&vec![external.clone()]).unwrap();
    std::fs::write(&path, &replacement).unwrap();
    assert!(provider.update_entry(entry.clone()).await.is_err());
    assert!(provider.remove_entry(entry.id).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), replacement);
    assert_eq!(provider.load_all_entries().await.unwrap(), vec![external]);
    provider.remove_entry(entry.id).await.unwrap();
}

#[tokio::test]
async fn second_provider_cannot_write_until_first_closes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.json");
    let mut first = JsonDataProvide::new(path.clone());
    first.load_all_entries().await.unwrap();
    let mut second = JsonDataProvide::new(path);
    assert!(
        second
            .add_entry(EntryDraft::new(Utc::now(), "Blocked".into(), vec![], None))
            .await
            .is_err()
    );
    drop(first);
    second
        .add_entry(EntryDraft::new(Utc::now(), "Allowed".into(), vec![], None))
        .await
        .unwrap();
}

#[tokio::test]
async fn unsupported_version_and_duplicate_ids_fail_without_rewriting() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("entries.json");
    let entry = Entry::from_draft(0, EntryDraft::new(Utc::now(), "Entry".into(), vec![], None));
    for bytes in [
        serde_json::to_vec(&serde_json::json!({"format":"tui-journal","version":999,"entries":[]}))
            .unwrap(),
        serde_json::to_vec(&vec![entry.clone(), entry]).unwrap(),
    ] {
        std::fs::write(&path, &bytes).unwrap();
        let mut provider = JsonDataProvide::new(path.clone());
        assert!(provider.load_all_entries().await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn replaced_lock_and_directory_are_rejected_before_writing() {
    let directory = tempfile::tempdir().unwrap();
    let store = directory.path().join("store");
    std::fs::create_dir(&store).unwrap();
    let path = store.join("entries.json");
    let mut provider = JsonDataProvide::new(path.clone());
    let entry = provider
        .add_entry(EntryDraft::new(Utc::now(), "Original".into(), vec![], None))
        .await
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    std::fs::rename(store.join("entries.json.lock"), store.join("previous.lock")).unwrap();
    std::fs::write(store.join("entries.json.lock"), b"").unwrap();
    assert!(provider.update_entry(entry.clone()).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(provider);
    let mut provider = JsonDataProvide::new(path.clone());
    provider.load_all_entries().await.unwrap();
    std::fs::rename(&store, directory.path().join("original")).unwrap();
    std::fs::create_dir(&store).unwrap();
    std::fs::write(&path, &before).unwrap();
    std::fs::write(store.join("entries.json.lock"), b"").unwrap();
    assert!(provider.update_entry(entry).await.is_err());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_store_is_not_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.json");
    let link = directory.path().join("entries.json");
    std::fs::write(&target, b"[]").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let mut provider = JsonDataProvide::new(link.clone());
    assert!(
        provider
            .add_entry(EntryDraft::new(Utc::now(), "Entry".into(), vec![], None))
            .await
            .is_err()
    );
    assert!(
        std::fs::symlink_metadata(link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(target).unwrap(), b"[]");
}
