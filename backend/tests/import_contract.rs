use backend::{DataProvider, EntriesDTO, EntryDraft};
use chrono::Utc;

async fn rejects_an_invalid_batch_before_committing_its_prefix(mut provider: impl DataProvider) {
    let existing = EntryDraft::new(Utc::now(), "existing".into(), vec![], None)
        .with_content("keep existing content".into());
    provider.add_entry(existing).await.unwrap();
    let before = provider.load_all_entries().await.unwrap();
    let valid = EntryDraft::new(Utc::now(), "imported".into(), vec![], None);
    let invalid = EntryDraft::new(Utc::now(), String::new(), vec![], None);
    assert!(
        provider
            .import_entries(EntriesDTO::new(vec![valid.clone(), invalid]))
            .await
            .is_err()
    );
    assert_eq!(provider.load_all_entries().await.unwrap(), before);
    provider
        .import_entries(EntriesDTO::new(vec![valid]))
        .await
        .unwrap();
    let after = provider.load_all_entries().await.unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(
        after
            .iter()
            .filter(|entry| entry.title == "imported")
            .count(),
        1
    );
}

#[cfg(feature = "json")]
#[tokio::test]
async fn json_batch_validation_preserves_existing_store() {
    let dir = tempfile::tempdir().unwrap();
    rejects_an_invalid_batch_before_committing_its_prefix(backend::JsonDataProvide::new(
        dir.path().join("entries.json"),
    ))
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_batch_validation_preserves_existing_store() {
    rejects_an_invalid_batch_before_committing_its_prefix(
        backend::SqliteDataProvide::create("sqlite::memory:")
            .await
            .unwrap(),
    )
    .await;
}

#[cfg(feature = "file")]
#[tokio::test]
async fn file_batch_validation_preserves_existing_store() {
    let dir = tempfile::tempdir().unwrap();
    rejects_an_invalid_batch_before_committing_its_prefix(backend::FileDataProvider::new(
        dir.path().to_owned(),
        backend::FileFormat::Plain,
    ))
    .await;
}

#[cfg(feature = "vjournal")]
#[tokio::test]
async fn vjournal_batch_validation_preserves_existing_store() {
    let dir = tempfile::tempdir().unwrap();
    rejects_an_invalid_batch_before_committing_its_prefix(backend::VjournalDataProvide::new(
        dir.path().to_owned(),
    ))
    .await;
}

#[cfg(feature = "json")]
#[tokio::test]
async fn json_id_exhaustion_does_not_commit_a_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("entries.json");
    let mut provider = backend::JsonDataProvide::new(path.clone());
    let draft = EntryDraft::new(Utc::now(), "existing".into(), vec![], None);
    provider
        .restore_entry(backend::Entry::from_draft(u32::MAX - 1, draft.clone()))
        .await
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(
        provider
            .import_entries(EntriesDTO::new(vec![draft.clone(), draft]))
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[cfg(feature = "file")]
#[tokio::test]
async fn file_serialization_failure_cleans_staging_without_publishing_a_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider =
        backend::FileDataProvider::new(dir.path().to_owned(), backend::FileFormat::Plain);
    provider.load_all_entries().await.unwrap();
    let valid = EntryDraft::new(Utc::now(), "valid".into(), vec![], None);
    let invalid = EntryDraft::new(Utc::now(), "x".repeat(1024 * 1024), vec![], None);
    assert!(
        provider
            .import_entries(EntriesDTO::new(vec![valid.clone(), invalid]))
            .await
            .is_err()
    );
    assert!(provider.load_all_entries().await.unwrap().is_empty());
    assert!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .all(|entry| entry.unwrap().file_name() == ".tui-journal.lock")
    );
    provider
        .import_entries(EntriesDTO::new(vec![valid.clone(), valid]))
        .await
        .unwrap();
    assert_eq!(provider.load_all_entries().await.unwrap().len(), 2);
}

#[cfg(feature = "vjournal")]
#[tokio::test]
async fn vjournal_priority_failure_cleans_staging_without_publishing_a_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = backend::VjournalDataProvide::new(dir.path().to_owned());
    let valid = EntryDraft::new(Utc::now(), "valid".into(), vec![], None);
    let invalid = EntryDraft::new(Utc::now(), "invalid".into(), vec![], Some(10));
    assert!(
        provider
            .import_entries(EntriesDTO::new(vec![valid.clone(), invalid]))
            .await
            .is_err()
    );
    assert!(provider.load_all_entries().await.unwrap().is_empty());
    assert!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .all(|entry| entry.unwrap().file_name() == ".tui-journal.lock")
    );
    provider
        .import_entries(EntriesDTO::new(vec![valid.clone(), valid]))
        .await
        .unwrap();
    assert_eq!(provider.load_all_entries().await.unwrap().len(), 2);
}
