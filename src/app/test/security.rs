use backend::protection::{self, SecretString};

use super::*;

const PASSWORD: &str = "app protection test password";
const NEW_PASSWORD: &str = "replacement protection test password";
const PRIVATE_BODY: &str = "PRIVATE_LOCKED_NOTE_BODY";
const PRIVATE_TAG: &str = "PRIVATE_LOCKED_NOTE_TAG";

fn password(value: &str) -> SecretString {
    SecretString::from(value.to_owned())
}

async fn protected_app() -> App<MockDataProvider> {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(0);
    let entry = app.get_current_entry().unwrap().clone();
    app.update_current_entry_attributes(entry.title, entry.date, vec![PRIVATE_TAG.into()], Some(3))
        .await
        .unwrap();
    app.protect_note(0, password(PASSWORD), PRIVATE_BODY.into())
        .await
        .unwrap();
    app
}

async fn stored_entry(app: &mut App<MockDataProvider>, id: u32) -> Entry {
    app.data_provide
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.id == id)
        .unwrap()
}

fn assert_encrypted(entry: &Entry) {
    assert!(entry.protection.is_some());
    assert!(entry.content.is_empty());
    assert!(entry.tags.is_empty());
    assert!(entry.priority.is_none());
    entry.validate_protection().unwrap();
}

#[tokio::test]
async fn protect_unlock_save_relock_and_remove_protection() {
    let mut app = protected_app().await;
    assert!(app.current_is_protected());
    assert!(app.current_is_locked());
    let original = stored_entry(&mut app, 0).await;
    assert_eq!(original.title, "Title 1");
    assert_eq!(original.date, get_default_entries()[0].date);
    assert_encrypted(&original);
    assert_eq!(app.get_current_entry(), Some(&original));
    assert!(app.get_all_tags().is_empty());
    assert!(app.undo().await.unwrap().is_none());

    assert_eq!(app.unlock_note(0, password(PASSWORD)).await.unwrap(), None);
    let unlocked = app.get_current_entry().unwrap();
    assert!(!app.current_is_locked());
    assert_eq!(unlocked.content, PRIVATE_BODY);
    assert_eq!(unlocked.tags, [PRIVATE_TAG]);
    assert_eq!(unlocked.priority, Some(3));
    assert_eq!(stored_entry(&mut app, 0).await, original);

    let edited = "PRIVATE_LOCKED_NOTE_EDIT";
    app.update_current_entry_content(edited.into())
        .await
        .unwrap();
    let saved = stored_entry(&mut app, 0).await;
    assert_encrypted(&saved);
    assert_ne!(saved.protection, original.protection);
    assert_eq!(app.get_current_entry().unwrap().content, edited);
    assert!(app.undo().await.unwrap().is_none());
    app.lock_note(0);
    assert!(app.current_is_locked());
    assert_eq!(app.get_current_entry(), Some(&saved));

    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    assert_eq!(app.get_current_entry().unwrap().content, edited);
    app.remove_note_protection(0).await.unwrap();
    assert!(!app.current_is_protected());
    assert!(!app.current_is_locked());
    assert!(app.security.sessions.is_empty());
    let plain = stored_entry(&mut app, 0).await;
    assert_eq!(plain.content, edited);
    assert_eq!(plain.tags, [PRIVATE_TAG]);
    assert_eq!(plain.priority, Some(3));
    assert!(plain.protection.is_none());
    assert_eq!(app.get_current_entry(), Some(&plain));
    assert!(app.undo().await.unwrap().is_none());
}

#[tokio::test]
async fn wrong_password_does_not_unlock_and_password_change_invalidates_old_password() {
    let mut app = protected_app().await;
    let original = stored_entry(&mut app, 0).await;
    assert!(
        app.unlock_note(0, password("wrong password"))
            .await
            .is_err()
    );
    assert!(app.current_is_locked());
    assert!(app.security.sessions.is_empty());
    assert_eq!(stored_entry(&mut app, 0).await, original);

    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    app.change_note_password(0, password(NEW_PASSWORD))
        .await
        .unwrap();
    let changed = stored_entry(&mut app, 0).await;
    assert_encrypted(&changed);
    assert_ne!(changed.protection, original.protection);
    assert_eq!(app.get_current_entry().unwrap().content, PRIVATE_BODY);
    app.lock_note(0);
    assert!(app.unlock_note(0, password(PASSWORD)).await.is_err());
    assert!(app.current_is_locked());
    app.unlock_note(0, password(NEW_PASSWORD)).await.unwrap();
    assert_eq!(app.get_current_entry().unwrap().content, PRIVATE_BODY);
}

#[tokio::test]
async fn failed_initial_protection_preserves_plaintext_note_and_undo_history() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(0);
    let before_edit = app.get_current_entry().unwrap().clone();
    app.update_current_entry_content("Existing saved edit".into())
        .await
        .unwrap();
    let original = app.get_current_entry().unwrap().clone();
    app.data_provide.set_return_err(true);
    assert!(
        app.protect_note(0, password(PASSWORD), "New unsaved draft".into())
            .await
            .is_err()
    );
    assert_eq!(app.get_current_entry(), Some(&original));
    assert!(!app.current_is_protected());
    assert!(app.security.sessions.is_empty());
    app.data_provide.set_return_err(false);
    assert_eq!(stored_entry(&mut app, 0).await, original);
    assert_eq!(app.undo().await.unwrap(), Some(0));
    assert_eq!(app.get_current_entry(), Some(&before_edit));
}

#[tokio::test]
async fn failed_save_password_change_and_removal_keep_original_ciphertext_and_password() {
    let mut app = protected_app().await;
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    let original = stored_entry(&mut app, 0).await;
    let session = app.get_current_entry().unwrap().clone();
    app.data_provide.set_return_err(true);
    assert!(
        app.update_current_entry_content("REJECTED_PRIVATE_EDIT".into())
            .await
            .is_err()
    );
    assert!(
        app.change_note_password(0, password(NEW_PASSWORD))
            .await
            .is_err()
    );
    assert!(app.remove_note_protection(0).await.is_err());
    assert_eq!(app.get_current_entry(), Some(&session));
    assert!(!app.current_is_locked());
    assert!(app.history.pop_undo().is_none());
    app.data_provide.set_return_err(false);
    assert_eq!(stored_entry(&mut app, 0).await, original);

    app.update_current_entry_content("Saved after retry".into())
        .await
        .unwrap();
    let saved = stored_entry(&mut app, 0).await;
    assert_eq!(
        protection::unlock(&saved, &password(PASSWORD))
            .unwrap()
            .content,
        "Saved after retry"
    );
    assert!(protection::unlock(&saved, &password(NEW_PASSWORD)).is_err());
    app.lock_note(0);
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    assert_eq!(
        app.get_current_entry().unwrap().content,
        "Saved after retry"
    );
}

#[tokio::test]
async fn locked_edits_are_rejected_and_unlocked_public_metadata_is_resealed() {
    let mut app = protected_app().await;
    let original = stored_entry(&mut app, 0).await;
    let date = Utc.with_ymd_and_hms(2025, 6, 7, 8, 9, 10).unwrap();
    assert!(
        app.update_current_entry_content("Rejected".into())
            .await
            .is_err()
    );
    assert!(
        app.update_current_entry_attributes("Rejected".into(), date, vec![], None)
            .await
            .is_err()
    );
    assert_eq!(stored_entry(&mut app, 0).await, original);

    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    app.update_current_entry_attributes(
        "New public title".into(),
        date,
        vec!["NEW_PRIVATE_TAG".into()],
        Some(4),
    )
    .await
    .unwrap();
    let changed = stored_entry(&mut app, 0).await;
    assert_encrypted(&changed);
    assert_eq!(changed.title, "New public title");
    assert_eq!(changed.date, date);
    assert_ne!(changed.protection, original.protection);
    app.lock_note(0);
    assert_eq!(app.get_current_entry(), Some(&changed));
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    let current = app.get_current_entry().unwrap();
    assert_eq!(current.title, "New public title");
    assert_eq!(current.date, date);
    assert_eq!(current.content, PRIVATE_BODY);
    assert_eq!(current.tags, ["NEW_PRIVATE_TAG"]);
    assert_eq!(current.priority, Some(4));
}

#[tokio::test]
async fn bulk_export_uses_ciphertext_even_while_note_is_unlocked() {
    let mut app = protected_app().await;
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    app.selected_entries.extend([0, 1]);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("export.json");
    app.export_entries(path.clone()).await.unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    assert!(!text.contains(PRIVATE_BODY));
    assert!(!text.contains(PRIVATE_TAG));
    assert!(text.contains("Title 1"));
    assert!(text.contains("Content 2"));
    assert!(text.contains("AGE ENCRYPTED FILE"));
    let exported: EntriesDTO = serde_json::from_str(&text).unwrap();
    assert_eq!(exported.entries.len(), 2);
    let protected = exported
        .entries
        .into_iter()
        .find(|entry| entry.protection.is_some())
        .unwrap();
    let entry = Entry::from_draft(99, protected);
    assert_encrypted(&entry);
    assert_eq!(
        protection::unlock(&entry, &password(PASSWORD))
            .unwrap()
            .content,
        PRIVATE_BODY
    );
}

#[tokio::test]
async fn protection_discards_plaintext_undo_redo_and_filters_cannot_disclose_private_fields() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(0);
    app.update_current_entry_content("PRIVATE_HISTORY_FIRST".into())
        .await
        .unwrap();
    app.update_current_entry_content("PRIVATE_HISTORY_SECOND".into())
        .await
        .unwrap();
    app.undo().await.unwrap();
    let original = app.get_current_entry().unwrap().clone();
    app.protect_note(0, password(PASSWORD), PRIVATE_BODY.into())
        .await
        .unwrap();
    assert!(app.undo().await.unwrap().is_none());
    assert!(app.redo().await.unwrap().is_none());
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    assert_eq!(app.get_current_entry().unwrap().tags, original.tags);
    assert!(app.get_all_tags().is_empty());

    for unlocked in [true, false] {
        if !unlocked {
            app.lock_note(0);
        }
        for criterion in [
            FilterCriterion::Content(PRIVATE_BODY.into()),
            FilterCriterion::Content(String::new()),
            FilterCriterion::Tag(TagFilterOption::Tag("Tag 1".into())),
            FilterCriterion::Tag(TagFilterOption::NoTags),
            FilterCriterion::Priority(3),
        ] {
            app.apply_filter(Some(Filter {
                criteria: vec![criterion],
                ..Filter::default()
            }));
            assert!(app.get_active_entries().all(|entry| entry.id != 0));
        }
        app.apply_filter(Some(Filter {
            criteria: vec![FilterCriterion::Title(original.title.clone())],
            ..Filter::default()
        }));
        assert_eq!(
            app.get_active_entries()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            [0]
        );
        app.apply_filter(None);
    }
}

#[tokio::test]
async fn protected_deletion_and_undo_restore_only_encrypted_records() {
    let mut app = protected_app().await;
    let original = stored_entry(&mut app, 0).await;
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    app.delete_entry(0).await.unwrap();
    assert!(app.get_entry(0).is_none());
    assert!(app.security.sessions.is_empty());
    let change = app.history.pop_undo().unwrap();
    let Change::RemoveEntry(entry) = &change else {
        panic!("Expected deletion history");
    };
    assert_eq!(entry.as_ref(), &original);
    assert_encrypted(entry);
    app.history
        .restore_failed_change(change, HistoryStack::Undo);
    assert_eq!(app.undo().await.unwrap(), Some(0));
    assert!(app.is_locked(0));
    assert_eq!(stored_entry(&mut app, 0).await, original);
    assert!(app.data_provide.restored_with_metadata);
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    assert_eq!(app.get_entry(0).unwrap().content, PRIVATE_BODY);
    app.redo().await.unwrap();
    assert!(app.get_entry(0).is_none());
    assert!(app.security.sessions.is_empty());
    assert_eq!(app.undo().await.unwrap(), Some(0));
    assert!(app.is_locked(0));
    assert_encrypted(app.get_entry(0).unwrap());
}

#[tokio::test]
async fn encrypted_unsaved_draft_survives_wrong_password_and_blocks_reload_and_delete() {
    let mut app = protected_app().await;
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    let original = stored_entry(&mut app, 0).await;
    app.seal_draft_and_lock(0, "PRIVATE_UNSAVED_DRAFT".into(), None)
        .await
        .unwrap();
    assert!(app.is_locked(0));
    assert!(app.has_sealed_draft(0));
    assert!(app.security.sessions.is_empty());
    assert_eq!(stored_entry(&mut app, 0).await, original);
    assert!(
        app.unlock_note(0, password("wrong password"))
            .await
            .is_err()
    );
    assert!(app.has_sealed_draft(0));
    assert!(app.load_entries().await.is_err());
    assert!(app.delete_entry(0).await.is_err());
    let draft = app.unlock_note(0, password(PASSWORD)).await.unwrap();
    assert_eq!(draft.as_deref(), Some("PRIVATE_UNSAVED_DRAFT"));
    assert!(!app.has_sealed_drafts());
    assert_eq!(app.get_entry(0).unwrap().content, PRIVATE_BODY);
    app.update_current_entry_content(draft.unwrap().content.clone())
        .await
        .unwrap();
    app.lock_note(0);
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    assert_eq!(app.get_entry(0).unwrap().content, "PRIVATE_UNSAVED_DRAFT");
}

#[tokio::test]
async fn changed_record_cannot_silently_replace_an_encrypted_unsaved_draft() {
    let mut app = protected_app().await;
    app.unlock_note(0, password(PASSWORD)).await.unwrap();
    let original = stored_entry(&mut app, 0).await;
    app.seal_draft_and_lock(0, "PRIVATE_UNSAVED_DRAFT".into(), None)
        .await
        .unwrap();
    app.entries
        .iter_mut()
        .find(|entry| entry.id == 0)
        .unwrap()
        .title = "Externally changed title".into();
    assert!(app.unlock_note(0, password(PASSWORD)).await.is_err());
    assert!(app.has_sealed_draft(0));
    assert!(app.security.sessions.is_empty());
    *app.entries.iter_mut().find(|entry| entry.id == 0).unwrap() = original;
    assert_eq!(
        app.unlock_note(0, password(PASSWORD))
            .await
            .unwrap()
            .as_deref(),
        Some("PRIVATE_UNSAVED_DRAFT")
    );
}

#[tokio::test]
async fn navigating_normal_notes_keeps_undo_and_redo_history() {
    use ui::{Styles, UICommand};
    let mut app = create_default_app();
    app.settings.security.lock_on_leave = true;
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    let original = app.get_current_entry().unwrap().content.clone();
    app.update_current_entry_content("Normal edited content".into())
        .await
        .unwrap();
    ui.set_current_entry(Some(1), &mut app);
    UICommand::Undo.execute(&mut ui, &mut app).await.unwrap();
    assert_eq!(app.current_entry_id, Some(0));
    assert_eq!(app.get_current_entry().unwrap().content, original);
    ui.set_current_entry(Some(1), &mut app);
    UICommand::Redo.execute(&mut ui, &mut app).await.unwrap();
    assert_eq!(app.current_entry_id, Some(0));
    assert_eq!(
        app.get_current_entry().unwrap().content,
        "Normal edited content"
    );
}
