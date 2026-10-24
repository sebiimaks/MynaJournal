use std::{
    fs,
    path::{Path, PathBuf},
};

use backend::{DataProvider, EntriesDTO, Entry, EntryDraft, FileDataProvider, FileFormat};
use chrono::{NaiveDateTime, TimeZone, Utc};
use tempfile::TempDir;

fn provider(path: &Path) -> FileDataProvider {
    FileDataProvider::new(path.to_owned(), FileFormat::Plain)
}

fn draft() -> EntryDraft {
    EntryDraft::new(
        Utc.with_ymd_and_hms(2023, 5, 6, 7, 8, 9).unwrap(),
        "A title: \"日本語\"".into(),
        vec!["walking".into(), "two words".into(), "日本語".into()],
        Some(2),
    )
    .with_content("\nA journal ✨\r\n---\nTrailing spaces  \n\n".into())
}

fn journal_paths(directory: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| !path.file_name().unwrap().to_string_lossy().starts_with('.'))
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn only_journal(directory: &Path) -> PathBuf {
    let paths = journal_paths(directory);
    assert_eq!(paths.len(), 1);
    paths[0].clone()
}

#[tokio::test]
async fn plain_files_round_trip_complete_entries_and_use_creation_time_names() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let entry = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let filename = path.file_name().unwrap().to_str().unwrap();
    let created_name = NaiveDateTime::parse_from_str(filename, "%Y%m%d_%H%M%S").unwrap();
    assert_eq!(filename.len(), 15);
    assert!(path.extension().is_none());
    assert_ne!(created_name.date(), entry.date.date_naive());
    assert_eq!(EntryDraft::from_entry(entry.clone()), draft());
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.starts_with("---\n"));
    assert!(contents.contains("tui_journal:"));
    assert!(contents.ends_with(&draft().content));
    drop(backend);
    let loaded = provider(directory.path()).load_all_entries().await.unwrap();
    assert_eq!(loaded, vec![entry]);
}

#[tokio::test]
async fn markdown_format_applies_only_to_new_files() {
    let directory = TempDir::new().unwrap();
    let mut plain = provider(directory.path());
    let first = plain.add_entry(draft()).await.unwrap();
    let original = only_journal(directory.path());
    drop(plain);
    let mut markdown = FileDataProvider::new(directory.path().to_owned(), FileFormat::Markdown);
    assert_eq!(
        markdown.load_all_entries().await.unwrap(),
        vec![first.clone()]
    );
    let second = markdown.add_entry(draft()).await.unwrap();
    assert_ne!(second.id, first.id);
    let mut changed = first;
    changed.title = "Changed".into();
    markdown.update_entry(changed).await.unwrap();
    assert!(original.exists());
    assert_eq!(
        journal_paths(directory.path())
            .iter()
            .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
            .count(),
        1
    );
    assert_eq!(markdown.load_all_entries().await.unwrap().len(), 2);
}

#[tokio::test]
async fn editing_title_date_and_content_keeps_the_filename_and_created_timestamp() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let mut entry = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let before = fs::read_to_string(&path).unwrap();
    let created = before
        .lines()
        .find(|line| line.starts_with("created:"))
        .unwrap()
        .to_owned();
    entry.title = "Renamed title".into();
    entry.date = Utc.with_ymd_and_hms(2010, 1, 2, 13, 14, 15).unwrap();
    entry.content = "No trailing newline".into();
    let updated = backend.update_entry(entry.clone()).await.unwrap();
    assert_eq!(updated, entry);
    assert_eq!(only_journal(directory.path()), path);
    let after = fs::read_to_string(path).unwrap();
    assert!(after.lines().any(|line| line == created));
    assert!(after.ends_with("No trailing newline"));
    assert_eq!(backend.load_all_entries().await.unwrap(), vec![entry]);
}

#[tokio::test]
async fn ignores_hidden_unrelated_binary_and_nested_files() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let expected = backend.add_entry(draft()).await.unwrap();
    let journal = fs::read(only_journal(directory.path())).unwrap();
    fs::write(directory.path().join(".backup.md"), &journal).unwrap();
    fs::write(directory.path().join("backup.txt"), &journal).unwrap();
    fs::write(directory.path().join("README.md"), "# Unrelated markdown\n").unwrap();
    fs::write(directory.path().join("notes"), "Unrelated plain text\n").unwrap();
    fs::write(directory.path().join("binary"), [0, 255, 1, 255]).unwrap();
    let nested = directory.path().join("subdirectory");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("entry.md"), journal).unwrap();
    assert_eq!(backend.load_all_entries().await.unwrap(), vec![expected]);
}

#[tokio::test]
async fn import_before_initial_load_preserves_existing_entries_and_assigns_distinct_ids() {
    let directory = TempDir::new().unwrap();
    let mut original = provider(directory.path());
    original.add_entry(draft()).await.unwrap();
    drop(original);
    let mut backend = provider(directory.path());
    let mut imported = draft();
    imported.title = "Imported".into();
    imported.priority = Some(u32::MAX);
    backend
        .import_entries(EntriesDTO::new(vec![imported.clone()]))
        .await
        .unwrap();
    let entries = backend.load_all_entries().await.unwrap();
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0].id, entries[1].id);
    assert!(
        entries
            .iter()
            .any(|entry| EntryDraft::from_entry(entry.clone()) == imported)
    );
}

#[tokio::test]
async fn rename_outside_app_preserves_id_after_explicit_reload() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let mut entry = backend.add_entry(draft()).await.unwrap();
    let renamed = directory.path().join("renamed.md");
    fs::rename(only_journal(directory.path()), &renamed).unwrap();
    entry.title = "Cannot save stale location".into();
    assert!(backend.update_entry(entry.clone()).await.is_err());
    let reloaded = backend.load_all_entries().await.unwrap();
    assert_eq!(reloaded[0].id, entry.id);
    backend.update_entry(entry).await.unwrap();
    assert_eq!(only_journal(directory.path()), renamed);
}

#[tokio::test]
async fn external_changes_block_update_and_delete_until_reload() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let mut entry = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let external = format!("{}external edit", fs::read_to_string(&path).unwrap());
    fs::write(&path, &external).unwrap();
    entry.content = "Stale app edit".into();
    assert!(backend.update_entry(entry.clone()).await.is_err());
    assert!(backend.remove_entry(entry.id).await.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
    let mut reloaded = backend.load_all_entries().await.unwrap().remove(0);
    assert!(reloaded.content.ends_with("external edit"));
    reloaded.content = "Edited after reload".into();
    backend.update_entry(reloaded).await.unwrap();
}

#[tokio::test]
async fn export_keeps_visible_snapshot_and_does_not_accept_external_changes() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let entry = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let external = format!("{}outside", fs::read_to_string(&path).unwrap());
    fs::write(&path, &external).unwrap();
    let exported = backend.get_export_object(&[entry.id]).await.unwrap();
    assert_eq!(exported.entries, vec![draft()]);
    assert!(backend.update_entry(entry).await.is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), external);
}

#[tokio::test]
async fn replacing_file_with_identical_contents_still_blocks_stale_write() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let mut entry = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let contents = fs::read(&path).unwrap();
    let replacement = directory.path().join(".replacement");
    fs::write(&replacement, &contents).unwrap();
    fs::rename(replacement, &path).unwrap();
    entry.title = "Attempted overwrite".into();
    assert!(backend.update_entry(entry).await.is_err());
    assert_eq!(fs::read(path).unwrap(), contents);
}

#[tokio::test]
async fn duplicate_uuid_blocks_writes_without_partial_reload() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let entry = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let contents = fs::read(&path).unwrap();
    let duplicate = directory.path().join("duplicate.md");
    fs::write(&duplicate, &contents).unwrap();
    assert!(backend.load_all_entries().await.is_err());
    assert!(backend.add_entry(draft()).await.is_err());
    assert!(backend.update_entry(entry.clone()).await.is_err());
    assert!(backend.remove_entry(entry.id).await.is_err());
    assert_eq!(fs::read(&path).unwrap(), contents);
    fs::remove_file(duplicate).unwrap();
    assert_eq!(backend.update_entry(entry.clone()).await.unwrap(), entry);
}

#[tokio::test]
async fn malformed_recognized_file_blocks_writes_until_repaired() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let entry = backend.add_entry(draft()).await.unwrap();
    let malformed = directory.path().join("malformed.md");
    fs::write(&malformed, "---\ntui_journal: [broken\n---\nBody").unwrap();
    assert!(backend.load_all_entries().await.is_err());
    assert!(backend.add_entry(draft()).await.is_err());
    assert!(backend.remove_entry(entry.id).await.is_err());
    fs::remove_file(malformed).unwrap();
    assert_eq!(backend.load_all_entries().await.unwrap(), vec![entry]);
}

#[tokio::test]
async fn unsupported_version_and_invalid_utf8_journals_are_reported() {
    let directory = TempDir::new().unwrap();
    let unsupported = directory.path().join("future.md");
    fs::write(&unsupported, "---\ntitle: Future\ndate: '2024-01-01T00:00:00Z'\ncreated: '2024-01-01T00:00:00Z'\ntags: []\ntui_journal:\n  version: 999\n  id: '8f86c7ae-c1a0-41ea-ad9e-86f3acd90fd3'\n---\nBody").unwrap();
    let mut backend = provider(directory.path());
    assert!(backend.load_all_entries().await.is_err());
    fs::remove_file(unsupported).unwrap();
    let invalid = directory.path().join("bad-encoding");
    for prefix in [b"".as_slice(), b"\xef\xbb\xbf".as_slice()] {
        let mut bytes = prefix.to_vec();
        bytes.extend_from_slice(b"---\ntui_journal: {}\n---\n\xff");
        fs::write(&invalid, &bytes).unwrap();
        assert!(backend.load_all_entries().await.is_err());
    }
}

#[tokio::test]
async fn invalid_utf8_body_with_bom_and_quoted_metadata_key_is_not_silently_skipped() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let contents = fs::read_to_string(&path)
        .unwrap()
        .replace("tui_journal:", "\"tui_journal\":");
    let mut bytes = b"\xef\xbb\xbf".to_vec();
    bytes.extend_from_slice(contents.as_bytes());
    bytes.push(255);
    fs::write(path, bytes).unwrap();
    assert!(backend.load_all_entries().await.is_err());
    assert!(backend.add_entry(draft()).await.is_err());
}

#[tokio::test]
async fn delete_and_restore_keep_original_filename_uuid_and_unknown_metadata() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let text = fs::read_to_string(&path).unwrap().replacen(
        "---\n",
        "---\ncustom:\n  source: handwritten\n",
        1,
    );
    fs::write(&path, &text).unwrap();
    let entry = backend.load_all_entries().await.unwrap().remove(0);
    let uid = text
        .lines()
        .find(|line| line.trim_start().starts_with("id:"))
        .unwrap()
        .trim()
        .to_owned();
    backend.remove_entry(entry.id).await.unwrap();
    backend.retain_restore_metadata(&[entry.id]);
    assert!(!path.exists());
    assert_eq!(backend.restore_entry(entry.clone()).await.unwrap(), entry);
    assert_eq!(only_journal(directory.path()), path);
    let restored = fs::read_to_string(path).unwrap();
    assert!(restored.contains("source: handwritten"));
    assert!(restored.lines().any(|line| line.trim() == uid));
    assert_eq!(backend.load_all_entries().await.unwrap(), vec![entry]);
}

#[tokio::test]
async fn occupied_restore_destination_is_preserved_and_can_be_retried() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let entry = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    backend.remove_entry(entry.id).await.unwrap();
    fs::write(&path, "Unrelated replacement").unwrap();
    assert!(backend.restore_entry(entry.clone()).await.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "Unrelated replacement");
    fs::remove_file(&path).unwrap();
    assert_eq!(backend.restore_entry(entry.clone()).await.unwrap(), entry);
}

#[tokio::test]
async fn restore_refuses_duplicate_uuid_at_another_filename() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let entry = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let contents = fs::read(&path).unwrap();
    backend.remove_entry(entry.id).await.unwrap();
    fs::write(directory.path().join("elsewhere.md"), contents).unwrap();
    assert!(backend.restore_entry(entry).await.is_err());
    assert!(!path.exists());
}

#[tokio::test]
async fn discarded_history_prunes_restore_metadata_without_reusing_ids() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let removed = backend.add_entry(draft()).await.unwrap();
    backend.remove_entry(removed.id).await.unwrap();
    backend.retain_restore_metadata(&[]);
    assert!(backend.restore_entry(removed.clone()).await.is_err());
    let next = backend.add_entry(draft()).await.unwrap();
    assert!(next.id > removed.id);
    backend.retain_restore_metadata(&[]);
    assert_eq!(backend.update_entry(next.clone()).await.unwrap(), next);
}

#[tokio::test]
async fn reloading_after_external_delete_never_reuses_an_old_id() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let old = backend.add_entry(draft()).await.unwrap();
    fs::remove_file(only_journal(directory.path())).unwrap();
    assert!(backend.load_all_entries().await.unwrap().is_empty());
    let next = backend.add_entry(draft()).await.unwrap();
    assert!(next.id > old.id);
}

#[tokio::test]
async fn a_second_provider_cannot_open_the_same_directory_until_lock_is_released() {
    let directory = TempDir::new().unwrap();
    let mut first = provider(directory.path());
    let entry = first.add_entry(draft()).await.unwrap();
    let mut second = provider(directory.path());
    assert!(second.load_all_entries().await.is_err());
    assert!(second.add_entry(draft()).await.is_err());
    drop(first);
    assert_eq!(second.load_all_entries().await.unwrap(), vec![entry]);
}

#[tokio::test]
async fn replacing_the_lock_file_blocks_later_operations() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let entry = backend.add_entry(draft()).await.unwrap();
    let lock = directory.path().join(".tui-journal.lock");
    fs::rename(&lock, directory.path().join(".old-lock")).unwrap();
    fs::write(lock, "").unwrap();
    assert!(backend.update_entry(entry.clone()).await.is_err());
    assert!(backend.remove_entry(entry.id).await.is_err());
    assert!(backend.load_all_entries().await.is_err());
}

#[tokio::test]
async fn replacing_the_root_directory_blocks_writes_to_the_replacement() {
    let parent = TempDir::new().unwrap();
    let directory = parent.path().join("journals");
    let mut backend = provider(&directory);
    let entry = backend.add_entry(draft()).await.unwrap();
    let old = parent.path().join("previous");
    fs::rename(&directory, &old).unwrap();
    fs::create_dir(&directory).unwrap();
    assert!(backend.update_entry(entry.clone()).await.is_err());
    assert!(backend.remove_entry(entry.id).await.is_err());
    assert!(backend.add_entry(draft()).await.is_err());
    assert!(fs::read_dir(directory).unwrap().next().is_none());
    assert_eq!(journal_paths(&old).len(), 1);
}

#[tokio::test]
async fn setting_priorities_changes_only_entries_without_a_priority() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let original = backend.add_entry(draft()).await.unwrap();
    let mut unassigned = draft();
    unassigned.priority = None;
    let added = backend.add_entry(unassigned).await.unwrap();
    backend.assign_priority_to_entries(0).await.unwrap();
    let loaded = backend.load_all_entries().await.unwrap();
    assert_eq!(
        loaded
            .iter()
            .find(|entry| entry.id == original.id)
            .unwrap()
            .priority,
        Some(2)
    );
    assert_eq!(
        loaded
            .iter()
            .find(|entry| entry.id == added.id)
            .unwrap()
            .priority,
        Some(0)
    );
}

#[tokio::test]
async fn restore_rejects_existing_ids_and_handles_exhausted_id_space_without_wraparound() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let entry = Entry::from_draft(u32::MAX, draft());
    assert_eq!(backend.restore_entry(entry.clone()).await.unwrap(), entry);
    assert!(backend.restore_entry(entry).await.is_err());
    assert!(backend.add_entry(draft()).await.is_err());
    assert_eq!(journal_paths(directory.path()).len(), 1);
}

#[tokio::test]
async fn empty_title_is_rejected_without_creating_the_directory() {
    let parent = TempDir::new().unwrap();
    let directory = parent.path().join("not-created");
    let mut backend = provider(&directory);
    let mut empty = draft();
    empty.title.clear();
    assert!(backend.add_entry(empty).await.is_err());
    assert!(!directory.exists());
}

#[tokio::test]
async fn unreadable_metadata_is_rejected_before_add_or_import_writes_a_journal() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let mut oversized = draft();
    oversized.tags = vec!["tag".into(); 40_000];
    assert!(backend.add_entry(oversized.clone()).await.is_err());
    assert!(
        backend
            .import_entries(EntriesDTO::new(vec![oversized]))
            .await
            .is_err()
    );
    assert!(backend.load_all_entries().await.unwrap().is_empty());
    assert!(journal_paths(directory.path()).is_empty());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    let entry = backend.add_entry(draft()).await.unwrap();
    assert_eq!(backend.load_all_entries().await.unwrap(), vec![entry]);
}

#[tokio::test]
async fn unreadable_metadata_update_preserves_file_and_cache_and_can_be_retried() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let original = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let original_bytes = fs::read(&path).unwrap();
    let mut update = original.clone();
    update.tags = vec!["tag".into(); 40_000];
    update.content = "Replacement body".into();
    assert!(backend.update_entry(update.clone()).await.is_err());
    assert_eq!(fs::read(&path).unwrap(), original_bytes);
    assert_eq!(
        backend.get_export_object(&[original.id]).await.unwrap(),
        EntriesDTO::new(vec![EntryDraft::from_entry(original.clone())])
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    update.tags = vec!["valid".into()];
    assert_eq!(backend.update_entry(update.clone()).await.unwrap(), update);
    assert_eq!(backend.load_all_entries().await.unwrap(), vec![update]);
}

#[cfg(unix)]
#[tokio::test]
async fn atomic_updates_preserve_permissions_and_leave_no_temporary_files() {
    use std::os::unix::fs::PermissionsExt;
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let mut entry = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    backend.load_all_entries().await.unwrap();
    entry.content = "Changed".into();
    backend.update_entry(entry).await.unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    let restored = backend.load_all_entries().await.unwrap().remove(0);
    backend.remove_entry(restored.id).await.unwrap();
    backend.restore_entry(restored).await.unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[cfg(unix)]
#[tokio::test]
async fn journal_and_lock_symlinks_are_rejected_without_modifying_their_target() {
    use std::os::unix::fs::symlink;
    let directory = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let target = outside.path().join("target");
    fs::write(&target, "Keep me").unwrap();
    let link = directory.path().join("linked.md");
    symlink(&target, &link).unwrap();
    let mut backend = provider(directory.path());
    assert!(backend.load_all_entries().await.is_err());
    fs::remove_file(link).unwrap();
    drop(backend);
    let lock = directory.path().join(".tui-journal.lock");
    fs::remove_file(&lock).unwrap();
    symlink(&target, lock).unwrap();
    assert!(provider(directory.path()).add_entry(draft()).await.is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "Keep me");
}

#[cfg(unix)]
#[tokio::test]
async fn retargeting_a_directory_symlink_blocks_writes() {
    use std::os::unix::fs::symlink;
    let parent = TempDir::new().unwrap();
    let first = parent.path().join("first");
    let second = parent.path().join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let link = parent.path().join("selected");
    symlink(&first, &link).unwrap();
    let mut backend = provider(&link);
    let entry = backend.add_entry(draft()).await.unwrap();
    fs::remove_file(&link).unwrap();
    symlink(second, link).unwrap();
    assert!(backend.update_entry(entry).await.is_err());
    assert_eq!(journal_paths(&first).len(), 1);
}

fn protected(entry: Entry) -> Entry {
    entry.with_protection(backend::EncryptedNote {
        version: 1,
        identity: age::x25519::Identity::generate().to_public().to_string(),
        ciphertext:
            "-----BEGIN AGE ENCRYPTED FILE-----\ndGVzdA==\n-----END AGE ENCRYPTED FILE-----\n"
                .into(),
    })
}

#[tokio::test]
async fn protection_removes_private_front_matter_and_restores_native_metadata() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let original = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let before = fs::read_to_string(&path).unwrap().replacen(
        "---\n",
        "---\n# Private comment\ncustom: private native value\n",
        1,
    );
    fs::write(&path, &before).unwrap();
    backend.load_all_entries().await.unwrap();
    let metadata = backend
        .protection_metadata(original.id)
        .await
        .unwrap()
        .unwrap();
    let locked = protected(original.clone());
    assert_eq!(backend.update_entry(locked.clone()).await.unwrap(), locked);
    let saved = fs::read_to_string(&path).unwrap();
    for private in [
        "Private comment",
        "private native value",
        "walking",
        "A journal",
        "priority:",
    ] {
        assert!(!saved.contains(private), "{private}");
    }
    assert!(saved.contains("version: 2"));
    assert!(saved.ends_with(&locked.protection.as_ref().unwrap().ciphertext));
    assert_eq!(only_journal(directory.path()), path);
    assert_eq!(
        backend.load_all_entries().await.unwrap(),
        vec![locked.clone()]
    );
    assert_eq!(
        backend
            .get_export_object(&[locked.id])
            .await
            .unwrap()
            .entries,
        vec![EntryDraft::from_entry(locked)]
    );
    backend
        .update_entry_with_metadata(original.clone(), Some(&metadata))
        .await
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert_eq!(backend.load_all_entries().await.unwrap(), vec![original]);
}

#[tokio::test]
async fn protected_import_keeps_full_timestamp_and_skips_bulk_priority() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let original = protected(Entry::from_draft(0, draft()));
    let imported = backend
        .add_entry(EntryDraft::from_entry(original.clone()))
        .await
        .unwrap();
    backend.assign_priority_to_entries(4).await.unwrap();
    assert_eq!(
        backend.load_all_entries().await.unwrap(),
        vec![imported.clone()]
    );
    assert_eq!(imported, original);
}

#[tokio::test]
async fn protected_file_rejects_unencrypted_private_fields_without_writing() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let original = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let before = fs::read(&path).unwrap();
    let mut invalid = protected(original);
    invalid.content = "must not escape".into();
    assert!(backend.update_entry(invalid).await.is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[tokio::test]
async fn protection_metadata_rejects_external_changes() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let original = backend.add_entry(draft()).await.unwrap();
    let path = only_journal(directory.path());
    let changed = fs::read_to_string(&path)
        .unwrap()
        .replace("A journal", "External journal");
    fs::write(&path, &changed).unwrap();
    assert!(backend.protection_metadata(original.id).await.is_err());
    assert!(backend.update_entry(protected(original)).await.is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), changed);
}

#[tokio::test]
async fn removing_protection_imported_from_other_backend_uses_file_metadata() {
    let directory = TempDir::new().unwrap();
    let mut backend = provider(directory.path());
    let original = Entry::from_draft(0, draft());
    backend
        .add_entry(EntryDraft::from_entry(protected(original.clone())))
        .await
        .unwrap();
    backend
        .update_entry_with_metadata(
            original.clone(),
            Some(r#"{"format":"vjournal-v1","document":"native calendar"}"#),
        )
        .await
        .unwrap();
    assert_eq!(backend.load_all_entries().await.unwrap(), vec![original]);
}
