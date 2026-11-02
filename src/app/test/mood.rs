use backend::Mood;

use super::*;

#[tokio::test]
async fn changing_mood_undo_redo_and_delete_restore_keep_the_original_prompt() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    let id = app
        .add_entry(
            "Reflection".into(),
            Utc::now(),
            vec![],
            None,
            Some(Mood::Low),
            true,
        )
        .await
        .unwrap();
    app.current_entry_id = Some(id);
    let original = app.get_current_entry().unwrap().clone();
    assert_eq!(original.journal_prompt.as_ref().unwrap().mood, Mood::Low);
    app.update_current_entry_attributes(
        original.title.clone(),
        original.date,
        vec![],
        None,
        Some(Mood::High),
    )
    .await
    .unwrap();
    assert_eq!(app.get_current_entry().unwrap().mood, Some(Mood::High));
    assert_eq!(
        app.get_current_entry().unwrap().journal_prompt,
        original.journal_prompt
    );
    app.undo().await.unwrap();
    assert_eq!(app.get_current_entry(), Some(&original));
    app.redo().await.unwrap();
    let edited = app.get_current_entry().unwrap().clone();
    app.delete_entry(id).await.unwrap();
    app.undo().await.unwrap();
    assert_eq!(app.get_entry(id), Some(&edited));
    app.load_entries().await.unwrap();
    assert_eq!(app.get_entry(id), Some(&edited));
}

#[tokio::test]
async fn prompt_creation_requires_both_settings_a_selected_mood_and_entry_consent() {
    for show_mood in [false, true] {
        for prompts in [false, true] {
            for mood in [None, Some(Mood::Average)] {
                for include_prompt in [false, true] {
                    let mut app = create_default_app();
                    app.settings.show_entry_mood = show_mood;
                    app.settings.journal_prompts = prompts;
                    let id = app
                        .add_entry(
                            "Title".into(),
                            Utc::now(),
                            vec![],
                            None,
                            mood,
                            include_prompt,
                        )
                        .await
                        .unwrap();
                    let entry = app.get_entry(id).unwrap();
                    assert_eq!(entry.mood, mood.filter(|_| show_mood));
                    assert_eq!(
                        entry.journal_prompt.is_some(),
                        show_mood && prompts && mood.is_some() && include_prompt
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn failed_mood_save_keeps_entry_and_history_unchanged() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(0);
    let original = app.get_current_entry().unwrap().clone();
    app.data_provide.set_return_err(true);
    assert!(
        app.update_current_entry_attributes(
            original.title.clone(),
            original.date,
            original.tags.clone(),
            original.priority,
            Some(Mood::Low)
        )
        .await
        .is_err()
    );
    assert_eq!(app.get_current_entry(), Some(&original));
    assert!(app.history.pop_undo().is_none());
}

#[tokio::test]
async fn locking_seals_mood_and_prompt_and_unlock_restores_them() {
    let mut app = create_default_app();
    let id = app
        .add_entry(
            "Private reflection".into(),
            Utc::now(),
            vec![],
            None,
            Some(Mood::Average),
            true,
        )
        .await
        .unwrap();
    let original = app.get_entry(id).unwrap().clone();
    let password = backend::protection::SecretString::from("mood-test-password".to_owned());
    app.protect_note(id, password.clone(), "Body".into())
        .await
        .unwrap();
    let stored = app
        .data_provide
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.id == id)
        .unwrap();
    assert!(stored.mood.is_none());
    assert!(stored.journal_prompt.is_none());
    let json = serde_json::to_string(&stored).unwrap();
    assert!(!json.contains(&original.journal_prompt.as_ref().unwrap().id));
    app.unlock_note(id, password).await.unwrap();
    assert_eq!(app.get_entry(id).unwrap().mood, original.mood);
    assert_eq!(
        app.get_entry(id).unwrap().journal_prompt,
        original.journal_prompt
    );
    app.current_entry_id = Some(id);
    app.update_current_entry_attributes(
        original.title,
        original.date,
        vec![],
        None,
        Some(Mood::High),
    )
    .await
    .unwrap();
    app.lock_note(id);
    assert!(app.get_entry(id).unwrap().mood.is_none());
    app.unlock_note(
        id,
        backend::protection::SecretString::from("mood-test-password".to_owned()),
    )
    .await
    .unwrap();
    assert_eq!(app.get_entry(id).unwrap().mood, Some(Mood::High));
    assert_eq!(
        app.get_entry(id).unwrap().journal_prompt,
        original.journal_prompt
    );
}
