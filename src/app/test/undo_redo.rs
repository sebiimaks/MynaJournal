use super::*;

#[tokio::test]
/// Test for adding Entry
async fn add() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();

    let original_count = app.get_active_entries().count();

    let added_title = "Added";

    let id = app
        .add_entry(added_title.into(), DateTime::default(), vec![], None)
        .await
        .unwrap();

    assert!(app.get_entry(id).is_some());

    app.undo().await.unwrap();

    assert_eq!(app.get_active_entries().count(), original_count);
    assert!(app.get_entry(id).is_none());

    let _id = app.redo().await.unwrap().unwrap();

    assert_eq!(app.get_active_entries().count(), original_count + 1);
    assert!(
        app.get_entry(id)
            .is_some_and(|entry| entry.title == added_title)
    )
}

#[tokio::test]
/// Test for removing Entry
async fn remove() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();

    let original_count = app.get_active_entries().count();
    let id = 1;
    let title = app.get_entry(id).unwrap().title.to_owned();

    app.delete_entry(id).await.unwrap();

    assert!(app.get_active_entries().all(|e| e.title != title));
    assert_eq!(app.get_active_entries().count(), original_count - 1);

    let _id = app.undo().await.unwrap().unwrap();

    assert!(app.get_active_entries().any(|e| e.title == title));
    assert_eq!(app.get_active_entries().count(), original_count);

    app.redo().await.unwrap();

    assert!(app.get_active_entries().all(|e| e.title != title));
    assert_eq!(app.get_active_entries().count(), original_count - 1);
}

#[tokio::test]
/// Test for Updating entry attributes
async fn update_attributes() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();

    app.current_entry_id = Some(1);

    let current = app.get_current_entry().unwrap();

    let id = current.id;
    let original_title = current.title.to_owned();
    let changed_title = "Changed_Title";

    app.update_current_entry_attributes(
        changed_title.into(),
        current.date,
        current.tags.to_owned(),
        current.priority,
    )
    .await
    .unwrap();

    let update_entry = app.get_entry(id).unwrap();
    assert_eq!(&update_entry.title, changed_title);

    let _id = app.undo().await.unwrap().unwrap();

    let undo_entry = app.get_entry(id).unwrap();
    assert_eq!(undo_entry.title, original_title);

    let _id = app.redo().await.unwrap().unwrap();
    let redo_entry = app.get_entry(id).unwrap();
    assert_eq!(redo_entry.title, changed_title);
}

#[tokio::test]
/// Test for Updating Entry Content
async fn update_content() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();

    app.current_entry_id = Some(1);

    let current = app.get_current_entry().unwrap();

    let id = current.id;
    let original_content = current.content.to_owned();
    let changed_content = "Changed_content";

    app.update_current_entry_content(changed_content.into())
        .await
        .unwrap();

    let update_entry = app.get_entry(id).unwrap();
    assert_eq!(&update_entry.content, changed_content);

    let _id = app.undo().await.unwrap().unwrap();

    let undo_entry = app.get_entry(id).unwrap();
    assert_eq!(undo_entry.content, original_content);

    let _id = app.redo().await.unwrap().unwrap();
    let redo_entry = app.get_entry(id).unwrap();
    assert_eq!(redo_entry.content, changed_content);
}

#[tokio::test]
async fn undo_redo_after_restoring_deleted_entry_keeps_history_id() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();

    let a_id = app
        .add_entry("A".into(), DateTime::default(), vec![], None)
        .await
        .unwrap();
    let _b_id = app
        .add_entry("B".into(), DateTime::default(), vec![], None)
        .await
        .unwrap();

    app.current_entry_id = Some(a_id);
    app.update_current_entry_content("edited content".into())
        .await
        .unwrap();
    app.delete_entry(a_id).await.unwrap();

    let restored_id = app.undo().await.unwrap().unwrap();
    assert_eq!(restored_id, a_id);

    app.undo().await.unwrap();
    assert_eq!(app.get_entry(a_id).unwrap().content, "");

    app.redo().await.unwrap();
    assert_eq!(app.get_entry(a_id).unwrap().content, "edited content");

    app.redo().await.unwrap();
    assert!(app.get_entry(a_id).is_none());
}

#[tokio::test]
/// This test will run multiple delete calls, undo do them, then redo them
async fn many() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();

    let original_count = app.get_active_entries().count();
    let mut current_count = original_count;

    while current_count > 0 {
        let id = app.entries.first().unwrap().id;
        app.delete_entry(id).await.unwrap();
        current_count -= 1;
        assert_eq!(app.entries.len(), current_count);
    }

    for _ in 0..original_count {
        app.undo().await.unwrap();
        current_count += 1;
        assert_eq!(app.entries.len(), current_count);
    }

    for _ in 0..original_count {
        app.redo().await.unwrap();
        current_count -= 1;
        assert_eq!(app.entries.len(), current_count);
    }
}

#[tokio::test]
async fn failed_undo_and_redo_of_content_can_be_retried() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(1);
    let original = app.get_current_entry().unwrap().content.clone();
    app.update_current_entry_content("Changed".into())
        .await
        .unwrap();

    app.data_provide.set_return_err(true);
    assert!(app.undo().await.is_err());
    assert_eq!(app.get_current_entry().unwrap().content, "Changed");
    app.data_provide.set_return_err(false);
    assert_eq!(app.undo().await.unwrap(), Some(1));
    assert_eq!(app.get_current_entry().unwrap().content, original);
    assert!(app.undo().await.unwrap().is_none());

    app.data_provide.set_return_err(true);
    assert!(app.redo().await.is_err());
    assert_eq!(app.get_current_entry().unwrap().content, original);
    app.data_provide.set_return_err(false);
    assert_eq!(app.redo().await.unwrap(), Some(1));
    assert_eq!(app.get_current_entry().unwrap().content, "Changed");
    assert!(app.redo().await.unwrap().is_none());
}

#[tokio::test]
async fn failed_history_restore_keeps_metadata_until_retry_succeeds() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    let original = app.get_entry(1).unwrap().clone();
    app.delete_entry(1).await.unwrap();

    app.data_provide.set_return_err(true);
    assert!(app.undo().await.is_err());
    assert!(app.get_entry(1).is_none());
    assert!(app.data_provide.restore_metadata.contains(&1));

    app.data_provide.set_return_err(false);
    assert_eq!(app.undo().await.unwrap(), Some(1));
    assert_eq!(app.get_entry(1), Some(&original));
    assert!(app.data_provide.restored_with_metadata);
    assert!(app.data_provide.restore_metadata.is_empty());

    app.data_provide.set_return_err(true);
    assert!(app.redo().await.is_err());
    assert_eq!(app.get_entry(1), Some(&original));
    app.data_provide.set_return_err(false);
    app.redo().await.unwrap();
    assert!(app.get_entry(1).is_none());
    assert!(app.data_provide.restore_metadata.contains(&1));
}

#[tokio::test]
async fn redo_restore_metadata_survives_new_undo_history() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(1);
    let id = app
        .add_entry("New entry".into(), DateTime::default(), vec![], None)
        .await
        .unwrap();
    app.undo().await.unwrap();
    app.update_current_entry_content("Changed".into())
        .await
        .unwrap();
    assert!(app.data_provide.restore_metadata.contains(&id));

    app.data_provide.set_return_err(true);
    assert!(app.redo().await.is_err());
    assert!(app.data_provide.restore_metadata.contains(&id));
    app.data_provide.set_return_err(false);
    assert_eq!(app.redo().await.unwrap(), Some(id));
    assert!(app.data_provide.restored_with_metadata);
}

#[tokio::test]
async fn history_eviction_releases_deleted_entry_metadata() {
    for operation in ["add", "attributes", "content", "remove"] {
        let mut app = create_default_app();
        app.history = HistoryManager::new(1);
        app.load_entries().await.unwrap();
        app.current_entry_id = Some(1);
        app.delete_entry(0).await.unwrap();
        assert!(app.data_provide.restore_metadata.contains(&0));

        match operation {
            "add" => {
                app.add_entry("New".into(), DateTime::default(), vec![], None)
                    .await
                    .unwrap();
            }
            "attributes" => {
                app.update_current_entry_attributes(
                    "Changed".into(),
                    DateTime::default(),
                    vec![],
                    None,
                )
                .await
                .unwrap();
            }
            "content" => {
                app.update_current_entry_content("Changed".into())
                    .await
                    .unwrap();
            }
            "remove" => app.delete_entry(1).await.unwrap(),
            _ => unreachable!(),
        }

        assert!(
            !app.data_provide.restore_metadata.contains(&0),
            "metadata retained after {operation} evicted its history"
        );
    }
}

#[tokio::test]
async fn disabled_history_releases_deleted_entry_metadata_immediately() {
    let mut app = create_default_app();
    app.history = HistoryManager::new(0);
    app.load_entries().await.unwrap();
    app.delete_entry(0).await.unwrap();
    assert!(app.data_provide.restore_metadata.is_empty());
    assert!(app.undo().await.unwrap().is_none());
}

#[tokio::test]
async fn missing_entry_history_returns_an_error_and_can_retry_after_reload() {
    for change_attributes in [false, true] {
        for redo in [false, true] {
            let mut app = create_default_app();
            app.load_entries().await.unwrap();
            app.current_entry_id = Some(1);
            let original = app.get_entry(1).unwrap().clone();
            if change_attributes {
                app.update_current_entry_attributes(
                    "Edited title".into(),
                    original.date,
                    original.tags.clone(),
                    original.priority,
                )
                .await
                .unwrap();
            } else {
                app.update_current_entry_content("Edited content".into())
                    .await
                    .unwrap();
            }
            let edited = app.get_entry(1).unwrap().clone();
            if redo {
                app.undo().await.unwrap();
            }
            let before = app.get_entry(1).unwrap().clone();

            app.data_provide.remove_entry(1).await.unwrap();
            app.load_entries().await.unwrap();
            app.current_entry_id = None;
            let result = if redo {
                app.redo().await
            } else {
                app.undo().await
            };
            assert!(result.unwrap_err().to_string().contains("no longer loaded"));
            assert!(app.get_entry(1).is_none());

            app.data_provide.restore_entry(before).await.unwrap();
            app.load_entries().await.unwrap();
            let result = if redo {
                app.redo().await
            } else {
                app.undo().await
            };
            assert_eq!(result.unwrap(), Some(1));
            assert_eq!(
                app.get_entry(1),
                Some(if redo { &edited } else { &original })
            );
        }
    }
}
