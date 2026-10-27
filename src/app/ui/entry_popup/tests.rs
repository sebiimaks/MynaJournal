use super::*;
use crate::app::test::create_default_app;

#[tokio::test]
async fn editing_metadata_preserves_the_full_original_timestamp() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(1);
    let timestamp = "2026-10-23T19:42:18.123456789+10:00"
        .parse::<DateTime<chrono::FixedOffset>>()
        .unwrap()
        .with_timezone(&Utc);
    app.update_current_entry_attributes("Imported".into(), timestamp, vec![], None)
        .await
        .unwrap();

    let mut popup = EntryPopup::from_entry(app.get_current_entry().unwrap(), &app.settings);
    popup.title_txt = TextArea::new(vec!["Edited title".into()]);
    popup.tags_txt = TextArea::new(vec!["first, second".into()]);
    popup.priority_txt = TextArea::new(vec!["3".into()]);
    assert_eq!(
        popup.handle_confirm(&mut app).await.unwrap(),
        EntryPopupInputReturn::UpdateCurrentEntry
    );

    let entry = app.get_current_entry().unwrap();
    assert_eq!(entry.date, timestamp);
    assert_eq!(entry.title, "Edited title");
    assert_eq!(entry.tags, ["first", "second"]);
    assert_eq!(entry.priority, Some(3));
    app.load_entries().await.unwrap();
    assert_eq!(app.get_current_entry().unwrap().date, timestamp);
}

#[tokio::test]
async fn changing_the_calendar_date_uses_midnight_on_the_new_day() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(1);
    let mut popup = EntryPopup::from_entry(app.get_current_entry().unwrap(), &app.settings);
    popup.date_txt = TextArea::new(vec!["23-10-2026".into()]);
    popup.handle_confirm(&mut app).await.unwrap();
    assert_eq!(
        app.get_current_entry().unwrap().date,
        Utc.with_ymd_and_hms(2026, 10, 23, 0, 0, 0).unwrap()
    );
}

#[tokio::test]
async fn failed_metadata_save_can_retry_without_losing_the_timestamp() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(1);
    let original = app.get_current_entry().unwrap().clone();
    let mut popup = EntryPopup::from_entry(&original, &app.settings);
    popup.title_txt = TextArea::new(vec!["Retry title".into()]);
    app.data_provide.set_return_err(true);
    assert!(popup.handle_confirm(&mut app).await.is_err());
    assert_eq!(app.get_current_entry(), Some(&original));

    app.data_provide.set_return_err(false);
    popup.handle_confirm(&mut app).await.unwrap();
    let entry = app.get_current_entry().unwrap();
    assert_eq!(entry.title, "Retry title");
    assert_eq!(entry.date, original.date);
}

#[tokio::test]
async fn new_entries_keep_auto_title_and_calendar_date_behavior() {
    let mut app = create_default_app();
    app.settings = toml::from_str("auto_title = 'Daily journal'").unwrap();
    let mut popup = EntryPopup::new_entry(&app.settings);
    assert_eq!(popup.title_txt.lines(), ["Daily journal"]);
    popup.date_txt = TextArea::new(vec!["23-10-2026".into()]);
    let EntryPopupInputReturn::AddEntry(id) = popup.handle_confirm(&mut app).await.unwrap() else {
        panic!("valid new entry was not added");
    };
    let entry = app.get_entry(id).unwrap();
    assert_eq!(entry.title, "Daily journal");
    assert_eq!(
        entry.date,
        Utc.with_ymd_and_hms(2026, 10, 23, 0, 0, 0).unwrap()
    );
}

#[test]
fn encrypted_draft_snapshot_preserves_partial_form_and_tag_picker_without_history() {
    let mut popup = EntryPopup::from_entry(
        &Entry::new(
            7,
            Utc::now(),
            "Original".into(),
            "Body".into(),
            vec!["private tag".into()],
            Some(3),
        ),
        &Settings::default(),
    );
    popup.title_txt = TextArea::new(vec!["Unsaved private title".into()]);
    popup.title_txt.move_cursor(CursorMove::Jump(0, 6));
    popup.title_txt.set_yank_text("private clipboard history");
    popup.date_txt = TextArea::new(vec!["24-1".into()]);
    popup.priority_txt = TextArea::new(vec!["invalid priority".into()]);
    popup.tags_txt = TextArea::new(vec!["private tag, new tag".into()]);
    popup.active_txt = ActiveText::Tags;
    popup.tags_popup = Some(TagsPopup::new(
        "private tag, new tag",
        vec!["private tag".into(), "other tag".into()],
    ));
    popup
        .tags_popup
        .as_mut()
        .unwrap()
        .handle_input(&Input::new(KeyCode::Char(' '), KeyModifiers::NONE));
    popup
        .tags_popup
        .as_mut()
        .unwrap()
        .handle_input(&Input::new(KeyCode::Down, KeyModifiers::NONE));
    popup.validate_all();
    assert!(!popup.is_input_valid());
    let snapshot = popup.snapshot().unwrap();
    assert!(!snapshot.contains("private clipboard history"));
    let mut restored = EntryPopup::from_snapshot(&snapshot).unwrap();
    assert_eq!(restored.snapshot().unwrap(), snapshot);
    assert_eq!(restored.original_date, popup.original_date);
    assert!(!restored.is_input_valid());
    assert_eq!(restored.date_err_msg, popup.date_err_msg);
    assert_eq!(restored.priority_err_msg, popup.priority_err_msg);
    assert_eq!(restored.title_txt.yank_text(), "");
    assert!(!restored.title_txt.undo());
    restored.handle_tags_popup_input(&Input::new(KeyCode::Enter, KeyModifiers::NONE));
    popup.handle_tags_popup_input(&Input::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(restored.tags_txt.lines(), popup.tags_txt.lines());
}

#[tokio::test]
async fn restored_new_entry_form_keeps_auto_title_and_can_be_saved() {
    let mut app = create_default_app();
    app.settings.auto_title = toml::from_str::<Settings>("auto_title = 'Daily journal'")
        .unwrap()
        .auto_title;
    let popup = EntryPopup::new_entry(&app.settings);
    let mut restored = EntryPopup::from_snapshot(&popup.snapshot().unwrap()).unwrap();
    assert_eq!(restored.title_txt.lines(), ["Daily journal"]);
    assert!(!restored.is_edit_entry);
    assert!(matches!(
        restored.handle_confirm(&mut app).await.unwrap(),
        EntryPopupInputReturn::AddEntry(_)
    ));
}

#[test]
fn malformed_form_snapshots_fail_without_exposing_their_private_values() {
    let error = match EntryPopup::from_snapshot(r#"{"private":"secret tag"}"#) {
        std::result::Result::Ok(_) => panic!("accepted invalid snapshot"),
        Err(error) => error,
    };
    assert_eq!(error.to_string(), "Invalid entry form snapshot");
    let popup = EntryPopup::new_entry(&Settings::default());
    let snapshot = popup
        .snapshot()
        .unwrap()
        .replacen("\"version\":1", "\"version\":999", 1);
    assert!(EntryPopup::from_snapshot(&snapshot).is_err());
}

#[tokio::test]
async fn hidden_priority_is_not_rendered_or_focused_in_new_and_edit_dialogs() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(1);
    app.settings.show_entry_priority = false;
    for mut popup in [
        EntryPopup::new_entry(&app.settings),
        EntryPopup::from_entry(app.get_current_entry().unwrap(), &app.settings),
    ] {
        let priority = popup.priority_txt.lines().to_vec();
        for code in [KeyCode::Tab, KeyCode::Down] {
            popup
                .handle_input(&Input::new(code, KeyModifiers::NONE), &mut app)
                .await
                .unwrap();
        }
        assert_eq!(popup.active_txt, ActiveText::Tags);
        popup
            .handle_input(
                &Input::new(KeyCode::Char('z'), KeyModifiers::NONE),
                &mut app,
            )
            .await
            .unwrap();
        assert!(popup.tags_txt.lines()[0].ends_with('z'));
        assert_eq!(popup.priority_txt.lines(), priority);
        popup
            .handle_input(&Input::new(KeyCode::Up, KeyModifiers::NONE), &mut app)
            .await
            .unwrap();
        assert_eq!(popup.active_txt, ActiveText::Date);
        for size in [(100, 30), (60, 18)] {
            let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
            terminal
                .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(text.contains("Title"));
            assert!(text.contains("Date"));
            assert!(text.contains("Tags"));
            assert!(!text.contains("Priority"));
        }
    }
    app.settings.show_entry_priority = true;
    let mut popup = EntryPopup::from_entry(app.get_current_entry().unwrap(), &app.settings);
    for _ in 0..2 {
        popup
            .handle_input(&Input::new(KeyCode::Tab, KeyModifiers::NONE), &mut app)
            .await
            .unwrap();
    }
    assert_eq!(popup.active_txt, ActiveText::Priority);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Priority"));
}

#[tokio::test]
async fn hidden_priority_survives_metadata_edits_and_encrypted_form_restoration() {
    for priority in [None, Some(0), Some(37), Some(u32::MAX)] {
        let mut app = create_default_app();
        app.load_entries().await.unwrap();
        app.current_entry_id = Some(1);
        let original = app.get_current_entry().unwrap().clone();
        app.update_current_entry_attributes(
            original.title.clone(),
            original.date,
            original.tags.clone(),
            priority,
        )
        .await
        .unwrap();
        app.settings.show_entry_priority = false;
        let mut popup = EntryPopup::from_entry(app.get_current_entry().unwrap(), &app.settings);
        popup.title_txt = TextArea::new(vec!["Renamed entry".into()]);
        popup.tags_txt = TextArea::new(vec!["updated tag".into()]);
        let mut restored = EntryPopup::from_snapshot(&popup.snapshot().unwrap()).unwrap();
        assert!(!restored.show_priority);
        assert_eq!(
            restored.handle_confirm(&mut app).await.unwrap(),
            EntryPopupInputReturn::UpdateCurrentEntry
        );
        app.load_entries().await.unwrap();
        let saved = app.get_current_entry().unwrap();
        assert_eq!(saved.priority, priority);
        assert_eq!(saved.title, "Renamed entry");
        assert_eq!(saved.tags, ["updated tag"]);
        assert_eq!(saved.date, original.date);
        assert_eq!(saved.content, original.content);
    }
}

#[tokio::test]
async fn hidden_priority_keeps_new_entry_defaults_and_auto_title() {
    for priority in [None, Some(0), Some(5)] {
        let mut app = create_default_app();
        app.settings =
            toml::from_str("show_entry_priority = false\nauto_title = 'Daily journal'").unwrap();
        app.settings.default_journal_priority = priority;
        let popup = EntryPopup::new_entry(&app.settings);
        let mut popup = EntryPopup::from_snapshot(&popup.snapshot().unwrap()).unwrap();
        assert!(!popup.show_priority);
        let EntryPopupInputReturn::AddEntry(id) = popup.handle_confirm(&mut app).await.unwrap()
        else {
            panic!("valid new entry was not added");
        };
        app.load_entries().await.unwrap();
        let saved = app.get_entry(id).unwrap();
        assert_eq!(saved.title, "Daily journal");
        assert_eq!(saved.priority, priority);
    }
}
