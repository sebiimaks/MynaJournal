use backend::{DataProvider, EntriesDTO, Entry, EntryDraft, JournalPrompt, Mood};
use chrono::{TimeZone, Utc};

fn draft() -> EntryDraft {
    let mut draft = EntryDraft::new(
        Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap(),
        "A journal entry".into(),
        vec!["tag".into()],
        Some(3),
    )
    .with_content("Body".into());
    draft.mood = Some(Mood::Average);
    draft.journal_prompt = Some(JournalPrompt {
        id: "average_001".into(),
        mood: Mood::Average,
        topic: "A topic, with punctuation; 日本語".into(),
        text: "Prompt: a comma, semicolon; literal \\n and newline\n🦀".into(),
    });
    draft
}

async fn round_trip(mut provider: impl DataProvider) -> Vec<Entry> {
    let added = provider.add_entry(draft()).await.unwrap();
    assert_eq!(
        provider.load_all_entries().await.unwrap(),
        std::slice::from_ref(&added)
    );
    let mut changed = added;
    changed.mood = Some(Mood::High);
    changed.content = "Edited body".into();
    let changed = provider.update_entry(changed).await.unwrap();
    assert_eq!(changed.journal_prompt, draft().journal_prompt);
    let protected = changed.clone().with_protection(backend::EncryptedNote {
        version: 1,
        identity: age::x25519::Identity::generate().to_public().to_string(),
        ciphertext:
            "-----BEGIN AGE ENCRYPTED FILE-----\ndGVzdA==\n-----END AGE ENCRYPTED FILE-----\n"
                .into(),
    });
    assert!(protected.mood.is_none());
    assert!(protected.journal_prompt.is_none());
    provider.update_entry(protected.clone()).await.unwrap();
    for with_mood in [true, false] {
        let mut invalid = protected.clone();
        if with_mood {
            invalid.mood = Some(Mood::Low);
        } else {
            invalid.journal_prompt = draft().journal_prompt;
        }
        assert!(provider.update_entry(invalid).await.is_err());
        assert_eq!(
            provider.load_all_entries().await.unwrap(),
            std::slice::from_ref(&protected)
        );
    }
    provider.update_entry(changed.clone()).await.unwrap();
    let export = provider.get_export_object(&[changed.id]).await.unwrap();
    let encoded = serde_json::to_string(&export).unwrap();
    let decoded: EntriesDTO = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.entries, [EntryDraft::from_entry(changed.clone())]);
    provider.remove_entry(changed.id).await.unwrap();
    provider.restore_entry(changed.clone()).await.unwrap();
    assert_eq!(
        provider.load_all_entries().await.unwrap(),
        std::slice::from_ref(&changed)
    );
    provider.import_entries(decoded).await.unwrap();
    let entries = provider.load_all_entries().await.unwrap();
    assert_eq!(entries.len(), 2);
    for entry in &entries {
        assert_eq!(entry.mood, Some(Mood::High));
        assert_eq!(entry.journal_prompt, changed.journal_prompt);
    }
    entries
}

#[cfg(feature = "json")]
#[tokio::test]
async fn json_persists_mood_and_prompt_and_upgrades_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("entries.json");
    let mut expected = round_trip(backend::JsonDataProvide::new(path.clone())).await;
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["version"], 3);
    let mut reopened = backend::JsonDataProvide::new(path)
        .load_all_entries()
        .await
        .unwrap();
    expected.sort_by_key(|e| e.id);
    reopened.sort_by_key(|e| e.id);
    assert_eq!(reopened, expected);
}

#[cfg(feature = "json")]
#[tokio::test]
async fn json_reads_previous_stores_and_refuses_mood_in_older_formats() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("entries.json");
    let old = Entry::new(
        1,
        Utc::now(),
        "Existing".into(),
        "Body".into(),
        vec![],
        None,
    );
    std::fs::write(
        &path,
        serde_json::to_vec(
            &serde_json::json!({"format": "tui-journal", "version": 2, "entries": [old]}),
        )
        .unwrap(),
    )
    .unwrap();
    {
        let mut provider = backend::JsonDataProvide::new(path.clone());
        let mut entry = provider.load_all_entries().await.unwrap().remove(0);
        assert_eq!(entry, old);
        entry.mood = Some(Mood::High);
        provider.update_entry(entry).await.unwrap();
    }
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["version"], 3);
    stored["version"] = 2.into();
    for invalid in [stored.clone(), stored["entries"].clone()] {
        std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(
            backend::JsonDataProvide::new(path.clone())
                .load_all_entries()
                .await
                .is_err()
        );
    }
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_persists_mood_and_prompt_after_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("entries.db");
    let mut expected = round_trip(
        backend::SqliteDataProvide::from_file(path.clone())
            .await
            .unwrap(),
    )
    .await;
    let mut reopened = backend::SqliteDataProvide::from_file(path)
        .await
        .unwrap()
        .load_all_entries()
        .await
        .unwrap();
    expected.sort_by_key(|e| e.id);
    reopened.sort_by_key(|e| e.id);
    assert_eq!(reopened, expected);
}

#[cfg(feature = "file")]
#[tokio::test]
async fn plain_and_markdown_files_persist_mood_and_prompt() {
    for format in [backend::FileFormat::Plain, backend::FileFormat::Markdown] {
        let dir = tempfile::tempdir().unwrap();
        let expected = round_trip(backend::FileDataProvider::new(dir.path().into(), format)).await;
        let reopened = backend::FileDataProvider::new(dir.path().into(), format)
            .load_all_entries()
            .await
            .unwrap();
        assert_eq!(reopened.len(), expected.len());
        for entry in reopened {
            assert_eq!(entry.mood, Some(Mood::High));
            assert_eq!(entry.journal_prompt, draft().journal_prompt);
        }
    }
}

#[cfg(feature = "vjournal")]
#[tokio::test]
async fn vjournal_persists_mood_and_prompt_after_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let expected = round_trip(backend::VjournalDataProvide::new(dir.path().into())).await;
    let reopened = backend::VjournalDataProvide::new(dir.path().into())
        .load_all_entries()
        .await
        .unwrap();
    assert_eq!(reopened.len(), expected.len());
    for entry in reopened {
        assert_eq!(entry.mood, Some(Mood::High));
        assert_eq!(entry.journal_prompt, draft().journal_prompt);
    }
}

#[test]
fn transfer_preserves_original_prompt_and_rejects_downgraded_metadata() {
    let document = EntriesDTO::new(vec![draft()]);
    let value = serde_json::to_value(&document).unwrap();
    assert_eq!(
        serde_json::from_value::<EntriesDTO>(value.clone()).unwrap(),
        document
    );
    for version in [100, 101] {
        let mut old = value.clone();
        old["version"] = version.into();
        assert!(serde_json::from_value::<EntriesDTO>(old).is_err());
    }
    let mut invalid = value;
    invalid["entries"][0]["mood"] = "unknown".into();
    assert!(serde_json::from_value::<EntriesDTO>(invalid).is_err());
}

#[test]
fn older_entries_and_transfers_have_no_mood_or_prompt() {
    let plain = EntryDraft::new(Utc::now(), "Existing".into(), vec![], None);
    for version in [100, 101, 102] {
        let value = serde_json::json!({ "version": version, "entries": [plain] });
        let decoded: EntriesDTO = serde_json::from_value(value).unwrap();
        assert!(decoded.entries[0].mood.is_none());
        assert!(decoded.entries[0].journal_prompt.is_none());
    }
}
